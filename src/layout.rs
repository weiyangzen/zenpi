//! Pure, terminal-independent BentoBox layout primitives.
//!
//! The TUI should eventually be one consumer of this module, not the owner of
//! its geometry rules.  Keeping the model free of `ratatui` makes breakpoints,
//! pane minimums, and tab presets cheap to test and safe to reuse from the
//! headless protocol or a future desktop client.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// Wire/schema version for persisted layout preferences.
pub const LAYOUT_SCHEMA_VERSION: u16 = 1;

/// Widths at which the responsive layout changes shape.
pub const COMPACT_WIDTH: u16 = 80;
pub const STANDARD_WIDTH: u16 = 100;
pub const WIDE_WIDTH: u16 = 160;

/// The five workspace tabs exposed by the v2 product contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TabId {
    Project,
    Goal,
    Learn,
    Review,
    Session,
}

impl TabId {
    pub const ALL: [Self; 5] = [
        Self::Project,
        Self::Goal,
        Self::Learn,
        Self::Review,
        Self::Session,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Goal => "goal",
            Self::Learn => "learn",
            Self::Review => "review",
            Self::Session => "session",
        }
    }
}

impl std::fmt::Display for TabId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Stable pane names.  Optional browser and terminal panes are represented in
/// presets even when their adapters are disabled, so a saved layout can be
/// restored without changing its identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaneId {
    ProjectConversation,
    Resources,
    GoalConversation,
    Gantt,
    Browser,
    Terminal,
    LearnConversation,
    LearnResources,
    LearnQueue,
    LearnMapping,
    Evidence,
    ReviewConversation,
    Checks,
    ApprovalQueue,
    Diff,
    SessionList,
    SessionConversation,
    ReplayControls,
    EventTimeline,
}

impl PaneId {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ProjectConversation => "project_conversation",
            Self::Resources => "resources",
            Self::GoalConversation => "goal_conversation",
            Self::Gantt => "gantt",
            Self::Browser => "browser",
            Self::Terminal => "terminal",
            Self::LearnConversation => "learn_conversation",
            Self::LearnResources => "learn_resources",
            Self::LearnQueue => "learn_queue",
            Self::LearnMapping => "learn_mapping",
            Self::Evidence => "evidence",
            Self::ReviewConversation => "review_conversation",
            Self::Checks => "checks",
            Self::ApprovalQueue => "approval_queue",
            Self::Diff => "diff",
            Self::SessionList => "session_list",
            Self::SessionConversation => "session_conversation",
            Self::ReplayControls => "replay_controls",
            Self::EventTimeline => "event_timeline",
        }
    }

    pub const fn is_optional(self) -> bool {
        matches!(self, Self::Browser | Self::Terminal)
    }
}

impl std::fmt::Display for PaneId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Responsive layout bands.  A zero-sized viewport is kept separate from a
/// narrow viewport because terminal libraries can report 0x0 while resizing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Breakpoint {
    Wide,
    Standard,
    Compact,
    Narrow,
    Zero,
}

impl Breakpoint {
    pub const fn for_size(width: u16, height: u16) -> Self {
        if width == 0 || height == 0 {
            Self::Zero
        } else if width < COMPACT_WIDTH {
            Self::Narrow
        } else if width < STANDARD_WIDTH {
            Self::Compact
        } else if width < WIDE_WIDTH {
            Self::Standard
        } else {
            Self::Wide
        }
    }

    pub const fn for_width(width: u16) -> Self {
        Self::for_size(width, 1)
    }
}

/// A two-dimensional terminal viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Viewport {
    pub width: u16,
    pub height: u16,
}

impl Viewport {
    pub const fn new(width: u16, height: u16) -> Self {
        Self { width, height }
    }
}

/// A cell rectangle.  Coordinates and extents are deliberately `u16` to
/// match terminal APIs, while internal calculations use wider integers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PaneRect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

