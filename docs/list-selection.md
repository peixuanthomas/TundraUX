# List multi-selection

Multi-selection applies where one operation can act on several objects at once. Lists used only to view details, enter parameters one at a time, or configure individual items stay single-selection.

## Common operations

Packages, services, processes, scheduled tasks, and file management provide Select Multiple / Finish Selection controls, checkboxes, and a selection count.

- Tapping a row toggles its checkbox; arrow keys move the current position; Enter or Space toggles the current row.
- Leaving selection mode clears the batch and restores single selection of the current row. Batch operations refuse an empty selection.
- Sorting and refreshing keep targets that still exist; changing filters or query criteria clears the selection.
- Confirmation and authorization dialogs list every target, and background processing checks the objects again before running.
- File management and management lists share the same gestures: Ctrl/Cmd+click toggles one record, Shift+click selects a range from a stable anchor, and Shift+Enter toggles the current row. Arrow navigation preserves the selection; an ordinary click restores single selection.
- Mouse and touch gestures are canceled after a drag-out, scroll, focus loss, or resize, so releasing cannot act on a different target.

Package selections are keyed by package ID. Install, remove, and upgrade are allowed only when every selected package supports that operation. APT, DNF, and pacman use batch commands; pacman install and upgrade also perform a full system upgrade. Homebrew groups by Formula/Cask; WinGet processes packages one at a time using exact IDs and versions.

Services support batch start, stop, restart, reload, enable, and disable. Processes support batch suspend, resume, terminate, and set-priority, with process identity checked before execution. Scheduled tasks support batch pause, resume, and run-now; changes from the same cron source are saved together.

## Selection behavior by page

| Page or list | Selection | Notes |
| --- | --- | --- |
| Packages: Installed, Updates, Search | Multiple | Batch installation, removal, and upgrades |
| Packages: sources, configuration conflicts, check results | Single | Inspect, compare, or repair individually |
| File Explorer: ordinary directories and Trash | Multiple | Copy, cut, delete, restore, and change permissions |
| File Explorer: quick access, history, and options | Single | Open one directory or change one option at a time |
| Service management | Multiple | Log viewing and configuration editing remain individual |
| Process management | Multiple | Send signals or set priorities in batches |
| Scheduled tasks | Multiple | Creation, editing, and time previews remain individual |
| Firewall rules and listening ports | Single | Review rules individually; ports are primarily for inspection |
| Linux users and groups | Single | Passwords, membership, ownership, and deletion concern one account |
| Networks, disks, SSH, system settings | Single | Each configuration uses separate parameters and checks |
| Configuration files and configuration backups | Single | Edit or restore one configuration at a time |
| Launcher applications and shortcuts | Single | Opening, ordering, and editing concern one item |
| Settings categories, application lists, and field options | Single | Checkboxes can still be toggled individually |
| Logs, log files, and incident reports | Single | Open details, follow, and inspect |
| Diagnostic checks and repair previews | Single | Supports repairing the current item or all items |
| System status cards and selection dialogs | Single | Inspect data and edit cards individually |
| Clock alarms and timers | Single | Management dialogs handle one item at a time |
| Editor documents and menus | Single | Text selections and Vim Visual mode follow their own rules |
| Initial setup and timezone map | Single | Select one option or input field at a time |
| Home, Help, notifications, Command Line, AA, weather | Single | Primarily entry points, instructions, messages, or the current task |

When adding a batch operation, make the backend accept and validate multiple targets first, then wire up page selection, confirmation, and authorization. Each page's F1 help lists its shortcuts.
