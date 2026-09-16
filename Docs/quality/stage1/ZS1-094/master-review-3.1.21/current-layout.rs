//! Pure, terminal-independent BentoBox layout primitives.
//!
//! The TUI should eventually be one consumer of this module, not the owner of
//! its geometry rules.  Keeping the model free of `ratatui` makes breakpoints,
//! pane minimums, and tab presets cheap to test and safe to reuse from the
//! headless protocol or a future desktop client.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Wire/schema version for persisted layout preferences.
pub const LAYOUT_SCHEMA_VERSION: u16 = 1;

/// Widths at which the responsive layout changes shape.
pub const COMPACT_WIDTH: u16 = 80;
pub const STANDARD_WIDTH: u16 = 100;
pub const WIDE_WIDTH: u16 = 160;

/// Legacy pane-preset identifiers. The TUI presents these as projections of a
/// project workspace; they are not project identities. Project identity is
/// owned by the session/workspace host and may be changed without changing
/// this enum.
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

/// Maximum number of bytes accepted for the user-owned BentoBox preference
/// snapshot.  Layout preferences are intentionally tiny; this guard keeps a
/// damaged or hostile file from turning startup into an unbounded allocation.
pub const MAX_LAYOUT_PREFERENCES_BYTES: usize = 256 * 1024;
/// Maximum number of provider profiles that may own layout preferences.
pub const MAX_LAYOUT_PROFILES: usize = 64;
/// Maximum number of pane entries retained in one tab's collapsed set.
pub const MAX_LAYOUT_COLLAPSED_PANES: usize = 64;

/// A validated, serializable snapshot of one tab's user-owned state.  Pane
/// capabilities are deliberately omitted: browser/PTY availability belongs
/// to the host and is applied after this state is loaded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersistedTabLayout {
    pub ratios: ColumnRatios,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub row_weights: BTreeMap<PaneId, u16>,
    #[serde(default)]
    pub collapsed: BTreeSet<PaneId>,
    #[serde(default)]
    pub focused: Option<PaneId>,
}

impl PersistedTabLayout {
    pub fn from_model(model: &LayoutModel) -> Self {
        Self {
            ratios: model.ratios,
            row_weights: model.row_weights.clone(),
            collapsed: model.collapsed.clone(),
            focused: model.focused,
        }
    }

    fn apply_to_model(&self, model: &mut LayoutModel) {
        model.ratios = self.ratios;
        model.row_weights = self.row_weights.clone();
        model.collapsed = self.collapsed.clone();
        model.focused = self.focused;
    }
}

/// Preferences for one named provider profile.  A profile can have one
/// independent BentoBox state per upper workspace tab.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileLayoutPreferences {
    #[serde(default)]
    pub tabs: BTreeMap<TabId, PersistedTabLayout>,
}

/// On-disk BentoBox preference document.  It is separate from provider
/// credentials/configuration so a malformed layout can never invalidate a
/// working provider profile, and so reset can replace only this file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutPreferences {
    pub schema_version: u16,
    #[serde(default)]
    pub profiles: BTreeMap<String, ProfileLayoutPreferences>,
}

impl Default for LayoutPreferences {
    fn default() -> Self {
        Self {
            schema_version: LAYOUT_SCHEMA_VERSION,
            profiles: BTreeMap::new(),
        }
    }
}

/// Validation failures are kept typed so callers can fail closed without
/// guessing whether a file was corrupt, stale, or out of bounds.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LayoutError {
    #[error("unsupported layout schema {found} (expected {expected})")]
    SchemaVersion { found: u16, expected: u16 },
    #[error("layout profile `{0}` is invalid")]
    InvalidProfile(String),
    #[error("layout preferences contain too many profiles (maximum {0})")]
    TooManyProfiles(usize),
    #[error("layout profile `{profile}` contains too many tabs (maximum {max})")]
    TooManyTabs { profile: String, max: usize },
    #[error("layout tab `{tab}` has out-of-range ratios")]
    InvalidRatios { tab: TabId },
    #[error("layout tab `{tab}` has too many collapsed panes (maximum {max})")]
    TooManyCollapsed { tab: TabId, max: usize },
    #[error("pane `{pane}` does not belong to tab `{tab}`")]
    UnknownPane { tab: TabId, pane: PaneId },
    #[error("layout JSON: {0}")]
    Json(String),
}

