# TundraUX documentation

[Project overview](../README.md) · [简体中文项目介绍](README.zh-CN.md)

Start with the [user guide](user-guide.md) to install TundraUX, open apps, and arrange windows. For source builds or code changes, use the [technical guide](README-TECHNICAL.md).

## Install and use TundraUX

| What you want to do | Guide |
| --- | --- |
| Install, finish first-time setup, and learn the workspace | [User guide](user-guide.md) · [Linux runtime guide](packaging/linux/README-LINUX.txt) |
| Split, switch, minimize, and close windows | [Window controls](user-guide.md#navigate-and-arrange-windows) · [Page shortcuts](../crates/shell/docs/session.md#common-shortcuts) |
| Browse, organize, and select files | [File Explorer](user-guide.md#browse-and-edit-files) · [List multi-selection](list-selection.md) |
| Edit files, use Vim, or debug Bash scripts | [Editor and script workspace](source-editor.md) |
| Open programs or use the built-in Command Line | [Launcher and Command Line](user-guide.md#launch-programs-and-run-commands) · [CLI commands](../crates/cli/docs/commands.md) |
| Check logs, trace a task, or recover a stuck window | [Logs](../crates/runtime-log/docs/logs.md) · [Task history](../crates/platform/docs/task-history.md) · [Watchdog commands](../crates/cli/docs/commands.md#watchdog) |
| Configure the interface and language | [Appearance and input](user-guide.md#use-clock-weather-and-appearance-settings) · [Language packs and repair](../crates/i18n/docs/localization.md) |
| Update TundraUX | [Update modes, requirements, and rollback](../crates/app/docs/update.md) |

## Manage a device

| Area | Guide |
| --- | --- |
| Linux overview and software packages | [Linux system management](../crates/platform/docs/linux-management.md) |
| macOS or Windows software packages | [Homebrew](../crates/platform/docs/homebrew.md) · [WinGet](../crates/platform/docs/windows-packages.md) |
| Approval, input requests, and permissions | [AutoAdmin workflow](../crates/auto-admin/docs/workflow.md) · [Authorization connections](../crates/platform/docs/authorization.md) |
| Network and storage | [Networks, Wi-Fi, mounts and disk health](../crates/platform/docs/network-disks.md) |
| Accounts and files | [Users and groups](../crates/platform/docs/user-management.md) · [File permissions and ownership](../crates/platform/docs/file-permissions.md) |
| Host settings and backups | [Operating-system settings](../crates/platform/docs/system-settings.md) · [System configuration files](../crates/platform/docs/system-config.md) · [Configuration backups](../crates/platform/docs/config-backups.md) |
| Service access and schedules | [Firewall and ports](../crates/platform/docs/firewall.md) · [SSH settings](../crates/platform/docs/ssh-management.md) · [Scheduled tasks](../crates/platform/docs/scheduled-tasks.md) |
| Script system operations or control a running UI | [Operations CLI](../crates/cli/docs/operations.md) · [External UI control](../crates/cli/docs/ui-control.md) |

## Develop and maintain the workspace

| Topic | Guide |
| --- | --- |
| Build prerequisites, process roles, crate boundaries, and where to change code | [Technical guide](README-TECHNICAL.md) |
| Startup, input, page lifecycle, and frame composition | [Shell sessions](../crates/shell/docs/session.md) · [Application state](../crates/app/docs/applications.md) |
| UI rules and shared visual checks | [UI requirements](UI-requirements.md) · [Debug previews](../crates/shell/docs/previews.md) |
| Data directories, startup identity, and desktop integration | [Platform integration](../crates/platform/docs/integration.md) |
| Supervised tasks, incident reports, and recovery | [Watchdog runtime](../crates/watchdog/docs/runtime.md) |
| Automated tests, real terminals, and platform acceptance | [Testing guide](testing.md) · [Linux update tests](../crates/app/docs/update-tests.md) |
| Editor and system performance | [Large-file editor](editor-scalability.md) · [Platform performance](../crates/platform/docs/performance.md) |
| Distribution | [Linux packaging](README-TECHNICAL.md#linux-packaging) · [Bundled Vim](packaging/vim.md) |

### Crate READMEs

See the [complete crate responsibilities table](README-TECHNICAL.md#crate-responsibilities) for each crate's purpose. The READMEs below map its source modules and tests.

| Area | Crates |
| --- | --- |
| Workspace and UI | [shell](../crates/shell/README.md), [app](../crates/app/README.md), [ui](../crates/ui/README.md), [cli](../crates/cli/README.md) |
| Operating system and data | [platform](../crates/platform/README.md), [storage](../crates/storage/README.md), [identity](../crates/identity/README.md), [auto-admin](../crates/auto-admin/README.md) |
| Terminal and editors | [terminal-runtime](../crates/terminal-runtime/README.md), [vim-runtime](../crates/vim-runtime/README.md), [script-runtime](../crates/script-runtime/README.md) |
| Shared services and resources | [system-services](../crates/system-services/README.md), [time](../crates/time/README.md), [weathr](../crates/weathr/README.md), [ascii-assets](../crates/ascii-assets/README.md), [i18n](../crates/i18n/README.md) |
| Process supervision and protocols | [watchdog](../crates/watchdog/README.md), [runtime-log](../crates/runtime-log/README.md), [workspace-protocol](../crates/workspace-protocol/README.md), [shell-control](../crates/shell-control/README.md) |

## Release and measurement records

- [V2.1.0 release notes](packaging/releases/v2.1.0.md) · [Changelog](CHANGELOG.md)
- [Linux performance measurements, 2026-11-17](performance-linux-2026-11-17.md)
- [Linux candidate measurements, 2026-11-16](performance-linux-2026-11-16.md)
