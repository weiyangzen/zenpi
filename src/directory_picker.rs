//! Bounded terminal-native directory selection. Never creates a project.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    Frame,
    layout::{Position, Rect},
    text::Line,
    widgets::{Block, Borders, Clear, Paragraph},
};
use std::{
    fs,
    path::{Path, PathBuf},
};
use unicode_segmentation::UnicodeSegmentation;

fn visible_path(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let cells = unicode_width::UnicodeWidthStr::width(text);
    if cells <= width {
        return text.to_owned();
    }
    if width == 1 {
        return "…".to_owned();
    }
    let mut result = String::from("…");
    let mut used = 1;
    for grapheme in text.graphemes(true).rev() {
        let cells = unicode_width::UnicodeWidthStr::width(grapheme);
        if used + cells > width {
            break;
        }
        result = format!(
            "…{grapheme}{}",
            result.strip_prefix('…').unwrap_or_default()
        );
        used += cells;
    }
    result
}

fn remove_last_grapheme(text: &mut String) {
    if let Some((start, _)) = text.grapheme_indices(true).next_back() {
        text.truncate(start);
    }
}

#[derive(Debug, Clone)]
pub struct DirectoryPicker {
    cwd: PathBuf,
    input: String,
    /// Type-ahead prefix within `cwd` (ZS1-170). While non-empty, printable
    /// keys extend the prefix and jump the selection instead of editing the
    /// path, so a few letters move straight to a folder.
    filter: String,
    entries: Vec<PathBuf>,
    selected: usize,
    error: String,
    area: Rect,
    offset: usize,
}
impl DirectoryPicker {
    pub fn new(base: &Path) -> Self {
        let mut picker = Self {
            cwd: base.to_path_buf(),
            input: base.display().to_string(),
            filter: String::new(),
            entries: Vec::new(),
            selected: 0,
            error: String::new(),
            area: Rect::default(),
            offset: 0,
        };
        picker.refresh();
        picker
    }
    pub fn input(&self) -> &str {
        &self.input
    }
    pub fn error(&self) -> &str {
        &self.error
    }
    fn refresh(&mut self) {
        self.entries.clear();
        self.selected = 0;
        self.offset = 0;
        self.error.clear();
        match fs::read_dir(&self.cwd) {
            Ok(entries) => {
                for entry in entries.take(2048) {
                    match entry {
                        Ok(entry)
                            if entry.path().is_dir()
                                && entry
                                    .path()
                                    .to_str()
                                    .is_some_and(|v| !v.chars().any(char::is_control)) =>
                        {
                            self.entries.push(entry.path())
                        }
                        Ok(_) => (),
                        Err(e) => self.error = e.to_string(),
                    }
                    if self.entries.len() == 512 {
                        self.error =
                            "Showing first 512 folders; type a path to open another.".into();
                        break;
                    }
                }
                self.entries.sort();
            }
            Err(e) => self.error = e.to_string(),
        }
    }
    pub fn paste(&mut self, text: &str) {
        let clean: String = text.chars().filter(|c| !c.is_control()).collect();
        if self.input.len() + clean.len() <= crate::project_workspace::MAX_PROJECT_PATH_BYTES {
            self.filter.clear();
            self.input.push_str(&clean);
        }
    }

