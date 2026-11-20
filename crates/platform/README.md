# platform

`platform` is the operating-system boundary. Its `Platform` trait provides the common native interface; `management` describes system administration in typed queries and commands. Shell and CLI select those operations, and platform backends own native file, process, authorization, and command handling.

## Module structure

| Module | Responsibility |
| --- | --- |
| [`platform`](src/platform.rs), [`mock`](src/mock.rs) | Common interface for current username, paths, files, volumes, trash, desktop integration and other capabilities. The mock supports tests without native writes. |
| [`linux/`](src/linux/), [`macos/`](src/macos/), [`windows/`](src/windows/) | Implement the trait for each OS. Linux also provides NSS identity, AccountsService, polkit and authorization sessions, D-Bus, power, and XDG integration. |
| [`paths`](src/paths.rs), [`installation`](src/installation.rs) | Resolve configuration, data, state, cache, logs and temporary paths; classify the installed program for diagnostics and updates. |
| [`document`](src/document.rs), [`file_operations`](src/file_operations.rs) | Check and read regular files, fingerprint and save documents, identify objects, and rename without replacing an existing destination. |
| [`process`](src/process.rs), [`managed_process`](src/managed_process.rs), [`command_session`](src/command_session.rs) | Describe and run programs, own cancellation/reaping, and maintain the REPL's working directory, environment and executable lookup. |
| [`pty_exec`](src/pty_exec.rs), [`terminal`](src/terminal.rs) | Gate Unix PTY launches on guardian registration; describe terminal capabilities, colors, and fullscreen sequences. |
| [`system_monitor`](src/system_monitor.rs), [`service`](src/service.rs) | Define and collect system metrics; share structured service errors across native adapters. |
| [`management/`](src/management/) | Typed management queries, actions, forms and fixed backends; see the breakdown below. |
| [`shell_control`](src/shell_control.rs) | Same-user local socket or named-pipe transport, peer checks, registrations, and session discovery. |
| [`linux_logs`](src/linux_logs.rs), [`log_file`](src/log_file.rs), [`task_history`](src/task_history.rs) | Read native logs or selected log files; record operation summaries and captured output for pages, helpers, and scheduled tasks. |
| [`task_events`](src/task_events.rs), [`diagnostics`](src/diagnostics.rs), [`error_details`](src/error_details.rs) | Deliver results and wake host loops; diagnose runtime capabilities and retain native error information. |

Document reads reject FIFOs, devices, symbolic links, and other non-regular files, and recheck file identity after opening.

Hosts can install one process-local `task_events` notifier. Result channels notify only after successful delivery; shared result slots and PTY readers notify after publishing and releasing their locks. Libraries retain ordinary channel behavior when no host notifier is installed.

Explorer moves, renames, and undo share `file_operations`: Linux uses `renameat2(RENAME_NOREPLACE)`, macOS uses `renamex_np(RENAME_EXCL)`, and Windows uses `MoveFileExW` without replacement. An existing destination preserves both files. File identities serialize over authenticated session IPC so the shared undo service can validate the same object and parent directories across app processes.

System command sessions use a fixed interpreter after its first execution. Windows supports CMD, Windows PowerShell, and PowerShell 7. Unix supports the platform default, sh, Bash, Zsh, and Fish. The session reads the working directory and exported environment after each command.

## Management interfaces

`ManagementQuery`, `ManagementSnapshot`, `ManagementAction`, and `ManagementCommand` describe queries, page data, available operations, and execution requests. Action IDs, field IDs, and target identities remain stable; display text uses the caller's language.

Current management types cover services, processes, packages, networks, disks, users, system configuration, configuration backups, firewalls, SSH, scheduled tasks, task history, file permissions, and operating-system settings. User management is Linux-only.

| Modules under [`src/management/`](src/management/) | Responsibility |
| --- | --- |
| `mod.rs`, `configuration`, `selection`, `problem` | Shared dispatch, query/action contracts, configuration drafts and validation, row identities and selection, and structured operation problems |
| `authorization`, `capability`, `client`, `helper`, `termination` | Native backend availability, authorized connections and request execution, interaction and process control |
| `services`, `processes`, `scheduled_tasks`, `task_history` | systemd units, process identity/metrics/actions, systemd timers and cron, and the task-history page |
| `packages`, `homebrew`, `winget` | APT/DNF/pacman on Linux, Homebrew on macOS (exported as `packages` there), and WinGet on Windows. Linux package submodules handle queries, sources, parsing, execution and progress. |
| `network`, `disks` | Interfaces, connections, Wi-Fi and configuration/rollback; volumes, mount persistence, health checks and directory scans |
| `users`, `file_permissions`, `system_settings` | Account and group operations, ownership/mode/ACLs, and hostname, timezone and time synchronization |
| `system_config`, `config_backups`, `ssh`, `firewall`, `listening_ports` | Protected configuration and history, SSH validation and recovery, native firewall rules and listening socket inventory |

