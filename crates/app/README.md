# app

`app` owns application state and domain commands. Shell turns keys, clicks, and focus changes into domain actions, then reads an application snapshot to prepare the UI. This crate has no dependency on UI, Ratatui, or Crossterm. Native I/O uses `platform`; structured application documents use `storage`.

Each warning, error, or critical alert gets a report ID and keeps its original localizable message. A dialog produces a report when its caller marks it as a problem. Hosts drain these reports independently of the currently visible page. Dismissing an ID clears its matching live alert. A later occurrence under the same key has its own ID.

## Modules

| Module | Responsibilities |
| --- | --- |
| [`application/`](src/application/) | `state` defines `AppState`, commands, actions, and snapshots. `catalog` lists platform-specific built-ins and setup choices. `notification` manages alerts, modals, and toasts. `pages` defines page kinds and instance IDs. |
| [`editor/`](src/editor/) | Owns documents, revision-specific saves, text editing, recovery, and the mirror used by Vim. See the editor breakdown below. |
| [`script/`](src/script/) | Stores run settings and breakpoints per document, resolves interpreter and arguments, and keeps track of the current run and pause. Actual execution is in [script-runtime](../script-runtime/README.md). |
| [`explorer/`](src/explorer/) | `state` models directories, filtering and selection; `controller` dispatches effects; `files` handles file types and path operations. `tasks`, `operations`, and `shared_operations` handle long writes, undo, and the shared clipboard interface. |
| [`launcher/`](src/launcher/) | Merges the built-in catalog with saved entries, checks executable targets, and plans the launch, including terminal settings and custom commands. |
| [`diagnostics/`](src/diagnostics/) | Builds system and storage health reports, runs supervised scans and repair tasks, and checks language resources through `localization`. |
| [`runtime_logs`](src/runtime_logs.rs) | Coordinates log queries and source visibility; `documents` prepares log content for display and `incidents` merges accessible watchdog reports. |
| [`system_settings`](src/system_settings.rs) | Defines the OS-settings backend, availability, snapshots and requests so hosts can schedule work without leaking UI state into the backend. |
| [`clock`](src/clock.rs) | Advances scheduled alarms and countdowns from clock snapshots and returns due events. Profile persistence is in storage; Shell presents the reminders. |
| [`update/`](src/update/) | Orchestrates update checks, preparation, replacement, and rollback. `git` reads remote history; `http` fetches update data; `release` handles Linux artifacts; `toolchain` checks source-build prerequisites; `vim` validates or installs the engine; `cancellation` cleans up or preserves transactions safely. |
| [`build.rs`](build.rs), [`build_support`](build_support.rs) | Compile the LSH language definitions and supply version, commit, and dirty-state metadata for update checks. |

Build identity follows the Git checkout and its index, including linked worktrees and packed refs. A nonempty `TUNDRAUX_BUILD_COMMIT` is the sole input for the release identity. Source archives default to `unknown`.

Callers modify state through `AppCommand` and read a consistent view through `AppSnapshot`. `clock_snapshot()` samples only time when no page snapshot is needed; `storage_config_revision()` lets hosts observe effective configuration changes without copying the configuration. `AppAction` only expresses requests to redraw, exit, reboot, or power off; Shell performs terminal restoration and process operations.

## Main state and workflows

### Explorer and Launcher

Explorer manages directories, filtering, sorting, multiple selection, the clipboard, and operation progress. Optional subfolder searches match names under the current directory and retain each result’s native path. The cancellable scan respects directory read permissions and visibility settings and skips directory links. `ExplorerTaskEngine` provides cancellation, checkpoints, and recovery for lengthy file tasks; the platform layer performs the actual copy, move, trash, and directory operations.

`ExplorerFileService` shares copied and cut files across pages and app processes in the current Shell session. Workers use a bounded shared-operations backend; the services process owns the clipboard, busy state, and undo history and scopes them to that session. It records completed moves and renames within one filesystem for cross-page undo. Undo restores the location while preserving later content edits, checks the object and both parent-directory identities, and rejects replaced objects or an occupied original path without recursively scanning directories. Cross-volume moves, replacements, and directory merges cannot be undone and clear older undo records. Native paths retain their original bytes or UTF-16 units over IPC. The system file clipboard is not connected; the text clipboard does not carry file transfers.

