# UI design and acceptance requirements

This is the shared design and acceptance contract for the whole interface: startup and first-time setup, Home, Launcher, Settings, system management, files, editor and Vim, scripts, Command Line, Tasks, clock, weather, dialogs, menus, and AutoAdmin (AA). Apply it in stages across shared foundations, global bars, workspaces, tools, and motion. A page is done only when its rendering, input, automation, and tests agree.

## Visual language and density

The default dark appearance uses Glacier Night blue surfaces and one accent. Layout and control hierarchy follow the OpenCode terminal interface. Theme overrides and explicit saved appearance choices stay supported.

| Token | Default | Use |
| --- | --- | --- |
| Canvas | `#071116` | Full-frame background |
| Surface | `#0D1B22` | Grouped content |
| Raised surface | `#13262F` | Raised controls, sidebars, and overlays |
| Text | `#E6F1F4` | Normal readable content |
| Muted text | `#8EA7B0` | Secondary information |
| Accent | `#63D3E5` | Focus and primary actions |
| Border | `#29434E` | Inputs and necessary overlay boundaries |

- Each application window has one complete outer frame with its own title and controls. Separate the page's internal groups, lists, tables, and action areas with headings, whitespace, alignment, or a background change; do not nest decorative frames.
- Home and Launcher use the same catalog, localized names, artwork, and launch actions. Launcher lists every built-in and user-added app available to the current platform and account. A shortcut can be removed from Home and added again from Launcher. Keep the Launcher shortcut on Home. Center each icon above its label. In organize mode, prefix each draggable label with a one-cell ASCII `#`. The whole tile accepts a drag and the insertion line marks where the tile will land.
- A modal, menu, or input may use one boundary. Temporary messages use the shared surface and the matching success, warning, or danger color.
- Dialogs share three-column side padding, a title and top-right close action, a readable body, and a raised footer when actions exist. Compact windows reduce vertical gaps before hiding required content. Dim covered content without moving it; toasts stay in the upper-right and do not take focus.
- Ordinary buttons use plain action text without `[]` on a visible control surface, distinct from nearby static text. Primary actions use an accent fill; destructive actions use the danger color. Selection and the current item use background emphasis independently of keyboard focus.
- Navigation entries and table rows each occupy one terminal row. Use one row between groups, two columns between adjacent groups, and one cell between grid items. Keep a whole row clickable, including its padding, for mouse and touch.
- The header and bottom window bar each occupy one row. The title at the far left opens the power menu from any page. The menu centers its action column horizontally and includes Tasks. The status text fills the space between the title and the screen keyboard, Help, clock, and contextual Back controls. Click the status text to open notifications. The default centers the status and enables Help, clock, and Back. At 108 × 20, the application window occupies 108 × 18 cells and its frame leaves 106 × 16 content cells.
- Use the same spacing, typography, and control states across pages. Text must stay readable with truecolor or ANSI color, and with Chinese, English, combining marks, and wide characters.

New defaults apply when a preference is absent. Keep explicit saved border, color, theme, animation speed, and timing preferences; do not overwrite them just because the defaults changed.

### OpenCode source reference

The implementation reference is `anomalyco/opencode` at commit `5b131f5995a9f7fad458c59a7a5e39686237beb8`, inspected on 2026-11-13. Use its terminal components, not its desktop/web CSS:

