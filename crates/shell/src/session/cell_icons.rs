use super::*;
use platform::{AppPaths, CellIconFontInstall, ManagedCommand};
use sha2::{Digest, Sha256};
use std::path::Path;
use workspace_protocol::CellIconMode;

const CHECK_KEY: &str = "shell.cell-icon-check";

pub(super) fn set_render_mode(mode: CellIconMode) {
    ui::set_cell_icon_mode(match mode {
        CellIconMode::Ascii => ui::CellIconMode::Ascii,
        CellIconMode::Font => ui::CellIconMode::Font,
    });
}

#[derive(Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum Preference {
    Selected {
        mode: CellIconMode,
        revision: String,
        availability: FontAvailability,
    },
    Suppressed,
}

impl Preference {
    fn from_choice(choice: CellIconChoice, availability: FontAvailability) -> Self {
        if choice == CellIconChoice::Suppress {
            return Self::Suppressed;
        }
        Self::Selected {
            mode: choice_mode(choice),
            revision: font_revision(),
            availability,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum FontAvailability {
    Present,
    Conflict,
    Remote,
    Unsupported,
    Failed,
}

struct Check {
    preference_path: Option<PathBuf>,
    preference: Option<Preference>,
    install: io::Result<CellIconFontInstall>,
    manual: bool,
}

impl Check {
    fn existing_mode(&self) -> Option<CellIconMode> {
        if self.manual {
            return None;
        }
        match self.preference.as_ref()? {
            Preference::Suppressed => Some(CellIconMode::Ascii),
            Preference::Selected {
                mode,
                revision,
                availability,
            } if !matches!(self.install, Ok(CellIconFontInstall::Installed))
                && *availability == self.availability()
                && *revision == font_revision() =>
            {
                Some(*mode)
            }
            _ => None,
        }
    }

    fn availability(&self) -> FontAvailability {
        use CellIconFontInstall::*;
        match self.install {
            Ok(Installed | Available) => FontAvailability::Present,
            Ok(Conflict) => FontAvailability::Conflict,
            Ok(Remote) => FontAvailability::Remote,
            Ok(Unsupported) => FontAvailability::Unsupported,
            Err(_) => FontAvailability::Failed,
        }
    }
}

fn choice_mode(choice: CellIconChoice) -> CellIconMode {
    match choice {
        CellIconChoice::Font => CellIconMode::Font,
        CellIconChoice::Ascii | CellIconChoice::Suppress => CellIconMode::Ascii,
    }
}

enum WorkResult {
    Checked(Check),
    Saved(CellIconChoice, io::Result<()>),
}

pub(super) struct TerminalCellIcons {
    group: ManagedTaskGroup,
    job: Option<(ManagedThreadHandle<()>, mpsc::Receiver<WorkResult>)>,
    mode: CellIconMode,
    started: bool,
    preference_target: Option<(PathBuf, FontAvailability)>,
    dialog: Option<u64>,
}

impl TerminalCellIcons {
    pub(super) fn new(watchdog: &AppWatchdog) -> Self {
        set_render_mode(CellIconMode::Ascii);
        Self {
            group: watchdog.task_group("terminal-cell-icons"),
            job: None,
            mode: CellIconMode::Ascii,
            started: false,
            preference_target: None,
            dialog: None,
        }
    }

    pub(super) fn mode(&self) -> CellIconMode {
        self.mode
    }

    pub(super) fn poll(&mut self, state: &mut ShellSession, platform: &Arc<dyn Platform>) -> bool {
        if !ready_for_check(state) {
            return false;
        }
        if let Some((_, receiver)) = &self.job {
            let result = match receiver.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Empty) => return false,
                Err(mpsc::TryRecvError::Disconnected) => None,
            };
            self.join();
            match result {
                Some(WorkResult::Checked(check)) => {
                    self.preference_target = check
                        .preference_path
                        .clone()
                        .map(|path| (path, check.availability()));
                    if let Some(mode) = check.existing_mode() {
                        self.mode = mode;
                    } else {
                        if !check.manual
                            || matches!(check.install, Ok(CellIconFontInstall::Installed))
                        {
                            self.mode = CellIconMode::Ascii;
                        }
                        self.dialog = Some(show_check_dialog(state, &check));
                    }
                }
                Some(WorkResult::Saved(choice, result)) => {
                    if let Err(error) = result {
                        state.notify_alert(i18n::msg!(
                            "cell-icons-save-failed",
                            error = error.to_string()
                        ));
                    }
                    self.mode = choice_mode(choice);
                }
                None => state.notify_alert(i18n::msg!("cell-icons-check-failed")),
            }
            return true;
        }
        if let Some(choice) = state.cell_icon_choice.take() {
            self.dialog = None;
            if let Some((path, availability)) = self.preference_target.clone() {
                if let Err(error) = self.submit(move || {
                    WorkResult::Saved(choice, save_preference(&path, choice, availability))
                }) {
                    state.notify_alert(i18n::msg!(
                        "cell-icons-save-failed",
                        error = error.to_string()
                    ));
                    self.mode = choice_mode(choice);
                    return true;
                }
            } else {
                self.mode = choice_mode(choice);
                return true;
            }
            return false;
        }
        let manual = std::mem::take(&mut state.cell_icon_recheck_requested);
        if (!self.started || manual)
            && self
                .dialog
                .is_none_or(|id| !state.app.notification_center().contains_modal_id(id))
        {
            self.started = true;
            let platform = platform.clone();
            if let Err(error) =
                self.submit(move || WorkResult::Checked(check_terminal(platform.as_ref(), manual)))
            {
                state.notify_alert(i18n::msg!(
                    "cell-icons-install-failed",
                    error = error.to_string()
                ));
                return true;
            }
        }
        false
    }

    fn submit(&mut self, job: impl FnOnce() -> WorkResult + Send + 'static) -> io::Result<()> {
        self.group.reap_finished();
        let (sender, receiver) = platform::task_events::channel();
        let mut job = Some(job);
        let handle = self
            .group
            .spawn_thread(
                TaskSpec::one_shot(TaskId::from_static("check-and-persist")),
                move || {
                    let result = job.take().expect("one-shot cell icon job")();
                    let _ = sender.send(result);
                },
            )
            .map_err(io::Error::other)?;
        self.job = Some((handle, receiver));
        Ok(())
    }

    pub(super) fn join(&mut self) {
        if let Some((handle, _)) = self.job.take() {
            let _ = handle.join();
        }
    }
}

fn ready_for_check(state: &ShellSession) -> bool {
    !state.window_owner.is_app()
        && !state.shutdown_requested()
        && state.content_screen() != ShellScreen::FirstRunSetup
        && state.pending_personalization_session.is_none()
        && state.storage_manager.is_some()
        && state
            .app
            .auth_session()
            .is_some_and(|auth| auth.role != UserRole::Guest)
}

fn font_revision() -> String {
    format!("{:x}", Sha256::digest(ascii_assets::CELL_ICON_FONT))
}

fn preference_path(paths: &AppPaths, profile: &str) -> PathBuf {
    let key = Sha256::digest(profile.as_bytes());
    paths
        .data_path()
        .join("cell-icon-profiles")
        .join(format!("{key:x}.json"))
}

fn load_preference(path: &Path) -> Option<Preference> {
    let document = platform::read_document_bytes_limited(path, 256).ok()?;
    serde_json::from_slice(&document.bytes).ok()
}

fn save_preference(
    path: &Path,
    choice: CellIconChoice,
    availability: FontAvailability,
) -> io::Result<()> {
    let bytes = serde_json::to_vec(&Preference::from_choice(choice, availability))?;
    platform::atomic_write_document(path, &bytes).map_err(io::Error::other)?;
    Ok(())
}

fn check_terminal(platform: &dyn Platform, manual: bool) -> Check {
    let env = |key: &str| std::env::var(key).ok().filter(|value| !value.is_empty());
    let profile = if platform.is_native_backend() {
        terminal_profile(env, gnome_profile)
    } else {
        None
    };
    let paths = platform.app_paths().map_err(io::Error::other);
    let preference_path = paths
        .as_ref()
        .ok()
        .zip(profile)
        .map(|(paths, profile)| preference_path(paths, &profile));
    let preference = preference_path.as_deref().and_then(load_preference);
    let install = paths.and_then(|paths| {
        if platform.is_native_backend() {
            platform::install_cell_icon_font(&paths, ascii_assets::CELL_ICON_FONT)
        } else {
            Ok(CellIconFontInstall::Unsupported)
        }
    });
    Check {
        preference_path,
        preference,
        install,
        manual,
    }
}

fn terminal_profile(
    env: impl Fn(&str) -> Option<String>,
    gnome_profile: impl FnOnce() -> Option<String>,
) -> Option<String> {
    if ["SSH_CONNECTION", "SSH_CLIENT", "SSH_TTY", "TMUX", "STY"]
        .into_iter()
        .any(|key| env(key).is_some())
    {
        return None;
    }
    let terminal = env("TERM").unwrap_or_default();
    // These frontends can inherit profile variables from the terminal that launched them.
    if env("XTERM_VERSION").is_some()
        || matches!(
            terminal.split(['-', '.']).next(),
            Some("linux" | "screen" | "tmux")
        )
    {
        return None;
    }
    let program = env("TERM_PROGRAM").unwrap_or_default().to_ascii_lowercase();
    if program == "apple_terminal" {
        return None;
    }
    if program == "iterm.app" || (program.is_empty() && env("ITERM_PROFILE").is_some()) {
        return env("ITERM_PROFILE").map(|name| format!("iterm2:{name}"));
    }
    if program == "kitty" || (program.is_empty() && env("KITTY_WINDOW_ID").is_some()) {
        let config_dir = env("KITTY_CONFIG_DIRECTORY")
            .or_else(|| env("XDG_CONFIG_HOME").map(|path| format!("{path}/kitty")))
            .unwrap_or_default();
        return Some(format!("kitty:{config_dir}"));
    }
    if program == "gnome-terminal" || (program.is_empty() && env("GNOME_TERMINAL_SCREEN").is_some())
    {
        return gnome_profile().map(|id| format!("gnome-terminal:{id}"));
    }
    if program == "wezterm" || (program.is_empty() && env("WEZTERM_EXECUTABLE").is_some()) {
        let config_file = env("WEZTERM_CONFIG_FILE").unwrap_or_default();
        let executable = env("WEZTERM_EXECUTABLE").unwrap_or_default();
        return Some(format!("wezterm\0{executable}\0{config_file}"));
    }
    if matches!(program.as_str(), "windows_terminal" | "windows terminal")
        || (program.is_empty() && (env("WT_SESSION").is_some() || env("WT_PROFILE_ID").is_some()))
    {
        let profile = env("WT_PROFILE_ID")?;
        let profile = profile.trim_matches(['{', '}']).to_ascii_lowercase();
        return (!profile.is_empty()).then(|| format!("windows-terminal:{profile}"));
    }
    None
}

fn gnome_profile() -> Option<String> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    let output = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.Terminal.ProfilesList", "list"])
        .output_managed()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    // GNOME exposes the screen object path, which changes with each tab.
    let value = std::str::from_utf8(&output.stdout).ok()?.trim();
    let id = value.strip_prefix("['")?.strip_suffix("']")?;
    (id.len() == 36
        && id
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || byte == b'-'))
    .then(|| id.into())
}

