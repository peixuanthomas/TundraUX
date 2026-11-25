# ui

`ui` converts view models into Ratatui layouts, frames, and control geometry. Shell owns routing, persistence, and the host terminal. Components may hold local interaction state and return actions; renderers read state without executing file or system operations.

## Module structure

| Location | Contents |
| --- | --- |
| [`foundation/`](src/foundation/) | `input` represents normalized events; `command` defines shortcut scopes and conflicts; `focus` handles traversal and modal traps; `hit_test` orders pointer targets; `intent` describes the resulting UI action. |
| [`components/`](src/components/) | Reusable controls; see the component groups below. |
| [`screens/`](src/screens/) | Page-specific view models, layouts, hit targets, and rendering; see the screen groups below. |
| [`theme/`](src/theme/) | Palette tokens, terminal color capabilities, component states, syntax colors, render context, and motion frame/schedule types |
| [`motion`](src/motion.rs), [`surface_motion`](src/surface_motion.rs), [`spring`](src/spring.rs) | Timing and interpolation, surface and panel geometry during transitions, and shared spring helpers |
| [`table_sort`](src/table_sort.rs) | Shared sort state, header geometry and drawing, and cell comparison |
| [`automation`](src/automation.rs) | Captures control trees, bounds, sensitive masks, and visible text from actual rendered frames. |
| [`workspace/`](src/workspace/) | `tree` stores splits; `layout` calculates pane geometry; `chrome`, `panel`, `split_menu`, and `taskbar` draw global window controls. `frame` translates and clips AppHost frame data. |
| [`editor_media`](src/editor_media.rs), [`terminal_images`](src/terminal_images.rs) | Detect and prepare graphics and fallbacks; crop, cache, and place images from the child terminal. |
| [`assets/`](src/assets/) | Accesses the shared application icon catalog and setup preview assets. See [asset sources](assets/README.md) for the included icons and timezone map. |
| [`ui_test`](src/ui_test.rs), [`style_preview`](src/style_preview.rs), [`rainbow_text_preview`](src/rainbow_text_preview.rs) | Provide demo data and renderers for interactive previews owned by Shell. |
| [`build.rs`](build.rs) | Copies the active checkout's resources to the build output for renderers and previews. |

### Screen modules

| Modules under [`src/screens/`](src/screens/) | UI responsibility |
| --- | --- |
| `shell/`, `context`, `home/`, `auth/` | Shared frame and header, render context, Home catalog, and first-run setup |
| `explorer/`, `launcher`, `launcher/edit` | Browser, save picker, grids and lists, entry editor, and run-settings dialogs |
| `editor/`, `script/`, `command_line` | Document canvas and controls, Vim viewport or installation progress, debugger panes, and terminal area |
| `management`, `management_workspace`, `management_touch`, `auto_admin`, `auto_admin_preview` | Management lists, details, form/actions layouts and touch affordances, plus approval and interactive task surfaces |
| `settings`, `timezone_map`, `clock/`, `system_status/` | Configuration controls, update and timezone screens, clock forms, and dashboard widgets |
| `logs/`, `diagnostics/`, `notifications/`, `help`, `panic`, `screen_keyboard` | Log browsing, check and repair results, notifications, localized F1 shortcuts, fatal-error display, and virtual keyboard geometry |

Screens with separate `model.rs`, `layout.rs`, and `render.rs` keep the data, rectangles, and drawing code together. Reuse the layout result wherever pointer targets, focus, or an embedded viewport need to match the display.

The notification panel sits under the header. Its full status and single column of cards share a scrollable area. Dismiss, open-log, clear-all, and close buttons use the same rectangles for drawing, keyboard navigation, mouse input, and automation.

## Workspace frames

Pages render only the content in `ShellFrameLayout.main` and their own overlays. Shell handles the header, window bar, Toast, and modal dialogs, using the same layout for rendering and clicks.