- [Dialog, header, layout, buttons, and footer](https://github.com/anomalyco/opencode/blob/5b131f5995a9f7fad458c59a7a5e39686237beb8/packages/tui/src/ui/dialog.tsx): a single modal surface, padded title and body, distinct footer actions, and an obscured background.
- [Selection dialog](https://github.com/anomalyco/opencode/blob/5b131f5995a9f7fad458c59a7a5e39686237beb8/packages/tui/src/ui/dialog-select.tsx): full-row interaction, grouped options, muted supporting text, and separate active/current markers.
- [Session sidebar](https://github.com/anomalyco/opencode/blob/5b131f5995a9f7fad458c59a7a5e39686237beb8/packages/tui/src/routes/session/sidebar.tsx): a distinct surface with shared padding and spacing between groups.
- [Spinner](https://github.com/anomalyco/opencode/blob/5b131f5995a9f7fad458c59a7a5e39686237beb8/packages/tui/src/component/spinner.tsx), [shimmer text](https://github.com/anomalyco/opencode/blob/5b131f5995a9f7fad458c59a7a5e39686237beb8/packages/tui/src/component/shimmer-text.tsx), and [animation scheduling](https://github.com/anomalyco/opencode/blob/5b131f5995a9f7fad458c59a7a5e39686237beb8/packages/tui/src/ui/animation.ts): elapsed-time feedback, motion-disabled fallbacks, and scheduling only while motion stays active.

OpenCode's dialog mounts directly; it does not supply TundraUX's page/dialog entrance transitions. The configurable phases below remain TundraUX behavior. When adapting these visual patterns, keep TundraUX's release-inside-target activation, safety confirmations, localization, terminal compatibility, and application workflows.

## Text and action entry points

- Use short action labels. File Explorer keeps shortcuts in F1 help so its common actions fit at the minimum width.
- Common operations use visible buttons and options. Keep search, filters, refresh, and path navigation near the top, and page actions at the bottom right of the content area. Wrap actions at their full text width in narrow windows; there is no fixed button count or three-row limit.
- Reduce decoration without losing features: multi-selection, sorting, search, expert areas, help, return paths, and task management must stay reachable.
- Keep service logs, file copy/cut/paste/rename, editor save/undo/find, and scheduled-task run-now directly visible. Home shows supported system tasks with compact readable entries in small windows.
- Complex pages provide an expert area that stays expanded when the selected object changes. Less frequent operations may also use menu shortcuts.
- The notification panel shows the full status captured on opening, then warning, error, and critical reports in one column, newest first. Cards show severity, source, original text, and a top-right ×. Keep the status and all cards readable by scrolling at 108 × 20. Keep Clear all and Close reachable. Tab and Shift+Tab visit card actions, arrows and Page Up/Down scroll, and Enter activates the focused action.
- Clicking the status again, clicking outside, or pressing Esc closes the panel and restores focus. Confirmation dialogs and incoming critical dialogs take priority. A card for a previous desktop crash or unexpected exit appears silently and opens that event’s read-only report with the existing diagnostics permission.
- Empty states, details, and status bars provide useful information. Errors explain the specific cause and next step. Update Chinese and English together and check text width and clickable areas.
- First save and Save As provide directory browsing, a persistent filename field, Save, and Cancel in the same picker.
- Delete in Trash affects the selected items. Empty Trash has its own explicit action. Irreversible actions, clock management, and manual AA approval default to Cancel or Deny.
- Keep the primary action at the far right. Selection-dependent actions stay visible but disabled when nothing is selected.
- Action models declare destructive risk independently of confirmation or authorization. Removing, uninstalling, and force-stopping use the danger color; viewing, starting, and resuming do not become dangerous just because they need confirmation.
- Center More Actions in the content area and size it to its options. Allow scrolling and button wrapping in small windows, keeping the current option visible.

## Buttons and input methods

The shared Button, Theme, and RenderContext own control states. Pages pass focused, selected, pressed, and disabled state instead of drawing their own equivalents.

| State | Appearance and behavior |
| --- | --- |
| Normal | Readable text on a neutral background |
| Mouse hover | Accent feedback without changing layout |
| Mouse pressed | Stronger feedback until release inside the same target. Danger buttons retain their icon and use different shades of red for normal, hover, and press in both truecolor and ANSI. |
| Keyboard focus | Visible accent indicator independent of current-item and selection backgrounds |
| Disabled | `TundraTheme::disabled_style()`; ignore activation |
| Click completed | Clear pressed and hover feedback until actual pointer movement |

- Show keyboard focus initially. Valid keyboard navigation moves focus immediately; actual mouse movement, taps, drags, or scrolling hide the indicator while keeping logical focus.
- A stationary pointer must not highlight another button or reclaim emphasis after keyboard input. Repeated reports of the same coordinates do not switch input methods.
- Activate a button only after pressing and releasing within the same target. Dragging outside, losing focus, resizing, or changing pages cancels the pending action.
- A double click needs a released left click followed within 500ms by a second press on the same file, icon, or control. Allow movement of one terminal cell on each axis within that target. A change of page or window, scrolling, dragging, keyboard input, focus loss, and resizing clear the pair.
- Only the topmost dialog receives hover, press, text, or keyboard input. Covered pages keep their state. Toasts do not take keyboard focus, but their painted area blocks pointer activation of covered controls until the toast disappears.
- Hide keyboard emphasis on window focus loss while keeping logical focus. Current-page and record-selection emphasis stays independent of the input method.
- Drawing a native image keeps the terminal cursor hidden at the image position, including during partial redraws.
- Selected text fields use accent feedback and a blinking underscore cursor at the actual position. Up/Down switches fields; Left/Right moves the cursor. Mask passwords and scroll long text with the cursor.
- Keyboard, mouse, and touch trigger the same transition behavior. Focus, activation, text input, scrolling, and cancellation take effect immediately, including during animation.

## Motion and scheduling

Use restrained transitions to explain state changes without delaying work. Layout and content must be usable from the first frame.

| Transition | Default duration | Presentation |
| --- | --- | --- |
| Page | 160ms | Fixed final layout; background and color transition |
| Dialog | 180ms | Fixed final layout; background and color transition |
| Menu | 120ms | Fixed final layout; background and color transition |
| Details | 180ms | Clipped expansion or collapse |
| On-screen keyboard | 160ms | Clipped expansion or collapse from the bottom |
| Progress graphic | 200ms | Interpolation toward the real value, without overshoot |
| Busy indicator | 80ms per step | One terminal character |
| Loading label | 1200ms per sweep | Muted text with OpenCode's moving highlight |

- Page, dialog, and menu bodies are readable on the first frame. Do not show an empty moving outline or wait for the transition to reveal text. Opening and closing keep the intended final layout and blend only the presentation.
- A new AppHost starts with its window frame and canvas. Show the localized Starting indicator only if it still lacks usable content after 100ms, and replace it as soon as the real frame arrives. Reduced motion keeps the indicator static. Record whether the indicator was actually output; hiding it does not prove startup completed.
- Split, cancellation, full screen, restoration, and resize keep each pane's last successfully output application frame until its replacement matches the current layout and size. Clip the retained text and images together from the top left and fill added space with the canvas. Disable stale pointer targets, automation actions, and cursors while keeping window controls and native keyboard routing. Advance the retained-frame cache only after successful terminal output.
- Details expand rightward in wide windows and vertically in narrow windows. The list, details, and keyboard use the same current geometry for painting, clipping, PTY sizing, automation, and hit testing. Do not insert blank intermediate frames.
- The real progress value and its numeric label update immediately. Only the graphic interpolates; changing targets or reversing direction starts from the displayed value and never overshoots.
- New navigation, reverse actions, repeated switching, input-method changes, and resize interrupt or retarget the active transition. Do not queue animations or need an extra key or click to finish one.
- Reduced motion renders the terminal state directly, including details, keyboard, progress, and busy feedback. No hidden animation continues afterward.
- Active UI transitions schedule frames at 60 FPS. Idle UI has no animation timer. Weather and continuous rainbow effects keep their separate 30 FPS policy. Cursor blink, clocks, and real data refresh keep their own deadlines.
- Input, IPC readiness, completed background work, and PTY output wake the host loop. Frame encoding and decoding do not run on the terminal drawing thread. Keep real service, watchdog, clock, and animation deadlines; do not replace independent AppHosts or the watchdog with prewarmed application processes.
- A visible loading label refreshes at 20Hz in truecolor. ANSI uses only the 80ms spinner cadence without RGB interpolation; reduced motion uses a static ellipsis and schedules neither effect. Respect animation speed and terminal-column width for Chinese and combining text.
- Slow output advances by elapsed time and skips missed frames. Never replay accumulated animation frames after the terminal stalls.
- Motion curves, durations, clipping, spacing, and pane geometry belong in `crates/ui`; Shell owns input routing, navigation, dialog stacking, and transition lifecycle.

Settings keeps animation speed, reduced motion, and all nine phase values. The three default phase durations are:

| Kind | Acceleration | Constant speed | Deceleration |
| --- | --- | --- | --- |
| Page | 40ms | 80ms | 40ms |
| Dialog | 45ms | 90ms | 45ms |
| Menu | 30ms | 60ms | 30ms |

Use one three-column form with direct input and Tab navigation. Each phase accepts 1–2000ms. Validate all values before saving them together. Reset changes the draft; Save commits 100% speed and the default phases after reset, while Cancel or Esc discards the draft and restores focus. Keep Reset, Save, and Cancel reachable at 108 × 20, including with reduced motion enabled.

## Pages, navigation, and global bars

- `ScreenCompositor` draws the header and bottom window bar. Pages draw inside `ShellFrameLayout.main`. The title remains available above page overlays.
- Create an independent instance when opening a subpage or explicitly starting a new application session. An ordinary application launch restores an existing window when available. Check instance IDs for state, task results, and focus restoration so late results cannot enter a reopened page.
- Esc and the header Back control take the same single step in the focused window. Cancel a dialog, menu, drag or edit first, then return to the previous location or page. An app stays open at its starting page. Use the matching Cancel, Previous location or page, or Home label and hide Back when there is no return step. Command Line, native Vim, and embedded terminals handle physical Esc. On idle Home, Esc opens the power menu. Esc dismisses that menu.
- Every visible window, including a single or maximized window, has a complete frame. Its top edge holds the title, Minimize, Split, and Close buttons. Full screen is an action inside Split. Clicking the frame focuses that window. Buttons act on their own window on release, even if another window had focus before.
- The bottom window bar lists running apps. Click a label to focus or restore it. Click the separate × button to request a normal close. Scroll buttons keep overflow entries reachable. Right-click a taskbar entry for Open, New session, and Move. Delete appears only for user-added Launcher entries and removes the entry after the selected window closes successfully. Launcher icons use the same menu labels for the applicable actions. The split button offers incremental left/right, top/bottom, and full screen. The original app stays left/top. The new pane opens a searchable selector for minimized windows and new app sessions, with a scrollbar for overflowing results. The mouse wheel scrolls those results without changing the selected app. Cancel removes only the pending split. Unavailable actions stay visible and disabled.
- The workspace prefix is Ctrl+W followed by v/s for splitting, h/j/k/l or arrows for focus, o for full screen, b for minimize, w for windows, q for normal close, and uppercase X for immediate kill. Outside native editing areas, Ctrl+W twice forwards one literal Ctrl+W; Esc cancels a pending prefix. Editor canvases, Vim, and terminals keep native Ctrl+W and use window buttons. Topmost dialogs receive input first; the old Ctrl+B prefix is removed. Holding a key must not repeat splits or dismiss several layers.
- A normal close keeps the existing dirty-document saving and warns before interrupting running work, with Cancel selected by default. After two seconds without a close response, offer waiting or immediate kill. Immediate kill is a distinct danger action with no second confirmation; show the app as stopping until the guardian confirms it has exited.
- Keep the terminal minimum at 108 columns × 20 rows. All living apps, including background apps, contribute to the component-wise maximum content minimum. Default content minimum is 106 × 16. Reserve the header and bottom window row, a one-cell frame on all four sides of every window, and one-cell gaps between windows. Use a bounded binary split tree with each axis at most once per path, three-window T layouts, and at most four panes in a 2×2 grid. Default thresholds are 217×20 left/right, 108×39 top/bottom, and 217×39 for four panes.
- A custom PTY app may report `TUNDRA_MIN_SIZE` through OSC 1337 SetUserVar, using a Base64-encoded positive `colsxrows`. Recompute capacity when a valid report changes. Invalid reports keep the previous minimum, empty reports withdraw it, and a confirmed process exit removes it. If the global minimum cannot fit one content area, keep window management and kill actions available while showing the size requirement.
- When content cannot fit, keep the focused window's frame controls and the bottom window bar, but suspend its business viewport. If a normal close needs a save or task confirmation that cannot fit, explain that the terminal must be enlarged and offer waiting or force kill; never silently discard work.
- Size-driven overflow keeps the focused app and a stable window order, and restores automatically hidden panes when space grows. Manually backgrounded or replaced panes stay in the background. Full screen clears the old topology and minimizes the other panes. Launches from Home or the window bar use the focused slot.
- When an app opens a new window, use the first unfocused pane in layout order if multiple panes are visible. In a single pane, try left/right, then top/bottom, then temporarily replace the focused window. When the new window closes or minimizes, restore any pane it covered and return focus to the opener. Editor Open and Save As use a separate File Explorer window that closes on selection or cancellation.
- Vim displays the full native terminal. Ordinary editor menus and status lines must not cover its mode, selection, command line, or errors. Geometry changes update rendering, PTY size, and pointer translation together.
- Keep automation control IDs and the external-control protocol compatible. Capture actual visible controls, disabled states, text, focus, clipping, and coordinates; preserve password masking. Interaction must never target a stale location from an earlier layout or covered layer.
- Accepted application inputs keep their order. When a window's bounded queue is full, reject new business input with localized feedback and a retryable CLI `busy` response; other windows, window management, cancellation, and close stay available. Cancel a captured press or drag when admission fails.
- For a visible window, input confirmation needs a frame for the current viewport carrying the processed input serial. A separate acknowledgement must not make retained content actionable. Hidden, stopped, closing, and unresponsive windows must not block unrelated snapshots indefinitely. Decode image references in wire order before keeping only the latest frame, including frames whose old viewport will not be displayed.
- Shared settings, appearance, and Clock writes complete in the background. Apply success only after persistence, keep newer or canceled drafts, and leave errors visible. Check the original configuration/profile and authorization revisions so a delayed save cannot cross Shell sessions or overwrite a newer edit.

## Screen keyboard, forms, and selection dialogs

- Object and option selection dialogs always have a top search input, focused initially. Filter names, descriptions, and command keywords case-insensitively while keeping original IDs and disabled states. Up/Down selects results, Enter confirms, and Esc closes. Empty results cannot activate a previous selection. Keep specialized previews such as theme colors and the timezone map. Compact right-click and More action menus do not add search.
- Place the screen-keyboard entry directly after the status area. The expanded keyboard clips the page above it. In an app window, the last key row fits inside the content area, above the window frame and the bottom window bar. The header follows the user's display settings. AA adjusts only the topmost dialog.
- Keep forms and action buttons above the keyboard. If the terminal cannot fit them, ask the user to enlarge it instead of covering required fields.
- Place Paste at the left end of the keyboard's bottom row and Hide at the right. Physical-key feedback and mouse-latched modifiers use separate state; clear both on focus loss or resize.
- Keyboard keys use neutral boundaries and text contrast of at least 4.5:1. Pressed or latched keys use accent fill.
- Use the shared `FormDialog`. Enter advances fields and submits from the last field; Space or a tap toggles switches; Ctrl+Enter submits directly; F10 activates the default confirmation.
- Keep the current field, Confirm, and Cancel reachable while scrolling long forms. Passwords stay masked in both rendering and snapshots.
- Launcher opens with search focused. Match the active language pack’s app names and descriptions and prefer an exact name. Enter opens the selected result and Down moves focus to the results. Esc clears an active query, then returns to the screen that opened Launcher.
- In File Explorer, put `<` Back, `^` Parent, and `>` Forward on the path row. Esc goes back in visit history. Tab and Shift+Tab visit enabled controls in screen order. The sidebar and file list each take one stop. Arrows select within a region and Enter opens the selection. Changing focus preserves the directory and current files. Include the Save As filename, Save and Cancel in that order.
- At 108 × 20, Explorer shows Open, New, Copy, Cut, Paste, Rename, Undo, Select, Delete, and More from left to right on one row with short localized labels and ASCII icons. More contains sort, properties and advanced options. Reserve the footer for multi-selection counts, progress, or errors.
- Packages uses an Installed, Updates, Search sidebar. Settings shares its layout. Tab and Shift+Tab move between controls. Arrows change categories in the focused sidebar. Packages keeps F7/F8/F6 for Installed/Updates/Search.
- In application settings, Left/Right moves between the application list and its controls. Enter/Space activates controls; arrow navigation does not save options.
- Numeric options support direct entry or minus/plus controls. AA policy and screen timeout use selection dialogs.
- First-time setup offers F9 to finish with the default appearance. Home shows the current system username, and Users is available on Linux.

## Lists and sorting

Packages, services, processes, scheduled tasks, and files share multi-selection behavior. See [list selection](list-selection.md) for scope.

- Ctrl/Cmd+click toggles individual records; Shift+click selects a range from a stable anchor; Shift+Enter toggles the current row. Explicit multi-selection mode also supports touch checkboxes.
- Clicking a column header sorts by it; clicking again reverses direction. Show the active sort direction.
- Compare text case-insensitively and counts, percentages, capacities, and rates numerically. Files keep directories first.
- Sort complete records, preserve selected objects, and keep sorting after refresh.
- Ctrl+F6 changes sort columns; Ctrl+Shift+F6 reverses direction. Files and Launcher keep their own shortcuts.
- Manually sorting logs pauses following. Returning to the latest records restores reverse chronological order.

## Acceptance checks

Run the matrix at 108 × 20, 120 × 36, and 180 × 50, in Chinese and English, with truecolor and ANSI color. Include wide and combining characters in text, paths, fields, and labels. Complete each workflow separately with keyboard, mouse, and touch events; touch checks use presses, releases, and drags without hover.

| Workflow | Keyboard | Mouse | Touchscreen |
| --- | --- | --- | --- |
| Enter, act, and return | Tab/arrows, Enter/Space, shortcuts | Visible actions, full rows, Back | Tap the same targets |
| Long lists and forms | Arrows, page keys, Home/End | Wheel, scrollbar dragging | Scrollbar dragging |
| Text editing and submission | Typing, deletion, focus, submit | On-screen tools and Submit/Cancel | On-screen tools and tappable actions |
| Menus and destructive actions | Open, cancel, confirm, restore focus | Visible entry, release activation, cancellation | No hover or right-click dependency |
| Focus loss, resize, navigation | Preserve logical focus | Cancel stale presses and drags | Cancel stale gestures and coordinates |

Verify these cases on shared controls and representative complete pages:

1. The minimum window keeps readable content and essential actions inside its complete outer frame. Interior groups do not add decorative frames; bars and rows use the specified density.
2. Hover, press, release, exit, disabled state, current item, multi-selection, and keyboard focus stay distinct. A resting mouse does not reclaim keyboard emphasis.
3. A dialog intercepts every input method; closing it restores the correct page and focus. Drag-out and focus loss cancel pending actions.
4. Both keyboard and pointer transitions show readable first-frame content and respond during continuous switching, reversal, resize, and nested modal changes.
5. Details and the keyboard clip without blank frames. Vim and child-terminal painting, PTY sizes, and pointer targets agree throughout geometry changes.
6. Reduced motion reaches the final state immediately. Active transitions stop scheduling frames at completion; long slow output does not cause frame catch-up.
7. Semantic PTY checks use control IDs, snapshots, or business content. Do not depend on decorative border glyphs, the old `╭Status` title, or fixed screen coordinates.
8. Release measurements at 120 × 36 target draw P95 ≤ 16.7ms and input-to-output P95 ≤ 50ms. Report the actual measured metrics, sample counts, build, and environment. PTY output or control-round-trip timing alone is not a draw-time measurement.
9. With normal animation settings, common built-in application activation to the first real business frame targets P95 ≤ 100ms; single-window and 217 × 39 two-window/four-window interaction feedback targets P95 ≤ 50ms. Use at least 50 independent new AppHosts per application and 100 samples per interaction type. Report first opening, repeated new processes, same-process restoration, and child-terminal readiness separately, with identical baseline/final environments and preserved raw PTY evidence.
10. Rapid split/cancel/full-screen/restore/resize must keep already displayed content without accepting stale pointer locations. Cover late frames, image-reference chains across replaced frames, input fences, full queues, background panes, close/unresponsive states, and concurrent shared saves. Confirm static AppHosts stop redraws while actual task completion and terminal output stay observable.

See [Testing guide](testing.md) for commands and the difference between automated, real-terminal, and device acceptance checks.
