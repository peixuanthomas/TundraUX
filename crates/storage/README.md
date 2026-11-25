# storage

`storage` reads and saves application documents, validates format versions, and provides atomic writes, corruption recovery, and data reset. `platform` provides platform paths and low-level file operations.

## Modules

| Location | Responsibility |
| --- | --- |
| [`layout`](src/layout.rs), [`manager`](src/manager.rs) | Map native app directories to document paths and give callers a single load, update, and reporting API. |
| [`descriptors`](src/descriptors.rs), [`schema`](src/schema.rs) | Define filenames, formats, and schema versions and check that documents use a supported schema. |
| [`document_io`](src/document_io.rs), [`atomic_write`](src/atomic_write.rs) | Parse and validate JSON/TOML; create private directories and save through the platform's atomic writing path. |
| [`config_document`](src/config_document.rs), [`animation`](src/animation.rs) | Define settings, defaults, validation and normalization, including appearance and transition timings. |
| [`profile_document`](src/profile_document.rs), [`system_status_dashboard`](src/system_status_dashboard.rs), [`clock_document`](src/clock_document.rs) | Store per-user appearance, dashboard widgets and placements, and alarms/timers. |
| [`state_documents`](src/state_documents.rs), [`trash_document`](src/trash_document.rs) | Represent general state, recent files, recoverable sessions, and trash metadata. |
| [`initialization`](src/initialization.rs), [`document_health`](src/document_health.rs), [`recovery`](src/recovery.rs), [`reset`](src/reset.rs), [`error`](src/error.rs) | Initialize storage and check document health; repair or reset saved files and report structured errors. |
| [`session_store`](src/session_store.rs) | Allows a managed host to substitute the SessionServices configuration adapter for local configuration I/O. |

## Main documents

| File | Format | Content |
| --- | --- | --- |
| `config.toml` | TOML, schema 3 | Language, timezone, weather, appearance, AA, automation, and application settings |
| `profiles.v1.json` | JSON, schema 3 | Appearance and dashboard preferences keyed by system username |
| `state.v1.json` | JSON, schema 3 | General application state |
| `recent-files.v1.json` | JSON, schema 3 | Recent files |
| `sessions.v1.json` | JSON, schema 3 | Recoverable session data |
| `clock.v2.json` | JSON, schema 3 | Alarms and timer items keyed by system username |
| `trash/trash.v1.json` | JSON, schema 3 | Application trash manifest |

`StorageManager` and `StorageLayout` manage document locations and loading, `StorageDescriptor` describes formats, and `StorageLoadReport` returns normalization and recovery results. Shell and app workers install a `SessionConfigStore` for their run's configuration path; its shared services process serializes configuration writes and rejects stale revisions. Other paths and standalone tools retain ordinary local storage.

Session configuration reads reuse the worker's latest successful shared snapshot. Its configuration and authorization revisions are in-memory metadata and are never persisted. Interactive callers submit writes in the background and apply their results after persistence; the synchronous storage adapter remains available to background operations.

## Safe saving and recovery

Documents are read up to a default limit of 1 GiB. Length, modification time, and path identity are checked before and after reading. Each path component rejects symbolic links, junctions, and reparse points; conditional writes use document fingerprints to check for external modifications.

Writing proceeds through a temporary file in the same directory, file synchronization, atomic replacement, and parent-directory synchronization. Linux uses `openat` and `O_NOFOLLOW` to prevent ancestor-directory redirection. Application directories use `0700`, and private documents use `0600`.

Unsupported schemas are rejected without rewriting their contents. If a current document cannot be parsed, the original is renamed to `<filename>.corrupt.<timestamp>`, then a default document is created and Shell shows a notice.

## Configuration notes

- `[automation].enabled` stores external CLI control, defaulting to `false`.
- `[top_bar]` stores `status_alignment` (`left`, `center`, or `right`) and the `show_help`, `show_time`, and `show_esc` preferences. The defaults center the status and show Help, time, and Back when available. Older configurations with `show_tasks` still load. Tasks is available from the title's power menu.
- `[appearance].icon_display_mode` defaults to `ascii` on Linux and `image` on other platforms. User profiles use the same default when the field is missing.
- `screen_timeout_seconds` stores the idle-weather timeout; `0` means never, and the default is 300 seconds.
- `[editor].vim_mode_enabled` stores the default for newly opened editor sessions, disabled by default. F8 changes only the current editor session.
- `[editor].explorer_open_extensions` controls the suffixes handed to the built-in editor. The default includes Word, Excel, PowerPoint and raster image formats for read-only previews. On upgrade, a list matching a default from TundraUX 2.x switches to the current defaults. Storage saves only customized lists, including an empty list, so default associations follow future updates.
- `[command_line].shell` selects the interpreter for new Command Line sessions. Its default is `default`. Windows accepts `cmd`, `powershell`, and `pwsh`. macOS and Linux accept `sh`, `bash`, `zsh`, and `fish`.
- `animation_timings` stores separate acceleration, constant-speed, and deceleration durations for page/dialog/menu animations, each from 1 to 2000ms.
- Default page/dialog/menu phases are `[40, 80, 40]`, `[45, 90, 45]`, and `[30, 60, 30]` milliseconds. Existing explicit values and animation speed remain effective; reduced motion bypasses transitions.
- The default border is Glacier Night `#29434E`, and the accent is `#63D3E5`. Missing preferences use current defaults and explicit colors remain supported.
- Launcher's `order` stores a unified ID order, `custom` stores names, icons, commands, and exit options, and `terminal` stores argument arrays and working directories.

`reset_saved_content` clears saved data. The caller is responsible for backup, confirmation, and restart workflows.

## Validation

```sh
cargo test --locked -p storage
```

Tests cover current document formats, invalid schemas, atomic writes, path safety, and corruption recovery.