The title at the left opens the power menu, which includes Tasks. Its list of actions is centered in the dialog. The status text can be left-aligned, centered, or right-aligned. Screen keyboard, Help, clock and Back follow it. When **Show Back button** is on, the right edge shows Esc and its current action. A dimmed Esc marks a tool with no return step.

Each app window has a complete frame with **Minimize**, **Split**, and **Close** controls. The Split menu offers left/right, top/bottom, and full screen, with unavailable actions disabled. Full screen retains only the chosen pane and minimizes the others. Window controls, rendering, automation, and hit testing share the same geometry.

The window bar uses the full bottom row. Click an entry's title to restore its window or its × button to close it. Right-click opens the window menu. Arrows scroll the bar when its titles cannot all fit.

`WorkspaceTree` stores incremental splits, allowing each axis once per path and at most four panes in a 2×2 grid. T layouts preserve the untouched pane. Size-driven hiding projects this topology without discarding it; growing the terminal restores the hidden panes. At the 108×20 minimum, a single app has 106×16 content cells. Complete frames and one-cell gaps require 217×20 for left/right, 108×39 for top/bottom, and 217×39 for four panes.

All live windows contribute to the minimum content size, including those in the background. If the workspace cannot fit, the focused window keeps its controls and shows a resize notice. The advanced window panel opens with Ctrl+W then W and can close or force-kill windows while the workspace is too small. F1 help lists the keyboard controls, including Ctrl+Q to close the focused window.

App-host frames carry content in pane-local coordinates. Composition translates and clips cells, controls, and sensitive areas together. Only the focused pane keeps the existing automation IDs. The composed snapshot masks sensitive text in every visible pane. AppHost attaches frame sequence numbers, input tracing serials, confirmed input fences, and first-frame startup timings after UI capture.

AppHosts without terminal image support load complete ASCII catalogs and fonts while omitting unused PNG bytes. Hosts with image support retain the full asset set, including when their current appearance uses ASCII icons.

Shell can compose a retained app frame at a new pane size, clipping from the top left and filling newly exposed space with the canvas color. Images keep the same clipping. Retained controls are non-actionable and the old cursor is hidden. Initial window startup uses a blank canvas for 100ms before showing the localized busy indicator; reduced motion keeps that indicator static.

## Shared controls

| Modules under [`src/components/`](src/components/) | Responsibility |
| --- | --- |
| `foundation`, `surface`, `button`, `tabs`, `nav_rail`, `page_action_bar` | Common geometry and visual state, background surfaces, buttons, navigation, and adaptive action rows |
| `dialog`, `form_dialog`, `context_menu`, `picker`, `text_input` | Shared modal layout, form events, menus, value selection, and text editing or masking |
| `data_table`, `list`, `selection_list`, `command_palette`, `sidebar_workspace`, `scrollbar`, `grid_reorder` | Table/list layout and selection, searchable options, sidebar composition, scrolling, and grid insertion |
| `metric_card`, `progress_gauge`, `update_activity`, `command_status`, `toast` | Present metrics, execution or download progress, command results, and transient feedback |
| `empty_state`, `skeleton`, `busy_indicator`, `rainbow_text`, `big_text` | Empty/loading states, motion and color affordances, and larger text for supported terminals |

`ProgressGauge` paints the fill and track with terminal cell backgrounds. The fill rounds to the nearest column, and the centered label uses contrast colors for the background beneath each character.

### Forms

`DialogLayout` gives shared and page-specific dialogs the same padded header, top-right close action, body, and raised footer. Its geometry also drives hit testing and automation. Shell dims covered content; only the topmost dialog receives input. Selection lists keep a fixed marker column and a full-row background, while supporting text stays muted.

`components::FormDialog` handles form-dialog layout, input, options, switches, scrolling, and buttons in one place. The caller supplies fields and buttons; the component returns a submit, cancel, or custom action. The caller performs application checks and saving.

