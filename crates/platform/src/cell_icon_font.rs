use std::env;
use std::io;

use crate::AppPaths;

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix;
#[cfg(any(windows, target_os = "linux"))]
mod windows;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellIconFontInstall {
    Installed,
    Available,
    Conflict,
    Remote,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellIconFontRemoval {
    Removed,
    Missing,
    Preserved,
}

/// Synchronously installs the font for the local terminal's operating system.
/// Call from a background task when a terminal session is active.
pub fn install_cell_icon_font(paths: &AppPaths, font: &[u8]) -> io::Result<CellIconFontInstall> {
    if ["SSH_CONNECTION", "SSH_CLIENT", "SSH_TTY"]
        .into_iter()
        .any(nonempty_env)
    {
        return Ok(CellIconFontInstall::Remote);
    }
    if !has_native_paths(paths) {
        return Ok(CellIconFontInstall::Unsupported);
    }
    if font.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "cell icon font is empty",
        ));
    }

    #[cfg(target_os = "linux")]
    if is_wsl() {
        match wsl_terminal() {
            WslTerminal::Windows => {
                return match windows::install(font) {
                    Err(error) if windows_interop_unavailable(&error) => {
                        Ok(CellIconFontInstall::Unsupported)
                    }
                    result => result,
                };
            }
            WslTerminal::Linux => {}
            WslTerminal::Unknown => return Ok(CellIconFontInstall::Unsupported),
        }
    }

    #[cfg(windows)]
    return windows::install(font);
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    return unix::install(paths, font);
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    Ok(CellIconFontInstall::Unsupported)
}

/// Removes an owned installation. Alternate application roots do not grant
/// access to the real account's fonts.
pub fn uninstall_cell_icon_font(paths: &AppPaths) -> io::Result<CellIconFontRemoval> {
    if !has_native_paths(paths) {
        return Ok(CellIconFontRemoval::Missing);
    }

    #[cfg(windows)]
    return windows::uninstall();
    #[cfg(target_os = "linux")]
    if is_wsl() {
        use CellIconFontRemoval::*;
        let windows = match windows::uninstall() {
            Err(error) if windows_interop_unavailable(&error) => Missing,
            result => result?,
        };
        return Ok(match (unix::uninstall(paths)?, windows) {
            (Preserved, _) | (_, Preserved) => Preserved,
            (Removed, _) | (_, Removed) => Removed,
            (Missing, Missing) => Missing,
        });
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    return unix::uninstall(paths);
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    Ok(CellIconFontRemoval::Missing)
}

fn has_native_paths(paths: &AppPaths) -> bool {
    AppPaths::from_environment().is_ok_and(|native| native == *paths)
}

fn nonempty_env(name: &str) -> bool {
    env::var_os(name).is_some_and(|value| !value.is_empty())
}

#[cfg(target_os = "linux")]
fn is_wsl() -> bool {
    nonempty_env("WSL_DISTRO_NAME")
        || std::fs::read_to_string("/proc/sys/kernel/osrelease")
            .is_ok_and(|release| release.to_ascii_lowercase().contains("microsoft"))
}

#[cfg(target_os = "linux")]
fn windows_interop_unavailable(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::Unsupported
    ) || error.raw_os_error() == Some(libc::ENOEXEC)
}

#[cfg(target_os = "linux")]
#[derive(Debug, PartialEq, Eq)]
enum WslTerminal {
    Windows,
    Linux,
    Unknown,
}

#[cfg(target_os = "linux")]
fn wsl_terminal() -> WslTerminal {
    let term_program = env::var("TERM_PROGRAM").unwrap_or_default();
    let imported_term_program = env::var("WSLENV").is_ok_and(|vars| {
        vars.split(':')
            .any(|var| var.split('/').next() == Some("TERM_PROGRAM"))
    });
    classify_wsl_terminal(
        &term_program.to_ascii_lowercase(),
        has_linux_terminal_parent(),
        nonempty_env("WT_SESSION"),
        imported_term_program,
    )
}

#[cfg(target_os = "linux")]
fn classify_wsl_terminal(
    term_program: &str,
    linux_parent: bool,
    wt_session: bool,
    imported_term_program: bool,
) -> WslTerminal {
    if linux_parent {
        WslTerminal::Linux
    } else if (wt_session && matches!(term_program, "" | "windows_terminal" | "windows terminal"))
        || (imported_term_program && term_program == "wezterm")
    {
        WslTerminal::Windows
    } else {
        WslTerminal::Unknown
    }
}

#[cfg(target_os = "linux")]
fn has_linux_terminal_parent() -> bool {
    let mut pid = std::process::id();
    while pid > 1 {
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
            break;
        };
        let Some((name, state_and_parent)) = stat
            .split_once('(')
            .and_then(|(_, rest)| rest.rsplit_once(')'))
        else {
            break;
        };
        if matches!(
            name,
            "alacritty"
                | "foot"
                | "footclient"
                | "ghostty"
                | "gnome-terminal-"
                | "kitty"
                | "konsole"
                | "lxterminal"
                | "mate-terminal"
                | "tilix"
                | "wezterm-gui"
                | "xfce4-terminal"
                | "xterm"
        ) {
            return true;
        }
        let Some(parent) = state_and_parent
            .split_whitespace()
            .nth(1)
            .and_then(|value| value.parse::<u32>().ok())
            .filter(|parent| *parent != pid)
        else {
            break;
        };
        pid = parent;
    }
    false
}

#[cfg(all(test, target_os = "linux"))]
#[test]
fn wsl_font_targets_the_terminal_instead_of_the_distribution() {
    assert_eq!(
        classify_wsl_terminal("", false, true, false),
        WslTerminal::Windows
    );
    assert_eq!(
        classify_wsl_terminal("wezterm", false, false, true),
        WslTerminal::Windows
    );
    assert_eq!(
        classify_wsl_terminal("", true, true, false),
        WslTerminal::Linux
    );
    assert_eq!(
        classify_wsl_terminal("", true, false, false),
        WslTerminal::Linux
    );
    assert_eq!(
        classify_wsl_terminal("vscode", false, true, false),
        WslTerminal::Unknown
    );
    assert_eq!(
        classify_wsl_terminal("", false, false, false),
        WslTerminal::Unknown
    );
}