Shell queues clipboard mutations directly to the shared service and reads its authoritative clipboard before planning Paste. File tasks wait for the shared busy reservation and publish their completed move records, consumed cut paths, and busy release together. Cached snapshots provide display state; operation requests and successful replies determine file-operation behavior.

Explorer creates and extracts ZIP, 7z, RAR, TAR, and TAR.GZ archives. ZIP, 7z, and RAR support passwords. The managed Explorer worker runs the archive codecs and reports byte progress. The file task writes into a private sibling staging directory and publishes the result after checking cancellation and the identity of any confirmed replacement. Archive passwords stay in memory and are omitted from the recovery journal.

Launcher lists the built-in applications available on the current platform, plus custom Shell commands and approved external programs. Home shows user-selected shortcuts into the same catalog. Built-in application IDs identify their shared theme icons. Custom entries support a name, a 5×2 terminal-cell text icon, arguments, a working directory, and retained output after exit. Custom commands use `/bin/sh -c` on Unix and `cmd.exe /C` on Windows.

### Editor and scripts

| Modules under [`src/editor/`](src/editor/) | Responsibility |
| --- | --- |
| `document`, `state`, `controller` | Own the document and its metadata, open-document state, and editor commands and effects |
| `pdf_preview` | Parse PDFs with hayro and fit the requested page to the editor viewport |
| `source_buffer`, `source_changes`, `line_changes` | Store source text and history; represent changed line ranges for incremental work and compare against saved text for line markers |
| `syntax`, `c_syntax`, `search`, `soft_wrap`, `json_format` | Compute highlighting, C lexical tokens, matches, visual row mapping, and JSON formatting |
| `background`, `recovery` | Run revision-tagged computations and I/O away from the UI loop; persist and claim recoverable drafts separately |
| `rich_document`, `rich_edit`, `markdown_codec` | Represent, edit, and serialize structured Markdown for rich-document support. Files normally opened in the standard editor use source text. |
| `office_preview` | Convert `.docx` and `.xlsx` to Markdown in memory and create a read-only document with the original path. |

The editor handles user-visible characters as graphemes, using save snapshots tied to an exact revision and separate recovery records. Bytes written externally by Vim enter the same atomic save workflow through `SaveSnapshot::external_bytes`, without re-encoding or changing line endings.

Editor calculations, Explorer tasks, and diagnostic results notify an optional host event loop after publishing their results. Domain code uses the platform notifier and does not depend on Shell.

Script state stores the interpreter, arguments, breakpoints, run ID, and pause ID for each document. Editing, undo, and redo move breakpoints; results from an old run or pause location do not overwrite current data.

### Updates and logs

Updates prepare Shell, CLI, and the complete `vim/` together, validate assets, licenses, and upstream metadata, and roll them all back on failure or startup timeout. Vim installation packages are checked for SHA-256, directory contents, and the pinned version; Windows also checks the iconv DLL, license, and source code.

Source updates can build a commit from any branch in the public remote history. Git reads the history and draws its branches and merges without checking out files. After confirmation, the updater downloads and builds the selected commit. It reuses a complete installed Vim when its upstream metadata matches the selected source's pin. Cargo and Vim output goes to an `update-*.log` file that remains after build failure or cancellation.

Runtime logs merge failure reports that the user is authorized to access at the highest severity, `Critical`, sort them by time, and respect report access restrictions. A read-only Editor window can use the original selection and filters to refresh a log snapshot.

## Documentation

- [Application state and built-in applications](docs/applications.md)
- [Application self-updates](docs/update.md)
- [Update tests](docs/update-tests.md)
- [Editor and script workspace](../../docs/source-editor.md)
- [Saving system configuration](../platform/docs/system-config.md)
- [Workspace architecture and crate boundaries](../../docs/README-TECHNICAL.md)

## Validation

```sh
cargo test --locked -p app
```

Use the named integration tests `editor`, `explorer`, `launcher`, and `script` for those workflows. Update tests reject legacy journals and check that a current journal records the Vim transaction. Update and Vim installation tests run offline with fixtures for validation, cancellation, file safety, and rollback. Run workspace tests when changing cross-crate commands or shared types.
