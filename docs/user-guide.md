# TundraUX user guide

Use this guide to get from first launch to everyday tasks. On pages with contextual help, press **F1** or select **Help** in the header for shortcuts.

- [Install and start](#install-and-start)
- [Navigate and arrange windows](#navigate-and-arrange-windows)
- [Browse and edit files](#browse-and-edit-files)
- [Launch programs and run commands](#launch-programs-and-run-commands)
- [Manage the system and view its status](#manage-the-system-and-view-its-status)
- [Use Clock, weather, and appearance settings](#use-clock-weather-and-appearance-settings)
- [Update or troubleshoot](#update-or-troubleshoot)

## Install and start

1. Get the archive for your operating system and architecture from the project's releases. Check the [release notes](packaging/releases/v2.0.0.md#download-and-installation) for the v2.0.0 formats and the v1 data compatibility notice.
2. Extract it to a directory you can keep. Keep `tundra-shell`, `tundra-cli`, `assets/`, and `vim/` together, as well as the installation marker when one is included.
3. Open a terminal in that directory. On Linux or macOS run `./tundra-shell`; in PowerShell on Windows run `./tundra-shell.exe`. The Linux archive also has `./install-tux.sh` to make the `tux` command available through `~/.local/bin`. See the [Linux runtime guide](packaging/linux/README-LINUX.txt) for optional desktop integration and package requirements.
4. Start with a terminal of at least **108 columns × 20 rows**. Choose language and appearance during first-time setup, or press **F9** to start with the default appearance. You can change preferences later in Settings.

Launch as your normal OS user. TundraUX uses that account for file access and saved preferences. Linux system operations that need more permission request authorization separately. To build instead of using an archive, follow [Building and running](README-TECHNICAL.md#building-and-running).

## Navigate and arrange windows

From Home, use the arrow keys and **Enter** or **Space**, or click a tile. Home shows the tools available on your platform. Press **F2** to edit the tile order. Inside a page, **Tab** and **Shift+Tab** move between regions; arrow keys usually move within the active region. Shortcuts typed into text fields are treated as text where applicable.

| What you want to do | How |
| --- | --- |
| Read help or a status message | Select **Help** or press **F1**. Click the status text beside the title to read the full message. |
| Go back or dismiss an overlay | Use the header's **Back** control for a local subpage. **Esc** dismisses the active dialog or menu first, and minimizes an ordinary app window when no overlay handles it. Vim and child terminals handle their own **Esc**. |
| Minimize, restore, or close | Use the window frame's **Minimize** or **Close** button. In the bottom window bar, click a title to focus or restore it, or click its minus button to close it normally. |
| Split the workspace | Choose **Split → Left/right** or **Top/bottom**, then pick an app or a minimized window. Cancel the chooser to undo that split. Repeat where space permits, up to four panes. |
| Make a pane full screen | Choose **Split → Full screen**. Other panes minimize and remain available in the window bar. |
| Open another instance | Use **New session** when available in the entry or window menu. Opening an existing app normally restores its current window. |

Outside the editor canvas and native terminal areas, press **Ctrl+W**, release it, then press a second key: **v** / **s** split, **arrows** or **h/j/k/l** change focus, **o** makes the pane full screen, **b** minimizes, **w** lists windows, and **q** closes normally. **Ctrl+W** followed by **Shift+X** force stops the window and can lose unsaved work. Use the frame buttons when the app itself handles Ctrl+W.

With the default minimum, left/right needs 217 × 20 cells, top/bottom needs 108 × 39, and a full grid needs 217 × 39. A child application can ask for more space. Enlarge the terminal to show panes hidden by a resize or to bring back a pending save confirmation. See [all window controls](../crates/shell/README.md#windows) and the [on-screen keyboard](../crates/shell/docs/session.md#on-screen-keyboard-for-aa-and-command-line).

## Browse and edit files

- **Open a location:** In File Explorer, use **Ctrl+L** (**Cmd+L** on macOS) to edit the path, **Alt+Up** to go to its parent, or **/** to search the list. Press **Enter** to open the selected entry.
- **Organize files:** In the file list, **N** creates a folder, **T** creates a text file, **F2** renames, **C/X/V** copies, cuts, or pastes, and **Delete** moves items to Trash where supported. Right-click or press **Shift+F10** to see the current entry's actions. In a regular directory, the empty-area menu can open Command Line in that directory.
- **Act on several entries:** Use the selection toggle, **Ctrl/Cmd+click** for individual entries, **Shift+click** for a range, or **Shift+Enter** to toggle the current row. **Ctrl/Cmd+A** selects all, **Ctrl/Cmd+I** inverts selection, and **Ctrl/Cmd+Shift+A** clears it. See [multi-selection](list-selection.md) for what stays selected when you filter, sort, or refresh.
- **Undo a move or rename:** Use **Undo Move/Rename** in Explorer or **Ctrl/Cmd+Z** while browsing. The file clipboard and undo history are shared between Explorer windows in this run. Cross-volume moves, replacements, and directory merges cannot be undone. Undo also refuses when the original location is occupied or the moved object has changed identity.
- **Save changes:** Open Editor from Launcher or open a supported text file from Explorer. Use the editor's Save action. On first save or Save As, select the folder and enter the filename in the same dialog. **Tab** moves between controls, **F10** saves, and **Esc** cancels. Use **F8** to switch between the standard editor and real Vim for the current window.

Markdown files opened in the standard editor are edited as source text. In Vim, familiar commands such as `:w` and `:wq` use the workspace's save checks. Linux also has a Script Editor with run and debug actions. See [Editor and script workspace](source-editor.md) for the editing, Vim, and Bash debugger controls.

## Launch programs and run commands

Open **Launcher** to find Editor, Command Line, Linux Script Editor, and your own entries. Search by name, path, command, or type. Press **Enter** to open the selected result. Right-click an entry for options such as **New session** or **Move**, or right-click empty space to add an entry. **F4** opens Run settings for supported entries, including arguments, working directory, and launch mode.

In **Command Line**, type a normal command such as `pwd` (Unix) or `cd` (Windows) to use the OS command interpreter. Use a leading slash for TundraUX commands, for example `/help` or `/doctor`. From an external terminal, use `tundra-cli help` or `tundra-cli doctor` without the slash. The built-in session retains its working directory and exported environment. At the end of the input, **Right** cycles completion candidates and **Tab** accepts the gray suggestion. Type `exit` to close that session normally.

See the [CLI command reference](../crates/cli/docs/commands.md) and [external UI control](../crates/cli/docs/ui-control.md). External input is disabled by default and can be enabled in **Settings → System → External CLI control**.

## Manage the system and view its status

On Linux, start from [Home's system management pages](../crates/platform/docs/linux-management.md) for services, processes, packages, network, disks, users, firewall, SSH, backups, and scheduled tasks. **Packages** on macOS uses [Homebrew](../crates/platform/docs/homebrew.md); **Packages** on Windows uses [WinGet](../crates/platform/docs/windows-packages.md). A visible entry can still depend on a native tool or service. Check its details or run `tundra-cli doctor` if it is unavailable.

For a system change, review the targets and choose the action. AutoAdmin shows approval, interactive prompts, and progress for supported operations. System authorization can still be required after you approve the task. Open **Tasks** in the header to return to pending input or inspect task output. If the outcome is unknown after a disconnect, refresh the real system state before you retry. See [AutoAdmin](../crates/auto-admin/docs/workflow.md) and [task history](../crates/platform/docs/task-history.md).

**System Status** shows metrics such as processor, memory, storage, and network state. Press **E** to edit the widget layout and use **Ctrl+S** to save it. Use **Diagnostics** to check prerequisites and **Logs** to inspect runtime events, incidents, or available system logs. See the [log guide](../crates/runtime-log/docs/logs.md) for filters, following, and export.

## Use Clock, weather, and appearance settings

- **Clock:** Select the clock in the header. **N** opens the creation dialog for an alarm or countdown; **M** manages the selected entry. Clock windows share your items. On Linux, the clock follows the OS time and timezone.
- **Weather:** The workspace shows weather after the configured period of inactivity. Settings lets you change the screen timeout and weather location. Press a key or tap to return to your open windows without signing in again.
- **Appearance and input:** Change language, theme, icon mode, or motion in Settings. ASCII icons work in terminals without image support. Use the header's keyboard button for supported text fields and embedded terminals.

## Update or troubleshoot

- **Update the portable app:** On Linux, open **Settings → Update** and choose **Release** for the stable portable archive, or **Beta** to build a selected commit from the Git tree. Windows also uses the source update screen. macOS has no built-in application updater; use the matching release archive or build locally. See [update requirements and rollback](../crates/app/docs/update.md).
- **Something does not open:** From another terminal, run `tundra-cli doctor` and `tundra-cli debug paths` to see missing capabilities and resolved data directories. In the portable directory, use the executable path if `tundra-cli` is absent from PATH.
- **A window is stuck:** Try a normal close first. If it cannot respond, [watchdog commands](../crates/cli/docs/commands.md#watchdog) can list or stop the window from a second terminal under the same account, even when external UI control is disabled.
- **A session stopped or data failed to load:** Check the next-startup message, the Logs app, and any reported backup paths. Back up your configuration and state before using [the data reset command](../crates/cli/docs/commands.md#data-reset), which deletes saved data.