impl LayoutPreferences {
    /// Validate schema, profile names, bounds, and pane identity.  The
    /// validator rejects rather than clamps values so a typo cannot silently
    /// change the user's layout or make geometry unsafe.
    pub fn validate(&self) -> Result<(), LayoutError> {
        if self.schema_version != LAYOUT_SCHEMA_VERSION {
            return Err(LayoutError::SchemaVersion {
                found: self.schema_version,
                expected: LAYOUT_SCHEMA_VERSION,
            });
        }
        if self.profiles.len() > MAX_LAYOUT_PROFILES {
            return Err(LayoutError::TooManyProfiles(MAX_LAYOUT_PROFILES));
        }
        for (profile, preferences) in &self.profiles {
            validate_layout_profile_name(profile)?;
            if preferences.tabs.len() > TabId::ALL.len() {
                return Err(LayoutError::TooManyTabs {
                    profile: profile.clone(),
                    max: TabId::ALL.len(),
                });
            }
            for (tab, state) in &preferences.tabs {
                validate_tab_state(*tab, state)?;
            }
        }
        Ok(())
    }

    /// Decode a bounded JSON document and validate it before returning it.
    /// This is intentionally a layout-level helper so headless and TUI hosts
    /// can share exactly the same migration and fail-closed behavior.
    pub fn from_json_bytes(bytes: &[u8]) -> Result<Self, LayoutError> {
        Self::from_json_bytes_with_migration(bytes).map(|(preferences, _)| preferences)
    }

    /// Decode a document and report whether it used the pre-v1 single-model
    /// shape. The caller may then explicitly persist the returned v1 document
    /// through the config store; ordinary reads remain side-effect free.
    pub fn from_json_bytes_with_migration(bytes: &[u8]) -> Result<(Self, bool), LayoutError> {
        if bytes.len() > MAX_LAYOUT_PREFERENCES_BYTES {
            return Err(LayoutError::Json(format!(
                "layout preferences exceed {} bytes",
                MAX_LAYOUT_PREFERENCES_BYTES
            )));
        }
        let value: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|error| LayoutError::Json(error.to_string()))?;
        let needs_migration = value.get("schema_version").is_none();
        let preferences = Self::migrate_value(value)?;
        preferences.validate()?;
        Ok((preferences, needs_migration))
    }

    /// Serialize a validated snapshot with a deterministic size bound.
    pub fn to_json_bytes(&self) -> Result<Vec<u8>, LayoutError> {
        self.validate()?;
        let mut bytes = serde_json::to_vec_pretty(self)
            .map_err(|error| LayoutError::Json(error.to_string()))?;
        bytes.push(b'\n');
        if bytes.len() > MAX_LAYOUT_PREFERENCES_BYTES {
            return Err(LayoutError::Json(format!(
                "layout preferences exceed {} bytes",
                MAX_LAYOUT_PREFERENCES_BYTES
            )));
        }
        Ok(bytes)
    }

    /// Return the saved state for a profile/tab, if one exists.
    pub fn tab_state(
        &self,
        profile: Option<&str>,
        tab: TabId,
    ) -> Result<Option<&PersistedTabLayout>, LayoutError> {
        let profile = layout_profile_key(profile)?;
        Ok(self
            .profiles
            .get(&profile)
            .and_then(|entry| entry.tabs.get(&tab)))
    }

    /// Build a layout model from the saved state, falling back to the named
    /// preset when this profile/tab has never been customized.
    pub fn model_for(&self, profile: Option<&str>, tab: TabId) -> Result<LayoutModel, LayoutError> {
        self.validate()?;
        let mut model = LayoutModel::new(tab);
        if let Some(state) = self.tab_state(profile, tab)? {
            state.apply_to_model(&mut model);
        }
        Ok(model)
    }

    /// Store one model's user-owned state.  The model's host capabilities are
    /// not persisted; this keeps a layout portable across machines.
    pub fn set_model(
        &mut self,
        profile: Option<&str>,
        model: &LayoutModel,
    ) -> Result<bool, LayoutError> {
        let profile = layout_profile_key(profile)?;
        let state = PersistedTabLayout::from_model(model);
        validate_tab_state(model.tab, &state)?;
        self.validate()?;
        let mut next = self.clone();
        let entry = next.profiles.entry(profile).or_default();
        if entry.tabs.get(&model.tab) == Some(&state) {
            return Ok(false);
        }
        entry.tabs.insert(model.tab, state);
        next.validate()?;
        *self = next;
        Ok(true)
    }

    /// Remove one saved tab state.  Reset is idempotent and does not retain an
    /// empty profile entry on disk.
    pub fn reset_tab(&mut self, profile: Option<&str>, tab: TabId) -> Result<bool, LayoutError> {
        let profile = layout_profile_key(profile)?;
        let Some(entry) = self.profiles.get_mut(&profile) else {
            return Ok(false);
        };
        let changed = entry.tabs.remove(&tab).is_some();
        if entry.tabs.is_empty() {
            self.profiles.remove(&profile);
        }
        Ok(changed)
    }

    /// Remove every saved tab state for one profile.
    pub fn reset_profile(&mut self, profile: Option<&str>) -> Result<bool, LayoutError> {
        let profile = layout_profile_key(profile)?;
        Ok(self.profiles.remove(&profile).is_some())
    }

    /// Migrate the only pre-v1 shape we support: a serialized `LayoutModel`
    /// without a schema/profile wrapper.  It becomes the `default` profile.
    /// Unknown or partial legacy data is rejected rather than guessed.
    fn migrate_value(value: serde_json::Value) -> Result<Self, LayoutError> {
        let schema = value
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
            .map(|version| u16::try_from(version).unwrap_or(u16::MAX));
        match schema {
            Some(version) if version != LAYOUT_SCHEMA_VERSION => Err(LayoutError::SchemaVersion {
                found: version,
                expected: LAYOUT_SCHEMA_VERSION,
            }),
            Some(_) => {
                serde_json::from_value(value).map_err(|error| LayoutError::Json(error.to_string()))
            }
            None => {
                let legacy: LayoutModel = serde_json::from_value(value)
                    .map_err(|error| LayoutError::Json(error.to_string()))?;
                let mut preferences = Self::default();
                preferences.set_model(None, &legacy)?;
                Ok(preferences)
            }
        }
    }
}