impl PaneRect {
    pub const fn new(x: u16, y: u16, width: u16, height: u16) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub const fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }

    pub const fn right(self) -> u16 {
        self.x.saturating_add(self.width)
    }

    pub const fn bottom(self) -> u16 {
        self.y.saturating_add(self.height)
    }

    pub const fn contains(self, x: u16, y: u16) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    pub const fn intersects(self, other: Self) -> bool {
        !self.is_empty()
            && !other.is_empty()
            && self.x < other.right()
            && other.x < self.right()
            && self.y < other.bottom()
            && other.y < self.bottom()
    }
}

/// Minimum cells required by a pane before it becomes useful.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MinSize {
    pub width: u16,
    pub height: u16,
}

impl MinSize {
    pub const fn new(width: u16, height: u16) -> Self {
        Self { width, height }
    }
}

/// A column in the three-column wide preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Column {
    Left,
    Center,
    Right,
}

impl Column {
    const fn index(self) -> usize {
        match self {
            Self::Left => 0,
            Self::Center => 1,
            Self::Right => 2,
        }
    }
}

/// Relative width weights for the left/center/right columns.  The values are
/// weights rather than percentages; callers may use any positive values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnRatios {
    pub left: u16,
    pub center: u16,
    pub right: u16,
}

impl Default for ColumnRatios {
    fn default() -> Self {
        Self {
            left: 30,
            center: 45,
            right: 25,
        }
    }
}

impl ColumnRatios {
    pub const fn new(left: u16, center: u16, right: u16) -> Self {
        Self {
            left,
            center,
            right,
        }
    }
}

/// One pane in a preset.  `row` controls ordering within its column and
/// `row_weight` controls the height share of that row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneSpec {
    pub id: PaneId,
    pub column: Column,
    pub row: u8,
    pub row_weight: u16,
    pub min_size: MinSize,
    pub optional: bool,
}

impl PaneSpec {
    pub const fn new(
        id: PaneId,
        column: Column,
        row: u8,
        row_weight: u16,
        min_size: MinSize,
        optional: bool,
    ) -> Self {
        Self {
            id,
            column,
            row,
            row_weight: if row_weight == 0 { 1 } else { row_weight },
            min_size,
            optional,
        }
    }
}

/// A complete named preset.  Presets contain no terminal state and can be
/// serialized or inspected by a headless client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayoutPreset {
    pub tab: TabId,
    pub ratios: ColumnRatios,
    pub panes: Vec<PaneSpec>,
}

