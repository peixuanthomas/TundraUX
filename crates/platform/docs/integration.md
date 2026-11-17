# Platform paths, identity, and system integration

## Application data paths

| Purpose | Windows | macOS | Linux |
| --- | --- | --- | --- |
| Configuration | `%APPDATA%\TundraUX\config.toml` | `~/Library/Application Support/TundraUX/config.toml` | `$XDG_CONFIG_HOME/TundraUX/config.toml`, default `~/.config/TundraUX/config.toml` |
| State | `%LOCALAPPDATA%\TundraUX\state` | `~/Library/Application Support/TundraUX/state` | `$XDG_DATA_HOME/TundraUX/state`, default `~/.local/share/TundraUX/state` |
| Cache | `%LOCALAPPDATA%\TundraUX\cache` | `~/Library/Caches/TundraUX` | `$XDG_CACHE_HOME/TundraUX`, default `~/.cache/TundraUX` |
| Logs | `%LOCALAPPDATA%\TundraUX\logs` | `~/Library/Logs/TundraUX` | `$XDG_STATE_HOME/TundraUX/logs`, default `~/.local/state/TundraUX/logs` |
| Temporary files | `%TEMP%\TundraUX` | `TundraUX` under the system temporary directory | `$XDG_RUNTIME_DIR/TundraUX`; if unavailable, a private `/tmp` directory containing the UID |

Run `tundra-cli debug paths` to view path templates and resolved absolute paths.

## Startup identity

Shell reads the current logon name from the Windows logon API, the macOS account database, or Linux NSS. Personal preferences use the system username as their key. On Linux, Shell and CLI check UID/EUID and GID/EGID before accessing storage or entering fullscreen mode.

| Startup identity | Behavior |
| --- | --- |
| Normal user | Read the current UID's NSS record and use its HOME, Shell, supplementary groups, and XDG environment |
| root | Show a privilege warning; when stdin and stderr are both terminals, press lowercase `y` to continue |
| Mismatched set-ID credentials | Refuse to start |

Home opens after the first-time preferences are saved. An interrupted setup resumes on the next start. A confirmed Linux root session uses root's NSS HOME.

Personal directories are read from NSS HOME and `user-dirs.dirs`, with support for Chinese characters, spaces, and the convention of using HOME to disable a directory. The local interface can still open without XDG_RUNTIME_DIR, logind, or D-Bus; the affected system features report their unavailability separately.

## System services and authorization

Linux management, account, and power requests use fixed interfaces:

- Management writes use Shell's private authorization connection; the interface does not accept arbitrary commands.
- Accounts use AccountsService or fixed system tools; power operations use logind.
- Other callers can handle polkit through `pkttyagent` bound to a process and its start time.
- Normal users changing their own profile or password use the existing system authorization and `passwd` flows.
- Cancellation, timeout, and disconnection do not automatically replay writes.

D-Bus operations distinguish permission denial, canceled authorization, service unavailability, busy state, network errors, disconnection, and unknown results. See [Authorization and helper connections](authorization.md) for detailed connection rules, and [Portable updates](../../app/docs/update.md) for application updates.

## Desktop and file integration

| Feature | Windows | Linux |
| --- | --- | --- |
| Open a file or URI with the default application | Native interfaces | `xdg-open`, executed in the background |
| Launcher | Native programs and shortcuts | ELF, AppImage, scripts, and validated `.desktop` files |
| Clipboard | Native backend | Wayland data-control or X11/XWayland |
| Local volumes and trash | Native backend | mountinfo/statvfs/sysfs and Freedesktop Trash |
| Critical errors | Platform alerts and logs | Desktop notifications, watchdog reports, and stderr |
| Reboot/shutdown | Native APIs | logind + polkit |

The volume list includes local fixed disks and mounted removable filesystems, filtering out network and pseudo filesystems. Btrfs subvolumes are traced back to their actual block devices; labels show the device name and mount path.

Trash operations use the current user's permissions and check private directories and symbolic links. Ordinary text and image files follow the file-opening configuration even if they have the executable bit set; ELF, scripts, AppImage, EXE, and `.desktop` files go to Launcher.

Run `tundra-cli doctor` to check the terminal, `/proc`, `/sys`, management commands, D-Bus, polkit, and desktop integration. See [Linux runtime notes](../../../docs/packaging/linux/README-LINUX.txt) for dependencies.

## Migrating from Windows to Linux

1. Close the application on both systems, run `tundra-cli debug paths` on Windows, and back up configuration and state.
2. Run the same command on Linux and copy the backups to the displayed config and state paths respectively, keeping the original backups.
3. Global settings can be reused. Set up personal preferences again when moving from a pre-2.0 installation.
4. Windows absolute paths need to be selected again; Launcher and recent files initially show Missing.
