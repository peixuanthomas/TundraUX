use super::ManagementError;
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
use std::path::Path;
use std::sync::atomic::AtomicBool;
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
use std::sync::atomic::Ordering;

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub fn commands(_target: &str, _cancelled: &AtomicBool) -> Result<Vec<String>, ManagementError> {
    Ok(Vec::new())
}

/// Look up launch commands attributed to this installed package by the native manager.
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
pub fn commands(target: &str, cancelled: &AtomicBool) -> Result<Vec<String>, ManagementError> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    use super::packages as backend;
    #[cfg(windows)]
    use super::winget as backend;
    let paths = backend::launch_paths(target, cancelled)?;
    let mut commands = Vec::new();
    for path in paths {
        if cancelled.load(Ordering::Acquire) {
            return Err(ManagementError::Cancelled);
        }
        if let Some(command) = command_for_path(&path) {
            commands.push(command);
        }
    }
    commands.sort();
    commands.dedup();
    Ok(commands)
}

#[cfg(any(target_os = "linux", target_os = "macos", windows))]
pub(super) fn command_for_path(path: &Path) -> Option<String> {
    let text = path.to_str()?;
    if !path.is_absolute() || text.chars().any(char::is_control) {
        return None;
    }
    let metadata = path.metadata().ok()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let quoted = shell_words::quote(text);
        #[cfg(target_os = "macos")]
        if metadata.is_dir()
            && path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("app"))
        {
            return Some(format!("/usr/bin/open {quoted}"));
        }
        #[cfg(target_os = "linux")]
        if path.extension().is_some_and(|ext| ext == "desktop")
            && metadata.is_file()
            && Path::new("/usr/bin/gio").is_file()
        {
            return Some(format!("/usr/bin/gio launch {quoted}"));
        }
        (metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
            .then(|| quoted.into_owned())
    }
    #[cfg(windows)]
    {
        (metadata.is_file()
            && path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
            && !text.contains(['%', '!', '"']))
        .then(|| format!("\"{text}\""))
    }
}