impl LayoutPreset {
    pub fn for_tab(tab: TabId) -> Self {
        let ratios = ColumnRatios::default();
        let panes = match tab {
            TabId::Project => vec![
                pane(
                    PaneId::ProjectConversation,
                    Column::Left,
                    0,
                    45,
                    24,
                    4,
                    false,
                ),
                pane(PaneId::Resources, Column::Left, 1, 30, 22, 3, false),
                pane(PaneId::GoalConversation, Column::Left, 2, 25, 24, 4, false),
                pane(PaneId::Gantt, Column::Center, 0, 100, 36, 6, false),
                pane(PaneId::Browser, Column::Right, 0, 55, 32, 6, true),
                pane(PaneId::Terminal, Column::Right, 1, 45, 32, 6, true),
            ],
            TabId::Goal => vec![
                pane(PaneId::GoalConversation, Column::Left, 0, 50, 24, 4, false),
                pane(PaneId::Resources, Column::Left, 1, 25, 22, 3, false),
                pane(
                    PaneId::ProjectConversation,
                    Column::Left,
                    2,
                    25,
                    24,
                    4,
                    false,
                ),
                pane(PaneId::Gantt, Column::Center, 0, 70, 36, 6, false),
                pane(PaneId::EventTimeline, Column::Center, 1, 30, 36, 4, false),
                pane(PaneId::Browser, Column::Right, 0, 55, 32, 6, true),
                pane(PaneId::Terminal, Column::Right, 1, 45, 32, 6, true),
            ],
            TabId::Learn => vec![
                pane(PaneId::LearnConversation, Column::Left, 0, 45, 26, 4, false),
                pane(PaneId::LearnResources, Column::Left, 1, 35, 22, 3, false),
                pane(PaneId::LearnQueue, Column::Left, 2, 20, 22, 3, false),
                pane(PaneId::LearnMapping, Column::Center, 0, 65, 36, 6, false),
                pane(PaneId::Evidence, Column::Center, 1, 35, 36, 4, false),
                pane(PaneId::Browser, Column::Right, 0, 55, 32, 6, true),
                pane(PaneId::Terminal, Column::Right, 1, 45, 32, 6, true),
            ],
            TabId::Review => vec![
                pane(
                    PaneId::ReviewConversation,
                    Column::Left,
                    0,
                    45,
                    26,
                    4,
                    false,
                ),
                pane(PaneId::Checks, Column::Left, 1, 35, 22, 3, false),
                pane(PaneId::ApprovalQueue, Column::Left, 2, 20, 22, 3, false),
                pane(PaneId::Diff, Column::Center, 0, 100, 44, 8, false),
                pane(PaneId::Terminal, Column::Right, 0, 55, 32, 6, true),
                pane(PaneId::Browser, Column::Right, 1, 45, 32, 6, true),
            ],
            TabId::Session => vec![
                pane(PaneId::SessionList, Column::Left, 0, 45, 24, 4, false),
                pane(
                    PaneId::SessionConversation,
                    Column::Left,
                    1,
                    35,
                    26,
                    4,
                    false,
                ),
                pane(PaneId::ReplayControls, Column::Left, 2, 20, 24, 3, false),
                pane(PaneId::EventTimeline, Column::Center, 0, 65, 36, 6, false),
                pane(PaneId::Gantt, Column::Center, 1, 35, 36, 4, false),
                pane(PaneId::Browser, Column::Right, 0, 55, 32, 6, true),
                pane(PaneId::Terminal, Column::Right, 1, 45, 32, 6, true),
            ],
        };
        Self { tab, ratios, panes }
    }

    pub fn project() -> Self {
        Self::for_tab(TabId::Project)
    }

    pub fn goal() -> Self {
        Self::for_tab(TabId::Goal)
    }

    pub fn learn() -> Self {
        Self::for_tab(TabId::Learn)
    }

    pub fn review() -> Self {
        Self::for_tab(TabId::Review)
    }

    pub fn session() -> Self {
        Self::for_tab(TabId::Session)
    }
}

fn pane(
    id: PaneId,
    column: Column,
    row: u8,
    row_weight: u16,
    min_width: u16,
    min_height: u16,
    optional: bool,
) -> PaneSpec {
    PaneSpec::new(
        id,
        column,
        row,
        row_weight,
        MinSize::new(min_width, min_height),
        optional,
    )
}

/// Capability switches for optional panes.  All other panes are local and
/// available in the default binary.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneCapabilities {
    pub browser: bool,
    pub terminal: bool,
}

/// User-owned layout state.  The preset remains derived from `tab`, so a
/// corrupt or unknown preset cannot smuggle arbitrary geometry into a host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayoutModel {
    pub tab: TabId,
    pub ratios: ColumnRatios,
    pub collapsed: BTreeSet<PaneId>,
    pub focused: Option<PaneId>,
    pub capabilities: PaneCapabilities,
}

impl LayoutModel {
    pub fn new(tab: TabId) -> Self {
        Self {
            tab,
            ratios: LayoutPreset::for_tab(tab).ratios,
            collapsed: BTreeSet::new(),
            focused: None,
            capabilities: PaneCapabilities::default(),
        }
    }

    pub fn preset(&self) -> LayoutPreset {
        LayoutPreset::for_tab(self.tab)
    }

    pub fn with_capabilities(mut self, capabilities: PaneCapabilities) -> Self {
        self.capabilities = capabilities;
        self
    }

    pub fn set_capabilities(&mut self, capabilities: PaneCapabilities) {
        self.capabilities = capabilities;
    }