```rust
let mut form = FormDialog::new("create", title, vec![
    FormField::text("name", name_label, ""),
    FormField::toggle("keep", keep_label, false),
]);
form.buttons = vec![
    FormButton::new("save", save_label, FormAction::Submit),
    FormButton::new("cancel", cancel_label, FormAction::Cancel),
];
let event = form.handle_event(input, area);
form.render_frame(frame, area, context);
```

Enter moves to the next field and submits from the last field; Space or a tap toggles a switch; Ctrl+Enter submits directly; F10 activates the default submit button. When rebuilding a form, use `interaction_state()` and `with_interaction()` to preserve scroll and drag positions. `ManagementForm` is a compatibility name.

### Lists and page layouts

`SelectionList` and `SelectionListItem` reuse the command palette with configurable titles, placeholders, grouped rows, and stable option IDs. `selection_matches` and `selection_search_areas` provide the common filtering and header geometry for specialized Settings, form-choice, and dashboard pickers. Search opens focused above the list; disabled and empty results cannot activate hidden values. Theme previews and the timezone map remain available. Action menus do not add search.

- `DataTable` provides a separate header band, full-row selection and pointer feedback, sorting, and column-width calculation.
- `SidebarWorkspaceLayout` provides a padded navigation surface, list, and detail columns, switching to top navigation and a vertical layout in narrow windows.
- `ManagementWorkspace` organizes search, details, action bars, and expert areas that stay expanded when the selected object changes.
- A successful package installation can show an Add to Launcher button beside Close in the AutoAdmin result. The dialog uses the same button areas for rendering and pointer input.
- The management details scrollbar uses the wrapped line count for both the scroll range and offset clamping.
- `PageActionBar` supports primary and danger colors, disabled states, and wrapping at the full button-text width in narrow windows, without fixed button-count or three-row limits. Log expert tools reuse the same layout without covering the list.
- Home and Launcher show a one-cell ASCII `#` beside draggable names in organize mode. The entire tile accepts the drag. Both pages share the insertion line and its geometry, including row gaps, empty tail slots, and scrollbar exclusion.
- `ManagementViewModel::selected_rows` represents all selected rows; `selected` represents the current position.
- Shared scrollbars draw a thin, muted track and a brighter thumb. The vertical thumb uses half a character cell's width, and the horizontal thumb uses a quarter of its height. `ScrollbarOrientation::hit_test` extends the pointer target by one cell on either side of a vertical bar or above and below a horizontal bar.

Selection dialogs keep this extended scrollbar hit area clear of option rows and the Close button, including the last row of long paths.

The split picker shows a scrollbar when the result rows overflow. Wheel, track and thumb input scroll the rows. Keyboard navigation keeps the selected row in view.

Inside app windows, pages use headings, backgrounds, and whitespace to separate content. Navigation entries and table rows use one line. Shared spacing is one row between groups, two columns between groups, and one cell between grid items; full rows remain clickable. A modal, menu, or input may retain one boundary.

### Page controls

