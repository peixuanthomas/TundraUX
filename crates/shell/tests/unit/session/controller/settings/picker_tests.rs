use super::*;

fn picker(kind: ui::SettingsPickerKind, query: &str) -> SettingsPickerState {
    SettingsPickerState {
        editing: Default::default(),
        options: vec![],
        kind,
        query: query.into(),
        selected_index: 0,
        window_start: 0,
        image_icons_supported: false,
    }
}

#[test]
fn filtered_settings_keep_the_source_index_for_numeric_and_disabled_options() {
    let all = settings_picker_options(&picker(ui::SettingsPickerKind::ScreenTimeout, ""), &[]);
    let target = all.last().unwrap();
    let filtered = settings_picker_options(
        &picker(ui::SettingsPickerKind::ScreenTimeout, &target.label),
        &[],
    );
    assert_eq!(
        filtered[0].source_index,
        Some(SCREEN_TIMEOUT_OPTIONS.len() - 1)
    );
    let all = settings_picker_options(&picker(ui::SettingsPickerKind::DefaultThemeIcons, ""), &[]);
    let filtered = settings_picker_options(
        &picker(ui::SettingsPickerKind::DefaultThemeIcons, &all[1].label),
        &[],
    );
    assert_eq!(filtered[0].source_index, Some(1));
    assert!(!filtered[0].enabled);
    assert!(
        settings_picker_options(
            &picker(ui::SettingsPickerKind::ScreenTimeout, "no-match-xyz"),
            &[]
        )
        .is_empty()
    );
}