    pub fn set_ratios(&mut self, ratios: ColumnRatios) {
        self.ratios = ratios;
    }

    pub fn set_focused(&mut self, pane: Option<PaneId>) {
        self.focused = pane;
    }

    pub fn set_collapsed(&mut self, pane: PaneId, collapsed: bool) {
        if collapsed {
            self.collapsed.insert(pane);
        } else {
            self.collapsed.remove(&pane);
        }
    }

    pub fn toggle_collapsed(&mut self, pane: PaneId) -> bool {
        if !self.collapsed.insert(pane) {
            self.collapsed.remove(&pane);
            false
        } else {
            true
        }
    }

    pub fn compute(&self, width: u16, height: u16) -> LayoutSnapshot {
        self.compute_viewport(Viewport::new(width, height))
    }

    pub fn compute_viewport(&self, viewport: Viewport) -> LayoutSnapshot {
        let preset = self.preset();
        let breakpoint = Breakpoint::for_size(viewport.width, viewport.height);
        let mut states = preset
            .panes
            .iter()
            .copied()
            .map(|spec| PaneState::new(spec, self.is_available(spec)))
            .collect::<Vec<_>>();

        match breakpoint {
            Breakpoint::Zero => {
                for state in &mut states {
                    if state.visibility != Visibility::Unavailable {
                        state.visibility = Visibility::Hidden;
                    }
                }
            }
            Breakpoint::Narrow => {
                let focus = self
                    .focused
                    .filter(|pane| {
                        states.iter().any(|state| {
                            state.spec.id == *pane
                                && state.is_available()
                                && !self.collapsed.contains(pane)
                        })
                    })
                    .or_else(|| {
                        states
                            .iter()
                            .find(|state| {
                                state.is_available()
                                    && !state.spec.optional
                                    && !self.collapsed.contains(&state.spec.id)
                            })
                            .map(|state| state.spec.id)
                    })
                    .or_else(|| {
                        states
                            .iter()
                            .find(|state| {
                                state.is_available() && !self.collapsed.contains(&state.spec.id)
                            })
                            .map(|state| state.spec.id)
                    });
                for state in &mut states {
                    if state.visibility != Visibility::Unavailable {
                        state.visibility = if Some(state.spec.id) == focus {
                            Visibility::Visible
                        } else {
                            Visibility::Collapsed
                        };
                    }
                }
            }
            Breakpoint::Compact => {
                for state in &mut states {
                    if state.spec.column == Column::Right && state.is_available() {
                        state.visibility = Visibility::Collapsed;
                    }
                }
            }
            Breakpoint::Standard | Breakpoint::Wide => {}
        }

        // User collapse and disabled adapters apply after breakpoint defaults.
        for state in &mut states {
            if !state.is_available() {
                state.visibility = Visibility::Unavailable;
            } else if self.collapsed.contains(&state.spec.id)
                && state.visibility == Visibility::Visible
            {
                state.visibility = Visibility::Collapsed;
            }
        }

        // If custom ratios make a column smaller than one of its required
        // panes, remove optional panes from that column before allocating.
        if !matches!(breakpoint, Breakpoint::Zero | Breakpoint::Narrow) {
            collapse_optional_for_width(&mut states, viewport.width, self.ratios);
        }

        let visible = states
            .iter()
            .filter(|state| state.visibility == Visibility::Visible)
            .collect::<Vec<_>>();
        let mut columns = [Vec::new(), Vec::new(), Vec::new()];
        for state in visible {
            columns[state.spec.column.index()].push(state);
        }
        for column in &mut columns {
            column.sort_by_key(|state| state.spec.row);
        }

        let column_specs = columns
            .iter()
            .enumerate()
            .filter_map(|(index, entries)| {
                (!entries.is_empty()).then_some((
                    match index {
                        0 => self.ratios.left.max(1),
                        1 => self.ratios.center.max(1),
                        _ => self.ratios.right.max(1),
                    },
                    entries
                        .iter()
                        .map(|state| state.spec.min_size.width)
                        .max()
                        .unwrap_or(0),
                ))
            })
            .collect::<Vec<_>>();
        let column_lengths = allocate_lengths(viewport.width, &column_specs);

        let mut panes = states
            .iter()
            .map(|state| PaneLayout {
                id: state.spec.id,
                rect: PaneRect::default(),
                visibility: state.visibility,
                min_size: state.spec.min_size,
            })
            .collect::<Vec<_>>();

        let mut x = 0_u16;
        for (active_column, entries) in columns
            .iter()
            .filter(|entries| !entries.is_empty())
            .enumerate()
        {
            let width = column_lengths.get(active_column).copied().unwrap_or(0);
            let row_specs = entries
                .iter()
                .map(|state| (state.spec.row_weight, state.spec.min_size.height))
                .collect::<Vec<_>>();
            let row_lengths = allocate_lengths(viewport.height, &row_specs);
            let mut y = 0_u16;
            for (state, height) in entries.iter().zip(row_lengths) {
                if let Some(layout) = panes.iter_mut().find(|pane| pane.id == state.spec.id) {
                    layout.rect = PaneRect::new(x, y, width, height);
                }
                y = y.saturating_add(height);
            }
            x = x.saturating_add(width);
        }

        LayoutSnapshot {
            tab: self.tab,
            viewport,
            breakpoint,
            panes,
        }
    }