    /// Jump the selection to the first entry whose name starts with the current
    /// type-ahead prefix, and mirror the partial name in the path line.
    fn apply_filter(&mut self) {
        if self.filter.is_empty() {
            self.input = self.cwd.display().to_string();
            return;
        }
        let needle = self.filter.to_lowercase();
        if let Some(index) = self.entries.iter().position(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.to_lowercase().starts_with(&needle))
        }) {
            self.selected = index;
        }
        self.input = self.cwd.join(&self.filter).display().to_string();
    }

    /// The type-ahead match currently selected, if any.
    fn filtered_entry(&self) -> Option<PathBuf> {
        if self.filter.is_empty() {
            return None;
        }
        self.entries.get(self.selected).cloned()
    }
    fn resolved(&self) -> Result<PathBuf, String> {
        let expanded = if self.input == "~" || self.input.starts_with("~/") {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .ok_or("Home directory unavailable")?
                .join(self.input.strip_prefix("~/").unwrap_or(""))
        } else {
            PathBuf::from(&self.input)
        };
        crate::project_workspace::normalize_directory(&expanded, &self.cwd)
            .map_err(|e| e.to_string())
    }
    fn confirm(&mut self) -> Option<Option<PathBuf>> {
        match self.resolved() {
            Ok(path) => match fs::read_dir(&path) {
                Ok(_) => Some(Some(path)),
                Err(e) => {
                    self.error = e.to_string();
                    None
                }
            },
            Err(e) => {
                self.error = e;
                None
            }
        }
    }
    fn enter(&mut self, path: PathBuf) {
        self.cwd = path;
        self.input = self.cwd.display().to_string();
        self.filter.clear();
        self.refresh();
    }
    /// Outer None means keep modal open; Some(None) means cancel.
    pub fn key(&mut self, key: KeyEvent) -> Option<Option<PathBuf>> {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('u') | KeyCode::Char('l') => {
                    self.input.clear();
                    self.filter.clear();
                    return None;
                }
                KeyCode::Char('w') => {
                    // Type-ahead is path-relative, so Ctrl-W first drops the
                    // filter, then walks the effective path one level up.
                    self.filter.clear();
                    let source = if self.input.trim().is_empty() {
                        self.cwd.clone()
                    } else {
                        PathBuf::from(self.input.trim_end_matches(std::path::MAIN_SEPARATOR))
                    };
                    let parent = source
                        .parent()
                        .map(|path| path.display().to_string())
                        .filter(|value| !value.is_empty())
                        .unwrap_or_else(|| self.cwd.display().to_string());
                    self.input = parent;
                    return None;
                }
                KeyCode::Char('c') => return Some(None),
                _ => return None,
            }
        }
        match key.code {
            KeyCode::Esc => return Some(None),
            KeyCode::Enter => {
                // In type-ahead mode Enter opens the highlighted folder.
                if let Some(path) = self.filtered_entry()
                    && let Ok(_) = fs::read_dir(&path)
                {
                    return Some(Some(path));
                }
                return self.confirm();
            }
            KeyCode::Backspace => {
                if !self.filter.is_empty() {
                    self.filter.pop();
                    self.apply_filter();
                } else {
                    remove_last_grapheme(&mut self.input);
                }
            }
            KeyCode::Char(c)
                if !c.is_control()
                    && self.filter.len() + self.cwd.as_os_str().len() + c.len_utf8()
                        <= crate::project_workspace::MAX_PROJECT_PATH_BYTES =>
            {
                self.filter.push(c);
                self.apply_filter();
            }
            KeyCode::Up => {
                if !self.filter.is_empty() {
                    self.selected = self.selected.saturating_sub(1);
                } else {
                    self.selected = self.selected.saturating_sub(1);
                    if let Some(p) = self.entries.get(self.selected) {
                        self.input = p.display().to_string();
                    }
                }
            }
            KeyCode::Down => {
                self.selected = (self.selected + 1).min(self.entries.len().saturating_sub(1));
                if self.filter.is_empty()
                    && let Some(p) = self.entries.get(self.selected)
                {
                    self.input = p.display().to_string();
                }
            }
            KeyCode::Left => {
                if let Some(p) = self.cwd.parent() {
                    self.enter(p.to_path_buf());
                }
            }
            KeyCode::Right => {
                if let Some(path) = self.filtered_entry() {
                    self.enter(path);
                } else if let Some(p) = self.entries.get(self.selected) {
                    self.enter(p.clone());
                }
            }
            KeyCode::Tab => {
                if let Some(path) = self.filtered_entry() {
                    self.enter(path);
                } else if let Ok(path) = self.resolved() {
                    self.enter(path);
                } else {
                    let path = if Path::new(&self.input).is_absolute() {
                        PathBuf::from(&self.input)
                    } else {
                        self.cwd.join(&self.input)
                    };
                    if let (Some(parent), Some(prefix)) =
                        (path.parent(), path.file_name().and_then(|s| s.to_str()))
                    {
                        let matches: Vec<_> = fs::read_dir(parent)
                            .into_iter()
                            .flatten()
                            .take(2048)
                            .filter_map(Result::ok)
                            .filter(|e| {
                                e.path().is_dir()
                                    && e.file_name().to_string_lossy().starts_with(prefix)
                            })
                            .take(2)
                            .collect();
                        if matches.len() == 1 {
                            self.input = matches[0].path().display().to_string();
                        } else {
                            self.error =
                                "Type more of the folder name, or use arrows to browse.".into();
                        }
                    }
                }
            }
            _ => (),
        }
        None
    }
    pub fn mouse(&mut self, mouse: MouseEvent) -> Option<Option<PathBuf>> {
        if mouse.kind != MouseEventKind::Down(MouseButton::Left)
            || !self.area.contains(Position::new(mouse.column, mouse.row))
        {
            return None;
        }
        let row = mouse.row.saturating_sub(self.area.y);
        if row == self.area.height.saturating_sub(2) {
            let x = mouse.column.saturating_sub(self.area.x);
            if x < 14 {
                return self.confirm();
            }
            if x < 28 {
                return Some(None);
            }
            if let Some(parent) = self.cwd.parent() {
                self.enter(parent.to_path_buf());
            }
        } else if row >= 4 && row < self.area.height.saturating_sub(3) {
            let index = self.offset + usize::from(row - 4);
            if let Some(path) = self.entries.get(index) {
                self.enter(path.clone());
            }
        }
        None
    }
    pub fn render(&mut self, frame: &mut Frame<'_>) {
        let area = frame.area();
        let width = area.width.saturating_sub(4).min(100);
        let height = area.height.saturating_sub(2).min(24);
        self.area = Rect::new(
            area.x + (area.width - width) / 2,
            area.y + (area.height - height) / 2,
            width,
            height,
        );
        frame.render_widget(Clear, self.area);
        frame.render_widget(
            Block::default()
                .borders(Borders::ALL)
                .title(" Open project folder "),
            self.area,
        );
        if width < 8 || height < 8 {
            return;
        }
        let inner = Rect::new(self.area.x + 1, self.area.y + 1, width - 2, height - 2);
        let path_width = usize::from(inner.width).saturating_sub(6);
        frame.render_widget(
            Paragraph::new(format!("Path: {}", visible_path(&self.input, path_width))),
            Rect::new(inner.x, inner.y, inner.width, 1),
        );
        frame.render_widget(
            Paragraph::new(
                "Type to jump to a folder | Enter/Tab open | arrows browse | Ctrl-W parent | Ctrl-U clear",
            ),
            Rect::new(inner.x, inner.y + 1, inner.width, 1),
        );
        frame.render_widget(
            Paragraph::new(format!("In: {}", self.cwd.display())),
            Rect::new(inner.x, inner.y + 2, inner.width, 1),
        );
        let rows = usize::from(height - 7);
        if self.selected < self.offset {
            self.offset = self.selected;
        }
        if self.selected >= self.offset + rows {
            self.offset = self.selected.saturating_sub(rows - 1);
        }
        let lines: Vec<_> = self
            .entries
            .iter()
            .enumerate()
            .skip(self.offset)
            .take(rows)
            .map(|(i, p)| {
                Line::from(format!(
                    "{} {}/",
                    if i == self.selected { ">" } else { " " },
                    p.file_name().unwrap_or_default().to_string_lossy()
                ))
            })
            .collect();
        frame.render_widget(
            Paragraph::new(if lines.is_empty() {
                vec![Line::from("(No subfolders; Enter opens this folder)")]
            } else {
                lines
            }),
            Rect::new(inner.x, inner.y + 3, inner.width, height - 7),
        );
        frame.render_widget(
            Paragraph::new(self.error.as_str()),
            Rect::new(inner.x, self.area.bottom() - 3, inner.width, 1),
        );
        frame.render_widget(
            Paragraph::new("[Open Enter]  [Cancel Esc]  [Parent]"),
            Rect::new(inner.x, self.area.bottom() - 2, inner.width, 1),
        );
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{remove_last_grapheme, visible_path};

    #[test]
    fn visible_path_keeps_the_selected_folder_tail() {
        let shortened = visible_path("/Users/example/workspaces/project-a", 12);
        assert!(shortened.ends_with("project-a"));
        assert_eq!(visible_path("工作目录", 8), "工作目录");
        assert_eq!(visible_path("深/工作目录", 5), "…目录");
    }

    #[test]
    fn backspace_removes_one_unicode_grapheme_without_splitting_it() {
        let mut text = "a\u{301}👩‍💻".to_owned();
        remove_last_grapheme(&mut text);
        assert_eq!(text, "a\u{301}");
        remove_last_grapheme(&mut text);
        assert_eq!(text, "");
    }

    #[test]
    fn ctrl_w_moves_the_path_input_to_its_parent() {
        let root = std::env::temp_dir().join(format!("zenpi-picker-{}", std::process::id()));
        let nested = root.join("one").join("two");
        std::fs::create_dir_all(&nested).unwrap();
        let mut picker = super::DirectoryPicker::new(&nested);
        let result = picker.key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('w'),
            crossterm::event::KeyModifiers::CONTROL,
        ));
        assert!(result.is_none());
        assert_eq!(picker.input(), root.join("one").display().to_string());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ctrl_w_keeps_the_root_path_at_root() {
        let mut picker = super::DirectoryPicker::new(Path::new("/"));
        picker.key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('w'),
            crossterm::event::KeyModifiers::CONTROL,
        ));
        assert_eq!(picker.input(), "/");
    }

    #[test]
    fn ctrl_w_skips_a_trailing_separator_and_moves_to_the_parent() {
        let mut picker = super::DirectoryPicker::new(Path::new("/tmp/example/"));
        picker.key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('w'),
            crossterm::event::KeyModifiers::CONTROL,
        ));
        assert_eq!(picker.input(), "/tmp");
    }
}
