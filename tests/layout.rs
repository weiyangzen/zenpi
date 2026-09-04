use zenpi::layout::{
    Breakpoint, LayoutModel, LayoutPreset, PaneCapabilities, PaneId, TabId, Visibility,
};

#[test]
fn every_workspace_tab_has_a_named_preset_and_required_panes() {
    for tab in TabId::ALL {
        let preset = LayoutPreset::for_tab(tab);
        assert_eq!(preset.tab, tab);
        assert!(!preset.panes.is_empty());
        assert!(
            preset.panes.iter().any(|pane| pane.id == PaneId::Resources)
                || matches!(tab, TabId::Learn | TabId::Review | TabId::Session)
        );
        assert!(
            preset.panes.iter().any(|pane| pane.id == PaneId::Gantt)
                || matches!(tab, TabId::Learn | TabId::Review)
        );
        assert!(preset.panes.iter().any(|pane| pane.id == PaneId::Browser));
        assert!(preset.panes.iter().any(|pane| pane.id == PaneId::Terminal));
        assert!(
            preset
                .panes
                .iter()
                .all(|pane| pane.min_size.width > 0 && pane.min_size.height > 0)
        );
    }
}

#[test]
fn breakpoint_bands_include_zero_and_narrow_resize_states() {
    assert_eq!(Breakpoint::for_size(0, 0), Breakpoint::Zero);
    assert_eq!(Breakpoint::for_size(79, 24), Breakpoint::Narrow);
    assert_eq!(Breakpoint::for_size(80, 24), Breakpoint::Compact);
    assert_eq!(Breakpoint::for_size(99, 24), Breakpoint::Compact);
    assert_eq!(Breakpoint::for_size(100, 24), Breakpoint::Standard);
    assert_eq!(Breakpoint::for_size(159, 24), Breakpoint::Standard);
    assert_eq!(Breakpoint::for_size(160, 24), Breakpoint::Wide);
}

#[test]
fn wide_project_layout_respects_ratios_minimums_and_has_no_overlap() {
    let model = LayoutModel::new(TabId::Project).with_capabilities(PaneCapabilities {
        browser: true,
        terminal: true,
    });
    let snapshot = model.compute(200, 40);
    assert_eq!(snapshot.breakpoint, Breakpoint::Wide);
    assert!(snapshot.visible_rects_non_overlapping());

    let left = snapshot.pane(PaneId::ProjectConversation).unwrap();
    let center = snapshot.pane(PaneId::Gantt).unwrap();
    let right = snapshot.pane(PaneId::Browser).unwrap();
    assert_eq!(left.rect.x, 0);
    assert!(center.rect.x > left.rect.x);
    assert!(right.rect.x > center.rect.x);
    for pane in snapshot.visible_panes() {
        assert!(pane.rect.width >= pane.min_size.width);
        assert!(pane.rect.height >= pane.min_size.height);
        assert!(pane.rect.right() <= snapshot.viewport.width);
        assert!(pane.rect.bottom() <= snapshot.viewport.height);
    }
}

#[test]
fn optional_right_panes_are_unavailable_by_default_and_collapse_on_compact() {
    let default_snapshot = LayoutModel::new(TabId::Project).compute(200, 40);
    assert_eq!(
        default_snapshot.pane(PaneId::Browser).unwrap().visibility,
        Visibility::Unavailable
    );
    assert_eq!(
        default_snapshot.pane(PaneId::Terminal).unwrap().visibility,
        Visibility::Unavailable
    );

    let enabled = LayoutModel::new(TabId::Project).with_capabilities(PaneCapabilities {
        browser: true,
        terminal: true,
    });
    let compact = enabled.compute(90, 30);
    assert_eq!(compact.breakpoint, Breakpoint::Compact);
    assert_eq!(
        compact.pane(PaneId::Browser).unwrap().visibility,
        Visibility::Collapsed
    );
    assert_eq!(
        compact.pane(PaneId::Terminal).unwrap().visibility,
        Visibility::Collapsed
    );
    assert!(compact.visible_rects_non_overlapping());
}

#[test]
fn narrow_layout_keeps_one_focusable_pane_and_zero_viewport_is_hidden() {
    let mut model = LayoutModel::new(TabId::Goal);
    model.set_focused(Some(PaneId::EventTimeline));
    let narrow = model.compute(60, 20);
    assert_eq!(narrow.breakpoint, Breakpoint::Narrow);
    assert_eq!(
        narrow
            .visible_panes()
            .map(|pane| pane.id)
            .collect::<Vec<_>>(),
        vec![PaneId::EventTimeline]
    );
    assert!(narrow.visible_rects_non_overlapping());

    let zero = model.compute(0, 0);
    assert_eq!(zero.breakpoint, Breakpoint::Zero);
    assert!(
        zero.panes
            .iter()
            .filter(|pane| pane.visibility != Visibility::Unavailable)
            .all(|pane| pane.visibility == Visibility::Hidden && pane.rect.is_empty())
    );
}

#[test]
fn custom_ratios_collapse_optional_panes_when_minimum_width_cannot_fit() {
    let mut model = LayoutModel::new(TabId::Project).with_capabilities(PaneCapabilities {
        browser: true,
        terminal: true,
    });
    model.set_ratios(zenpi::layout::ColumnRatios::new(1, 98, 1));
    let snapshot = model.compute(120, 30);
    assert_ne!(
        snapshot.pane(PaneId::Browser).unwrap().visibility,
        Visibility::Visible
    );
    assert_ne!(
        snapshot.pane(PaneId::Terminal).unwrap().visibility,
        Visibility::Visible
    );
    assert!(snapshot.visible_rects_non_overlapping());
}

#[test]
fn fully_collapsed_state_is_safe_at_a_normal_viewport() {
    let mut model = LayoutModel::new(TabId::Project);
    for pane in [
        PaneId::ProjectConversation,
        PaneId::Resources,
        PaneId::GoalConversation,
        PaneId::Gantt,
    ] {
        model.set_collapsed(pane, true);
    }
    let snapshot = model.compute(120, 30);
    assert!(snapshot.visible_panes().next().is_none());
    assert!(snapshot.visible_rects_non_overlapping());
}