fn validate_tab_state(tab: TabId, state: &PersistedTabLayout) -> Result<(), LayoutError> {
    let ratios = state.ratios;
    if ratios.left < ColumnRatios::MIN
        || ratios.center < ColumnRatios::MIN
        || ratios.right < ColumnRatios::MIN
        || ratios.left > ColumnRatios::MAX
        || ratios.center > ColumnRatios::MAX
        || ratios.right > ColumnRatios::MAX
        || ratios
            .left
            .saturating_add(ratios.center)
            .saturating_add(ratios.right)
            != ColumnRatios::TOTAL
    {
        return Err(LayoutError::InvalidRatios { tab });
    }
    if state.collapsed.len() > MAX_LAYOUT_COLLAPSED_PANES {
        return Err(LayoutError::TooManyCollapsed {
            tab,
            max: MAX_LAYOUT_COLLAPSED_PANES,
        });
    }
    let valid = LayoutPreset::for_tab(tab)
        .panes
        .into_iter()
        .map(|pane| pane.id)
        .collect::<BTreeSet<_>>();
    for (pane, weight) in &state.row_weights {
        if !valid.contains(pane) {
            return Err(LayoutError::UnknownPane { tab, pane: *pane });
        }
        if !(1..=1000).contains(weight) {
            return Err(LayoutError::InvalidRatios { tab });
        }
    }
    for pane in state.collapsed.iter().copied() {
        if !valid.contains(&pane) {
            return Err(LayoutError::UnknownPane { tab, pane });
        }
    }
    if let Some(pane) = state.focused
        && !valid.contains(&pane)
    {
        return Err(LayoutError::UnknownPane { tab, pane });
    }
    Ok(())
}

fn layout_profile_key(profile: Option<&str>) -> Result<String, LayoutError> {
    let profile = profile.unwrap_or("default");
    validate_layout_profile_name(profile)?;
    Ok(profile.to_owned())
}