- Home and Launcher tiles show the icon and application name. Keyboard focus underlines the name. Home centers its title, account, and grid within 112 columns and uses name-only tiles when the artwork would force scrolling. Launcher shows the selected custom entry's path and any status that needs attention.
- Explorer uses the shared form for archive format, output path, and masked password entry. Archive creation and extraction report progress through its file operation controls.
- Explorer puts Back, Parent, and Forward beside the path and fits ten short actions on one row at 108×20, from Open on the left to More on the right. The Delete button retains its trash icon and red feedback through hover and press on truecolor and ANSI terminals. The search row includes a keyboard- and pointer-accessible Include subfolders toggle. Recursive results show paths relative to the search directory. The sidebar and file list each take one Tab stop. Save pickers add filename, Save and Cancel to the same focus order.
- The ordinary editor keeps Save, Undo, and Find visible. Its footer shows the cursor position and filename on the buttons that act on them. Its context menu stays inside the window and preserves the selection. Rich Markdown keeps its selection-formatting menu.
- PDF previews show the fitted page and page count with previous and next controls. Both the pixels and the color-block version are prepared before a prefetched page is shown. Large Kitty images use compressed transfer when the payload shrinks.
- Office previews use the read-only Rich Markdown canvas and wrap long values inside table cells.
- Launcher opens with its search field focused. The field uses `TextInput` and shares its region between rendering and hit testing. The entry menu offers Open, New session for managed apps, Move, and Add to Home. User-added entries also expose Delete and editing actions. Right-click the blank area for launcher-wide actions.
- Settings uses Tab/Shift+Tab across categories, the operating-system entry, and controls. Arrows switch categories when that region is focused and operate content otherwise; ordinary numeric options open the shared input dialog.
- Command Line appears in the application settings list and uses the shared searchable picker for its shell setting.
- The Release update page adds a large action, download meter, and update notes when an update is available. Source updates use a scrollable Git tree with full-row commit targets and a live output panel that moves below the graph in narrow windows.
- The shared terminal minimum is 108×20, with all Home entries and the Command Line available at that size. First-time setup keeps completion with default appearance at the bottom of the visible area while its body scrolls. Home shows the system username as read-only text.

### Input and themes

`TextInput` provides a cursor, selection, scrolling, and password masking. Button hover, pressed, focused, current-item, and disabled styles are calculated centrally by `TundraTheme` and `RenderContext`. See [UI requirements](../../docs/UI-requirements.md) for keyboard, mouse, and touchscreen rules.

Text fields and forms use the host's scoped `TextClipboard` for Ctrl+C, Ctrl+X, and Ctrl+V. Cut waits for a successful write. Password fields accept paste and keep their contents out of copy and cut.

The default Glacier Night palette uses `#071116` for canvas, `#0D1B22` for surfaces, `#13262F` for raised surfaces, `#E6F1F4` for text, `#8EA7B0` for muted text, `#63D3E5` for accent, and `#29434E` for borders. Ordinary buttons have a visible control surface and plain labels. Primary actions use accent fill, while danger actions retain semantic color. Current-item and multi-selection backgrounds remain independent of keyboard focus. Explicit saved appearance settings remain supported.

Compatibility themes retain resolved surface colors and the frame's terminal color capability, so page renderers use the same layered palette and ANSI output remains free of RGB colors. `Surface`, `Button`, and `List` accept an explicit background when embedded in a sidebar, dialog, or action band.

## Animations

Shared animations include:

- `ease_in_out_cruise` and `MotionPhases`: three phase durations and a continuous velocity curve.
- Pages, dialogs, and menus keep their final layout and readable body from the first frame, using short color transitions.
- `PanelMotion`, `expand_trailing_pane`: clipped details and on-screen keyboard expansion, with shared geometry during reversal.
- Active UI transitions schedule at 60 FPS; weather and continuous rainbow effects retain their separate 30 FPS cadence.
- `RainbowText`: continuous rainbow text colored by terminal character width.
- `BusyIndicator`: OpenCode's ten-frame Braille spinner and 1200ms loading-text sweep, using its coast and smootherstep curves. Wide and combining characters are colored by grapheme and terminal column.

`ui::animate` exports `Tween`, `Spring`, `Clock`, and macros from animate 0.8, with support for Ratatui types such as `Rect`, `Color`, and `Style`:

```rust
use ui::animate::{self, Clock};
```

Callers retain animation state and advance the clock with the same `MotionFrame` for each frame. Keyboard and pointer actions share transitions and take effect immediately. New navigation, reversal, or resize interrupts active motion; reduced motion goes directly to the target. Idle UI has no animation timer, and slow output skips missed frames instead of catching up.

