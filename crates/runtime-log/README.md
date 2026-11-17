# runtime-log

`runtime-log` stores and queries structured runtime events, handling redaction, rotation, retention, capacity limits, and safe cleanup. Shell's Logs page and the CLI share these data; `platform` provides Linux journal and selected-file reading.

## Modules

| Module | Responsibility |
| --- | --- |
| [`lib.rs`](src/lib.rs) | Exposes event recording and the process-local logger used by hosts and libraries. |
| [`model`](src/model.rs) | Defines event context, stable levels and phases, query filters, and status types. |
| [`writer`](src/writer.rs) | Receives events through a bounded queue, deduplicates and flushes them, and manages file rotation, retention, and capacity. |
| [`reader`](src/reader.rs) | Reads a bounded amount of data, filters rows, and prepares output for queries, following, and export. |
| [`privacy`](src/privacy.rs) | Applies redaction, strips control sequences, and limits persisted text. |
| [`clear`](src/clear.rs) | Previews and clears managed log files using the supported cleanup modes. |

The UX failure phase `Incident` is consistently mapped to the highest level, `Critical`, during queries. Original files remain unchanged, and recovery results continue to use their original levels.

Log bodies use a fixed language and stable event codes, rather than being generated from editable translation files. Passwords, tokens, clipboard contents, file bodies, and command contents are not used as log context.

## Documentation and validation

The [log guide](docs/logs.md) describes sources, permissions, TUI, CLI, following, export, cleanup, and disk quotas. Incident identities and task IDs are managed by [watchdog](../watchdog/README.md).

```sh
cargo test --locked -p runtime-log
```

After changing pages or commands, also run the affected Shell/CLI tests. Validate Linux journal/dmesg sources in a Linux environment.
