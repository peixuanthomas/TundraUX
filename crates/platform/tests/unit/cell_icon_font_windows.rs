use super::*;

#[test]
fn windows_font_registration_and_reset_preserve_external_changes() {
    let temp = tempfile::tempdir().unwrap();
    let mut command = powershell();
    command
        .env("TEMP", temp.path())
        .env("TMP", temp.path())
        .env("TUNDRA_FONT_TEST_ROOT", temp.path());
    let result = run_with(
        command,
        include_str!("cell_icon_font_windows.ps1"),
        b"bundled test font",
    )
    .unwrap();
    assert_eq!(result, "passed");
}
