# cli

`cli` provides `tundra-cli`: configuration, diagnostics, logs, Linux operations commands, Shell interface control, managed-process recovery, and an interactive command line. It also hosts update and authorization helper entry points; these internal entry points are not part of the public scripting interface.

## Common entry points

| Usage | Example |
| --- | --- |
| Commands from an external terminal | `tundra-cli doctor` |
| View help | `tundra-cli help` |
| Linux system user management | `tundra-cli users --help` |
| View version | `tundra-cli --version` |
| Inspect a built-in app icon | `tundra-cli debug asset app_icons --builtin.clock` |
| UX commands in the built-in Command Line | `/doctor`, `/config get` |
| System commands in the built-in Command Line | `ls -la` or `dir` |

Normal REPL input is passed to the system command interpreter. Tundra commands use the `/` prefix. The built-in Command Line shows how to get help (`/help`) and leave (`exit`) in the workspace language. The session retains its working directory and exported environment variables without changing the Tundra main process or system environment configuration.

On Linux the embedded REPL registers child process resources through its Command Line window's guardian. The guardian verifies the embedded CLI's process ancestry and tracks registered authorization helpers through cleanup if the window dies.

Tab accepts the gray inline completion on Windows and Unix. Right cycles through the candidates below the input and previews the selection in gray. It moves the cursor normally within existing text. Typo suggestions remain informational.

## Modules and responsibilities

| Location | Responsibility |
| --- | --- |
| [`main`](src/main.rs), [`embedded_children_linux`](src/embedded_children_linux.rs) | Dispatch queries and internal helper entry points, start standalone supervision, and register managed children from the embedded Linux CLI. |
| [`arguments`](src/arguments.rs), [`help_text`](src/help_text.rs) | Parse user arguments into `CliCommand` and configuration fields; render topic help and syntax errors. |
| [`runner`](src/runner.rs) | Loads the appropriate environment and dispatches parsed commands; applies the output and exit-code contract. |
| [`repl`](src/repl.rs), [`repl/completion`](src/repl/completion.rs) | Read interactive lines, distinguish leading-slash commands from native commands, manage history/completion, and notify the embedded host of command completion or special exit actions. Native session state lives in `platform`. |
| [`management_client`](src/management_client.rs) | Reuses platform's typed management connection for native operations. |
| [`commands/`](src/commands/) | Implement the command families below. |
| [`build.rs`](build.rs) | Copies the canonical asset tree to the build output for CLI diagnostics and previews. |

| Modules under `src/commands/` | Responsibility |
| --- | --- |
| `config`, `storage_reset`, `launcher` | Read and update settings, reset application data, and manage pinned entries. |
| `doctor`, `debug`, `path_report`, `asset` | Report runtime capabilities and resolved paths, drive previews and explicit failure exercises, and inspect or render assets. |
| `logs`, `clear_logs` | Query and follow logs, export structured output, and preview or confirm log cleanup. |
| `management` | Parse native operation arguments, run typed queries or commands through the client, and render task IDs and outcomes. |
| `ui`, `watchdog` | Connect to the live Shell for opt-in input, or to the independent guardian for process listing and recovery. |

Help and version queries print built-in text directly without starting watchdog or reading configuration. `doctor` is a top-level command; the former top-level debug entry points are grouped under `debug`.

## Shell interface control

`tundra-cli ui` can list the current user's Shell sessions, read interface snapshots, and send clicks, moves, drags, scrolling, keys, and text. This feature is disabled by default; enable it in **Settings → System → External CLI control**.

```sh
tundra-cli ui sessions
tundra-cli ui snapshot
tundra-cli ui wake
```

Results use protocol version 1 JSON. Multiple sessions require `--session`; snapshots must be read again after real user input, and timed-out input is not automatically resent. See [UI control](docs/ui-control.md).

## Watchdog recovery

Watchdog commands retain public JSON protocol version 1 independently of private managed-worker IPC versions.

`tundra-cli watchdog list` shows runs and workers owned by the current OS user. Use a run ID and an application session number from that list to force kill one window, or use `--shell` to end the entire run. These commands contact the watchdog directly and work when the Shell interface stops responding. See [watchdog commands](docs/commands.md#watchdog) for arguments and result states.

## Documentation

- [Commands, configuration, and REPL](docs/commands.md)
- [Operations commands, authorization, and exit codes](docs/operations.md)
- [UI control and JSON protocol](docs/ui-control.md)
- [Log queries, following, and export](../runtime-log/docs/logs.md)
- [Debug interface previews](../shell/docs/previews.md)

## Build and validation

```sh
cargo build --locked -p shell -p cli
cargo test --locked -p cli
```

Shell's built-in Command Line uses `tundra-cli` from the same directory, so rebuild both Shell and CLI after updates. Real Linux PTY validation of UI control uses `python3 scripts/tests/shell-control.py target/debug/tundra-shell`.

The embedded `/debug test-watchdog-panic` command explicitly requests Shell's real fullscreen panic boundary. An ordinary system command or external Launcher program exiting with status 76 remains a command result and does not request a Shell panic.