fn show_check_dialog(state: &mut ShellSession, check: &Check) -> u64 {
    use CellIconFontInstall::*;
    let guidance = match &check.install {
        Ok(Installed | Available) => i18n::msg!("cell-icons-install-local"),
        Ok(Conflict) => i18n::msg!("cell-icons-install-conflict"),
        Ok(Remote) => i18n::msg!("cell-icons-install-remote"),
        Ok(Unsupported) => i18n::msg!("cell-icons-install-unsupported"),
        Err(error) => i18n::msg!("cell-icons-install-failed", error = error.to_string()),
    };
    let guidance = if check.preference_path.is_none() {
        i18n::msg!("cell-icons-guidance-session", guidance = guidance)
    } else {
        guidance
    };
    let mut actions = vec![
        ShellNotificationAction::new("use-ascii", i18n::msg!("cell-icons-use-ascii"))
            .with_follow_up(ShellCommand::SetCellIconChoice(CellIconChoice::Ascii)),
    ];
    if check.preference_path.is_some() {
        actions.push(
            ShellNotificationAction::new("suppress", i18n::msg!("cell-icons-suppress"))
                .with_follow_up(ShellCommand::SetCellIconChoice(CellIconChoice::Suppress)),
        );
    }
    actions.extend([
        ShellNotificationAction::new("looks-right", i18n::msg!("cell-icons-looks-right"))
            .with_follow_up(ShellCommand::SetCellIconChoice(CellIconChoice::Font)),
        ShellNotificationAction::new("close", i18n::msg!("cell-icons-close")).cancel(),
    ]);
    state.notify_modal_with_options(
        ShellNotification::modal(
            i18n::msg!("cell-icons-title"),
            i18n::msg!(
                "cell-icons-sample",
                icon = ui::CellIconMode::Font.icon("fileops", "delete"),
                guidance = guidance
            ),
            ui::NotificationTone::Info,
            actions,
        )
        .with_key(CHECK_KEY)
        .for_session(),
    )
}

#[cfg(test)]
#[path = "../../tests/unit/session/cell_icons.rs"]
mod tests;