fn validate_layout_profile_name(profile: &str) -> Result<(), LayoutError> {
    if profile.is_empty()
        || profile.len() > 128
        || profile
            .chars()
            .any(|character| !(character.is_ascii_alphanumeric() || matches!(character, '_' | '-')))
    {
        return Err(LayoutError::InvalidProfile(profile.to_owned()));
    }
    Ok(())
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

/// Direction used by keyboard navigation and split adjustment.
///
/// This lives in the terminal-independent layout module so a future headless
/// or GUI host can apply the same focus policy without importing crossterm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FocusDirection {
    Left,
    Right,
    Up,
    Down,
}

impl FocusDirection {
    pub const fn is_horizontal(self) -> bool {
        matches!(self, Self::Left | Self::Right)
    }

    pub const fn is_forward(self) -> bool {
        matches!(self, Self::Right | Self::Down)
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

    /// The target sum used by interactive split editing.  Presets are
    /// expressed as percentages (30/45/25), while layout computation still
    /// accepts arbitrary positive weights for compatibility with callers that
    /// build their own presets.
    pub const TOTAL: u16 = 100;
    /// Keep every column useful while a user repeatedly nudges a split.
    pub const MIN: u16 = 5;
    /// Keep one column from swallowing the entire workspace.
    pub const MAX: u16 = 90;
    /// One keyboard step in percentage points.
    pub const STEP: i16 = 5;

    pub const fn get(self, column: Column) -> u16 {
        match column {
            Column::Left => self.left,
            Column::Center => self.center,
            Column::Right => self.right,
        }
    }

    pub fn set(&mut self, column: Column, value: u16) {
        match column {
            Column::Left => self.left = value,
            Column::Center => self.center = value,
            Column::Right => self.right = value,
        }
    }

    /// Return a deterministic percentage representation suitable for editing.
    ///
    /// `set_ratios` intentionally remains a low-level escape hatch and accepts
    /// arbitrary weights.  Interactive controls call this method first so a
    /// malformed/legacy value cannot produce a zero or overflowing split.
    pub fn bounded(self) -> Self {
        let weights = vec![self.left.max(1), self.center.max(1), self.right.max(1)];
        let mut values = weighted_partition(u32::from(Self::TOTAL), weights)
            .into_iter()
            .map(|value| value.clamp(Self::MIN, Self::MAX))
            .collect::<Vec<_>>();

        // Clamping can change the sum.  Repair it with deterministic one-cell
        // transfers while respecting both floors; 3 * MIN <= TOTAL <= 2 * MAX
        // makes a solution possible for the constants above.
        let mut sum = values.iter().map(|value| u32::from(*value)).sum::<u32>();
        while sum < u32::from(Self::TOTAL) {
            if let Some(index) = values.iter().position(|value| *value < Self::MAX) {
                values[index] = values[index].saturating_add(1);
                sum += 1;
            } else {
                break;
            }
        }
        while sum > u32::from(Self::TOTAL) {
            if let Some(index) = values.iter().position(|value| *value > Self::MIN) {
                values[index] = values[index].saturating_sub(1);
                sum = sum.saturating_sub(1);
            } else {
                break;
            }
        }
        Self::new(values[0], values[1], values[2])
    }

    /// Nudge one column while preserving the 100-point total and bounded
    /// floors/ceilings.  Positive deltas grow the selected column; negative
    /// deltas shrink it.  The nearest neighbouring column supplies/receives
    /// the transferred weight before the remaining column is used.
    pub fn adjust_column(self, column: Column, delta: i16) -> Self {
        if delta == 0 {
            return self;
        }
        let mut values = self.bounded();
        let old = values.get(column);
        let desired = (i32::from(old) + i32::from(delta))
            .clamp(i32::from(Self::MIN), i32::from(Self::MAX)) as u16;
        let requested = i32::from(desired) - i32::from(old);
        if requested == 0 {
            return values;
        }

        let donors = match column {
            Column::Left => [Column::Center, Column::Right],
            Column::Center => [Column::Left, Column::Right],
            Column::Right => [Column::Center, Column::Left],
        };
        if requested > 0 {
            let mut remaining = requested as u16;
            for donor in donors {
                let available = values.get(donor).saturating_sub(Self::MIN);
                let transfer = available.min(remaining);
                if transfer > 0 {
                    values.set(donor, values.get(donor).saturating_sub(transfer));
                    remaining -= transfer;
                }
                if remaining == 0 {
                    break;
                }
            }
            let transferred = requested as u16 - remaining;
            values.set(column, old.saturating_add(transferred));
        } else {
            let requested_abs = requested.unsigned_abs().min(u32::from(u16::MAX)) as u16;
            let mut remaining = requested_abs;
            for donor in donors {
                let available = Self::MAX.saturating_sub(values.get(donor));
                let transfer = available.min(remaining);
                if transfer > 0 {
                    values.set(donor, values.get(donor).saturating_add(transfer));
                    remaining -= transfer;
                }
                if remaining == 0 {
                    break;
                }
            }
            let transferred = requested_abs - remaining;
            values.set(column, old.saturating_sub(transferred));
        }
        values
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
    #[serde(default)]
    pub row_weights: BTreeMap<PaneId, u16>,
    pub collapsed: BTreeSet<PaneId>,
    pub focused: Option<PaneId>,
    pub capabilities: PaneCapabilities,
}

impl LayoutModel {
    pub fn new(tab: TabId) -> Self {
        Self {
            tab,
            ratios: LayoutPreset::for_tab(tab).ratios,
            row_weights: BTreeMap::new(),
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

    /// Return the currently selected pane, if one has been selected.
    pub const fn focused_pane(&self) -> Option<PaneId> {
        self.focused
    }

    /// Return panes that can receive focus at a given viewport.
    ///
    /// At a narrow width the geometry intentionally exposes only one pane, but
    /// all available, non-collapsed panes remain candidates so a user can use
    /// the keyboard to move through the single-pane stack.  At wider widths,
    /// breakpoint-collapsed and unavailable panes are omitted.
    pub fn focusable_panes(&self, width: u16, height: u16) -> Vec<PaneId> {
        let viewport = Viewport::new(width, height);
        let snapshot = self.compute_viewport(viewport);
        let narrow = matches!(snapshot.breakpoint, Breakpoint::Narrow);
        self.preset()
            .panes
            .iter()
            .filter_map(|spec| {
                if !self.is_available(*spec) || self.collapsed.contains(&spec.id) {
                    return None;
                }
                if narrow {
                    Some(spec.id)
                } else {
                    snapshot
                        .pane(spec.id)
                        .filter(|pane| pane.visibility == Visibility::Visible)
                        .map(|_| spec.id)
                }
            })
            .collect()
    }

    /// Cycle focus in preset order, wrapping at either end.
    pub fn focus_next(&mut self, width: u16, height: u16) -> Option<PaneId> {
        self.focus_cycle(width, height, true)
    }

    /// Cycle focus backwards in preset order, wrapping at either end.
    pub fn focus_previous(&mut self, width: u16, height: u16) -> Option<PaneId> {
        self.focus_cycle(width, height, false)
    }

    /// Move focus toward the nearest pane in a direction.  If there is no pane
    /// in that direction (or the viewport is narrow and has only one visible
    /// rectangle), fall back to a deterministic forward/backward cycle rather
    /// than trapping keyboard focus.
    pub fn focus_direction(
        &mut self,
        direction: FocusDirection,
        width: u16,
        height: u16,
    ) -> Option<PaneId> {
        let candidates = self.focusable_panes(width, height);
        if candidates.is_empty() {
            return None;
        }
        let snapshot = self.compute(width, height);
        let current = self.focused.filter(|pane| candidates.contains(pane));
        let Some(current) = current else {
            self.focused = candidates.first().copied();
            return self.focused;
        };
        let current_rect = snapshot.pane(current).map(|pane| pane.rect);
        let mut nearest: Option<((u8, u32, u32), PaneId)> = None;
        if let Some(current_rect) = current_rect.filter(|rect| !rect.is_empty()) {
            let current_x = i32::from(current_rect.x) + i32::from(current_rect.width) / 2;
            let current_y = i32::from(current_rect.y) + i32::from(current_rect.height) / 2;
            for candidate in &candidates {
                if *candidate == current {
                    continue;
                }
                let Some(rect) = snapshot.pane(*candidate).map(|pane| pane.rect) else {
                    continue;
                };
                if rect.is_empty() {
                    continue;
                }
                let candidate_x = i32::from(rect.x) + i32::from(rect.width) / 2;
                let candidate_y = i32::from(rect.y) + i32::from(rect.height) / 2;
                let (primary, secondary) = match direction {
                    FocusDirection::Left if candidate_x < current_x => (
                        current_x - candidate_x,
                        (candidate_y - current_y).unsigned_abs(),
                    ),
                    FocusDirection::Right if candidate_x > current_x => (
                        candidate_x - current_x,
                        (candidate_y - current_y).unsigned_abs(),
                    ),
                    FocusDirection::Up if candidate_y < current_y => (
                        current_y - candidate_y,
                        (candidate_x - current_x).unsigned_abs(),
                    ),
                    FocusDirection::Down if candidate_y > current_y => (
                        candidate_y - current_y,
                        (candidate_x - current_x).unsigned_abs(),
                    ),
                    _ => continue,
                };
                // Prefer a pane that overlaps the current pane on the
                // perpendicular axis (for example, Down should stay in the
                // same column).  Primary distance then dominates, while the
                // secondary distance keeps ties deterministic.
                let axis_overlap = if direction.is_horizontal() {
                    current_rect.y < rect.bottom() && rect.y < current_rect.bottom()
                } else {
                    current_rect.x < rect.right() && rect.x < current_rect.right()
                };
                let score = (
                    u8::from(!axis_overlap),
                    u32::try_from(primary).unwrap_or(u32::MAX),
                    secondary,
                );
                if nearest.is_none_or(|(best, _)| score < best) {
                    nearest = Some((score, *candidate));
                }
            }
        }
        if let Some((_, pane)) = nearest {
            self.focused = Some(pane);
            return self.focused;
        }
        self.focus_cycle(width, height, direction.is_forward())
    }

    fn focus_cycle(&mut self, width: u16, height: u16, forward: bool) -> Option<PaneId> {
        let candidates = self.focusable_panes(width, height);
        if candidates.is_empty() {
            return None;
        }
        let next = match self
            .focused
            .and_then(|pane| candidates.iter().position(|id| *id == pane))
        {
            Some(index) if forward => (index + 1) % candidates.len(),
            Some(index) => (index + candidates.len() - 1) % candidates.len(),
            None if forward => 0,
            None => candidates.len() - 1,
        };
        self.focused = candidates.get(next).copied();
        self.focused
    }

    /// Adjust one column by a signed percentage-point delta while preserving
    /// a bounded, 100-point ratio total.
    pub fn adjust_ratio(&mut self, column: Column, delta: i16) -> bool {
        let adjusted = self.ratios.adjust_column(column, delta);
        if adjusted == self.ratios {
            false
        } else {
            self.ratios = adjusted;
            true
        }
    }

    /// Adjust the split associated with the focused pane.  Horizontal arrows
    /// nudge its column; vertical arrows are navigation-only and therefore do
    /// not mutate ratios.
    pub fn adjust_focused_split(
        &mut self,
        direction: FocusDirection,
        width: u16,
        height: u16,
    ) -> bool {
        if !direction.is_horizontal() {
            return false;
        }
        let pane = self
            .focused
            .filter(|pane| self.focusable_panes(width, height).contains(pane));
        let pane = pane.or_else(|| self.focusable_panes(width, height).first().copied());
        let Some(pane) = pane else {
            return false;
        };
        let Some(spec) = self.preset().panes.into_iter().find(|spec| spec.id == pane) else {
            return false;
        };
        self.focused = Some(pane);
        let delta = if direction == FocusDirection::Right {
            ColumnRatios::STEP
        } else {
            -ColumnRatios::STEP
        };
        self.adjust_ratio(spec.column, delta)
    }

    /// Restore the active tab's preset ratios and clear user pane state.
    pub fn reset_layout(&mut self) {
        self.ratios = self.preset().ratios;
        self.row_weights.clear();
        self.collapsed.clear();
        self.focused = None;
    }

    /// Short alias useful to hosts implementing a reset key or command.
    pub fn reset(&mut self) {
        self.reset_layout();
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
                .map(|state| {
                    (
                        self.row_weights
                            .get(&state.spec.id)
                            .copied()
                            .unwrap_or(state.spec.row_weight)
                            .clamp(1, 1000),
                        state.spec.min_size.height,
                    )
                })
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