- Queries use the current process's permissions; operations requiring additional permissions first go through AA and system authorization.
- Batch operations check all targets first. Services use one systemctl invocation, processes are bound to their original identity, and changes from the same cron source are saved together.
- APT, DNF, and pacman support batch package operations; pacman installation and upgrades include a full system upgrade.
- Homebrew is in `management/homebrew/` and distinguishes Formula and Cask targets; WinGet uses exact package IDs and versions.
- If a backend rejects an operation or cannot confirm the result, read the actual state first to avoid repeating writes through a different backend.
- Actions declare destructive risk through `dangerous`, independently of confirmation or authorization; services mark start or stop as the primary action according to their current state. firewalld exit code 253 is treated as insufficient permission and retains the existing authorized-query path and native error details.
- Network connection states and configuration restrictions use English and Chinese resources; device identities and native configuration syntax remain unchanged for later state checks.

## Shell control connections

The Shell and app hosts request larger Unix socket buffers for window frames. Guardian traffic and external CLI control use separate connections with their own limits.

`shell_control` provides local connections for the external CLI and for managed Shell, application, and service processes. Unix uses private sockets and checks the peer's UID and PID. Windows named pipes and registration directories allow only the current user's SID, and clients check the server PID.
Shell control servers created in one process use different endpoints even when started within the same clock tick.

Managed window and service traffic can use `AsyncLocalListener` and `AsyncLocalConnection`. They preserve those credential checks and use Tokio readiness notifications, including overlapped named pipes on Windows.

Their asynchronous stream cancels Windows pipe lingering when it closes, including when split reader and writer tasks are aborted, matching the synchronous connection's cleanup behavior.

Each Shell poll processes at most 16 connections, with 64 KiB of reads and writes per connection; requests are limited to 1 MiB and responses to 32 MiB. Connections are cleaned up after 30 seconds, and registration files and sockets are removed on exit. Protocol types are defined by [shell-control](../shell-control/README.md).

The guardian publishes a separate `shell-watchdog` registration under the application cache directory. The watchdog endpoint accepts direct same-user requests up to 4 KiB and returns responses up to 1 MiB. On Linux the guardian also checks the embedded CLI's process descriptor and ancestry before accepting child resource registrations. Its protocol types are defined by [workspace-protocol](../workspace-protocol/README.md).

Managed Windows workers send desktop opens to the guardian, which calls the platform's normal desktop launch outside the workers' job objects. Closing a managed window leaves those desktop programs running.

On Unix a managed PTY starts in a separate terminal session. Its initial entry point waits on a private connection inside the guardian's run directory. The worker verifies the peer PID, registers the session with the guardian, then passes the target environment and lets the program start.

## Documentation

| Topic | Document |
| --- | --- |
| Paths, startup identity, desktop and file integration | [System integration](docs/integration.md) |
| Services, processes, packages, and tasks | [Linux system management](docs/linux-management.md) |
| Authorization connections, revocation, and fixed operations | [Authorization and helper connections](docs/authorization.md) |
| Networks, Wi-Fi, disks, and directory scans | [Networks and disks](docs/network-disks.md) |
| Users, groups, and SSH public keys | [User management](docs/user-management.md) |
| File permissions, ownership, and recursive scope | [File permissions](docs/file-permissions.md) |
| Hostname, timezone, and automatic time synchronization | [Operating-system settings](docs/system-settings.md) |
| Configuration validation, saving, and recovery | [System configuration files](docs/system-config.md) |
| Configuration history, retention, and cleanup | [Configuration backups and recovery](docs/config-backups.md) |
| Listening ports, UFW, and firewalld | [Firewall and ports](docs/firewall.md) |
| SSH settings and confirmation from a new connection | [SSH service management](docs/ssh-management.md) |
| systemd timers and cron | [Scheduled tasks](docs/scheduled-tasks.md) |
| Task summaries, output, and cleanup | [Task history](docs/task-history.md) |
| Homebrew Formulae and Casks | [Homebrew packages](docs/homebrew.md) |
| WinGet packages | [Windows packages](docs/windows-packages.md) |
| Process, log, and directory performance | [Performance and retesting](docs/performance.md) |
| Operations commands, task IDs, and exit codes | [Operations CLI](../cli/docs/operations.md) |

## Validation

```sh
cargo test --locked -p platform
cargo test --locked -p platform --test shell_control
```

Run Linux management tests on Linux or WSL. Use the corresponding isolated environment or test host for real authorization, service writes, wireless devices, mounts, and physical-disk operations; see each topic's document for specific entry points.
The private D-Bus account test covers administrator visibility of root, service, local, and directory accounts. The watchdog connection test models the server poll between discovering the endpoint and connecting a client.

`workspace_watchdog_constraints` checks that background work and processes have an owner and that low-level bootstrap adapters remain explicit.
