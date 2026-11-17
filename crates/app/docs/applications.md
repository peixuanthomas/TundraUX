# Application state and built-in applications

This page describes the state behind the main applications. For installation, shortcuts, and day-to-day tasks, see the [user guide](../../../docs/user-guide.md).

## AppState

`app::AppState` stores the clock, exit state, notifications, authentication session, user, configuration, appearance, and optional Explorer, Launcher, Diagnostics, and Editor state.

```rust
AppState::dispatch_at(AppCommand, Instant) -> AppAction
AppState::snapshot() -> AppSnapshot<'_>
```

Passing the monotonic clock `Instant` explicitly makes notification timeouts deterministic to test. `AppSnapshot` provides a borrowed read-only view; state changes go through `AppCommand`. `AppAction` describes `Redraw`, `Exit`, `Reboot`, or `PowerOff`; Shell performs the terminal and system operations.

Notifications include 4-second toasts, keyed alerts, FIFO modals, and critical modals that can preempt others. The response queue has bounded capacity.

## Explorer

Explorer maintains filtering, sorting, multiple selection, history, the clipboard, drag and drop, conflict handling, and progress. The platform layer performs directory enumeration, copying, moving, renaming, opening, and system trash operations.

`ExplorerTaskEngine` provides a single worker, cancellation, staging, journals/checkpoints, and crash recovery for copy, move, and trash operations. Some lightweight file and storage operations still run in the command apply path.

The shared clipboard and move/rename undo state belong to the current workspace run; list filters, selection, and navigation belong to a page. Batch commands act on selected entries in the visible list. Right-clicking empty space in a regular directory can open Command Line there by passing the working directory to the child process. See [Explorer use](../../../docs/user-guide.md#browse-and-edit-files) and the [selection rules](../../../docs/list-selection.md).

## Launcher

Launcher stores executable targets and a fixed order, with icon and list views. Records include an absolute, non-symlink target, type, approver, and approval time. Paths and execution approval are checked again before launch; scripts and installation packages retain a second confirmation.

Built-in applications are declared per platform in `BUILT_IN_APPLICATIONS` and are not written to removable configuration. Launcher includes only entries marked `fixed_in_launcher`; system applications are opened from Home. The package entry uses distribution tools on Linux, WinGet on Windows, and Homebrew on macOS.

For external programs, **Run settings (F4)** saves the choice of system launch or an embedded terminal, arguments, and working directory. Arguments are parsed into an array without variable expansion. Custom commands can return immediately after exit or retain output until Enter is pressed.

For the user flow, see [Launcher and Command Line](../../../docs/user-guide.md#launch-programs-and-run-commands). For native command session state, see [CLI commands](../../cli/docs/commands.md#session-directory-and-environment) and [terminal-runtime](../../terminal-runtime/README.md).

## Plain-text editor

The editor uses `Rope` and a grapheme cursor to read and write plain text directly. Markdown files are edited as source text, without generating rich text or previews.

- Opening and saving use background tasks. Save snapshots are tied to an exact revision, so older tasks cannot clear newer changes.
- Each document has its own recovery ID, process file lock, and throttled writes; drafts are claimed individually after an abnormal exit.
- The extensions Explorer opens in the editor by default are defined by `storage::DEFAULT_EDITOR_EXPLORER_OPEN_EXTENSIONS`; custom lists and empty lists take precedence.
- LSH provides syntax highlighting, with selection styling taking precedence. Display choices do not affect file contents.
- Line-change markers compare the current text with the text from the most recent successful save, independently of Git.

For complete controls, see [Editor and script workspace](../../../docs/source-editor.md).

## Clock, system status, and diagnostics

Clock scheduling is in `app::clock`. User alarms and countdowns are saved in `clock.v2.json` (schema 3); SessionServices checks each update against the shared clock revision. Linux follows system time and timezone. Windows and macOS use the application time configuration. See [storage documents](../../storage/README.md#main-documents) and [time synchronization](../../time/README.md).

System Status displays system metrics, processes, network, and storage. The dashboard supports adding, editing, and arranging widgets; the system overview shows detailed metrics separately.

`app::system_settings` defines `SystemSettingsBackend`, snapshots, requests, and operation state. The backend performs I/O through supervised tasks; Shell turns availability and failure reasons into interface state.

`DiagnosticsTaskRuntime` is managed by watchdog, and each page can scan independently. Repairs first show a preview and require user confirmation. After storage repair, the "restart required" state is latched, and the repaired storage is used after restart. Logs and incident reports are both viewed in the Logs application; see the [log guide](../../runtime-log/docs/logs.md).