Defaults are 160ms for pages, 180ms for dialogs and details, and 120ms for menus. The keyboard expands over 160ms. Progress graphics interpolate for 200ms without overshoot while numeric values update immediately. Busy indicators occupy one character with an 80ms step; visible truecolor loading labels refresh at 20Hz for the sweep. ANSI keeps the spinner without RGB interpolation. Animation speed applies to both, and reduced motion uses a static ellipsis with no animation timer.

OpenCode's dialogs mount directly; the configurable page/dialog/menu transitions above remain TundraUX behavior. The inspected upstream revision, source links, and MIT attribution are documented in [UI requirements](../../docs/UI-requirements.md#opencode-source-reference) and [OpenCode reference](../../third_party/opencode/README.md).

Advanced animation settings reuse `FormDialog` for nine numeric fields in three columns, Tab navigation, range checks, and small-window scrolling. Reset changes the draft; Save validates and commits all values together, while Cancel leaves configuration unchanged. Reset, Save, and Cancel remain visible.

Default acceleration/constant-speed/deceleration phases are page `[40, 80, 40]`, dialog `[45, 90, 45]`, and menu `[30, 60, 30]` milliseconds. Explicit saved timing values and speed settings remain effective.

## Child terminals and images

`TerminalImageFragment` describes RGBA data, source cropping, content-area coordinates, pixel margins, and layers. `TerminalImageCache` clips and caches Kitty, Sixel, and iTerm2 images by viewport. An app host captures the visible image placements alongside its rasterized cells. Shell rebuilds native graphics in the pane and retires native resources when the pane moves, closes, or becomes obscured.

Home and Launcher tiles resolve the same themed ASCII artwork and image by built-in application ID. Image layout uses the terminal's measured cell size. iTerm2 packets specify the target in terminal cells so images fill the assigned area when a font measurement differs from the terminal's actual cell size. WezTerm on Windows displays each image after the cursor used to place it is hidden. App hosts send icon pixels and pane-local placement to Shell, with half-block cells for frames where an overlay covers the pane. Transparent pixels blend with the tile's current background before both representations are generated.

Text terminals, dialog occlusion, layout transitions, and transparent or overlapping images use character-block compositing. Snapshots preserve wide characters, italics, and native cursor shapes; the editor's Vim region shares rendering, PTY sizing, and mouse coordinates through `editor_vim_terminal_area`.

Inactive rich Markdown headings use the terminal's text-sizing protocol when available. App hosts send heading text, level, style, and location as structured data. Shell generates OSC 66 only when the complete heading fits in the visible pane.

Vim reserves one row for its editor-mode switch and gives the rest to the native terminal. Command Line uses the entire main region. Script output reuses Command Line terminal geometry, reserving a scrollbar column only when history is present.

Command Line and script output reuse the same selection highlight and context menu. Selection uses terminal cell positions and includes a complete wide grapheme when either of its cells is selected.

## Automation snapshots

When external control is enabled, `automation::FrameCapture` collects the control tree during actual rendering:

1. Shell starts the frame with `begin` and marks pages and overlays with `set_group`.
2. Controls record their IDs, names, regions, focus, selection, disabled status, and sensitive status using the actual layout.
3. `finish` reads the final Buffer and adds visible text not covered by controls.

Password regions are consistently masked in both controls and full-screen text; child terminals export parsed visible text. Capture only describes the screen; application operations still run through Shell's normal input flow.

## Validation

```sh
cargo test --locked -p ui
cargo test --locked -p ui --test automation
cargo test --locked -p ui --test components form_dialog::
cargo test --locked -p ui --test workspace workspace_layout::
tundra-cli debug ui-test
```

The `components`, `input`, `management_workspace`, `motion`, `screens`, and `workspace` targets group related integration tests. Filter by module name to run a particular workflow. Automation, Editor, file permissions, localization, Logs, and Script each have a dedicated target. Shell tests and real-terminal acceptance checks also cover page hit testing and complete workflows.

Related documentation: [Debug previews](../shell/docs/previews.md), [Shell sessions](../shell/docs/session.md), [UI requirements](../../docs/UI-requirements.md).