    fn is_available(&self, spec: PaneSpec) -> bool {
        if !spec.optional {
            return true;
        }
        match spec.id {
            PaneId::Browser => self.capabilities.browser,
            PaneId::Terminal => self.capabilities.terminal,
            _ => true,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct PaneState {
    spec: PaneSpec,
    visibility: Visibility,
}

impl PaneState {
    fn new(spec: PaneSpec, available: bool) -> Self {
        Self {
            spec,
            visibility: if available {
                Visibility::Visible
            } else {
                Visibility::Unavailable
            },
        }
    }

    fn is_available(self) -> bool {
        self.visibility != Visibility::Unavailable
    }
}

/// Visibility status for one pane in a computed snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    Visible,
    Collapsed,
    Unavailable,
    Hidden,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneLayout {
    pub id: PaneId,
    pub rect: PaneRect,
    pub visibility: Visibility,
    pub min_size: MinSize,
}

/// Computed, deterministic layout output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayoutSnapshot {
    pub tab: TabId,
    pub viewport: Viewport,
    pub breakpoint: Breakpoint,
    pub panes: Vec<PaneLayout>,
}

impl LayoutSnapshot {
    pub fn pane(&self, id: PaneId) -> Option<&PaneLayout> {
        self.panes.iter().find(|pane| pane.id == id)
    }

    pub fn visible_panes(&self) -> impl Iterator<Item = &PaneLayout> {
        self.panes
            .iter()
            .filter(|pane| pane.visibility == Visibility::Visible)
    }

    pub fn visible_rects_non_overlapping(&self) -> bool {
        let visible = self.visible_panes().collect::<Vec<_>>();
        visible.iter().enumerate().all(|(index, pane)| {
            visible[index + 1..]
                .iter()
                .all(|other| !pane.rect.intersects(other.rect))
        })
    }
}

fn collapse_optional_for_width(
    states: &mut [PaneState],
    viewport_width: u16,
    ratios: ColumnRatios,
) {
    loop {
        let mut active = [false; 3];
        let mut minimums = [0_u16; 3];
        for state in states
            .iter()
            .filter(|state| state.visibility == Visibility::Visible)
        {
            let index = state.spec.column.index();
            active[index] = true;
            minimums[index] = minimums[index].max(state.spec.min_size.width);
        }
        let specs = (0..3)
            .filter(|index| active[*index])
            .map(|index| {
                (
                    match index {
                        0 => ratios.left.max(1),
                        1 => ratios.center.max(1),
                        _ => ratios.right.max(1),
                    },
                    minimums[index],
                )
            })
            .collect::<Vec<_>>();
        // Compare the requested ratio with the pane minimum before minimum
        // floors are applied.  Otherwise the allocator would silently grow a
        // tiny optional column to its minimum and defeat the user's ratio.
        let raw_lengths = weighted_partition(
            u32::from(viewport_width),
            specs.iter().map(|(weight, _)| *weight).collect(),
        );
        let lengths = allocate_lengths(viewport_width, &specs);
        let mut length_index = 0;
        let mut to_collapse = None;
        for index in 0..3 {
            if !active[index] {
                continue;
            }
            let raw_length = raw_lengths.get(length_index).copied().unwrap_or(0);
            let length = lengths.get(length_index).copied().unwrap_or(0);
            length_index += 1;
            if (raw_length < minimums[index] || length < minimums[index])
                && let Some(candidate) = states
                    .iter()
                    .filter(|state| {
                        state.visibility == Visibility::Visible
                            && state.spec.column.index() == index
                            && state.spec.optional
                    })
                    .max_by_key(|state| state.spec.min_size.width)
            {
                to_collapse = Some(candidate.spec.id);
                break;
            }
        }
        let Some(id) = to_collapse else { break };
        if let Some(state) = states.iter_mut().find(|state| state.spec.id == id) {
            state.visibility = Visibility::Collapsed;
        }
    }
}

/// Allocate a total length using weights while satisfying minimums whenever
/// the viewport is large enough.  The result always sums to `total`.
fn allocate_lengths(total: u16, specs: &[(u16, u16)]) -> Vec<u16> {
    if specs.is_empty() {
        return Vec::new();
    }
    let total_u32 = u32::from(total);
    let weight_sum = specs
        .iter()
        .map(|(weight, _)| u32::from((*weight).max(1)))
        .sum::<u32>();
    let min_sum = specs
        .iter()
        .map(|(_, minimum)| u32::from(*minimum))
        .sum::<u32>();

    if min_sum > total_u32 {
        return weighted_partition(total_u32, specs.iter().map(|(weight, _)| *weight).collect());
    }

    let mut result = specs
        .iter()
        .map(|(_, minimum)| *minimum)
        .collect::<Vec<_>>();
    let remaining = total_u32 - min_sum;
    let mut remainders = Vec::with_capacity(specs.len());
    let mut distributed = 0_u32;
    for (index, (weight, _)) in specs.iter().enumerate() {
        let numerator = remaining * u32::from((*weight).max(1));
        let share = numerator / weight_sum;
        result[index] = result[index].saturating_add(share.min(u32::from(u16::MAX)) as u16);
        distributed += share;
        remainders.push((numerator % weight_sum, index));
    }
    let mut leftover = remaining.saturating_sub(distributed);
    remainders.sort_by(|left, right| right.cmp(left));
    let mut index = 0;
    while leftover > 0 {
        let target = remainders[index % remainders.len()].1;
        result[target] = result[target].saturating_add(1);
        leftover -= 1;
        index += 1;
    }
    result
}

fn weighted_partition(total: u32, weights: Vec<u16>) -> Vec<u16> {
    if weights.is_empty() {
        return Vec::new();
    }
    let sum = weights
        .iter()
        .map(|weight| u32::from((*weight).max(1)))
        .sum::<u32>();
    let mut result = Vec::with_capacity(weights.len());
    let mut remainders = Vec::with_capacity(weights.len());
    let mut used = 0_u32;
    for (index, weight) in weights.iter().enumerate() {
        let numerator = total * u32::from((*weight).max(1));
        let share = numerator / sum;
        result.push(share.min(u32::from(u16::MAX)) as u16);
        used += share;
        remainders.push((numerator % sum, index));
    }
    let mut leftover = total.saturating_sub(used);
    remainders.sort_by(|left, right| right.cmp(left));
    let mut index = 0;
    while leftover > 0 {
        let target = remainders[index % remainders.len()].1;
        result[target] = result[target].saturating_add(1);
        leftover -= 1;
        index += 1;
    }
    result
}
