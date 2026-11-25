# ascii-assets

`ascii-assets` loads, validates, and repairs themes, ASCII artwork, icons, fonts, and language resource files, and calculates the dimensions needed for rendering. [i18n](../i18n/README.md) handles language loading and translation.

## Modules

| Module | Responsibility |
| --- | --- |
| [`artwork`](src/artwork.rs), [`clock_font`](src/clock_font.rs) | Decode text art, themed icon sets, and the clock bitmap font; retain the geometry used by renderers. |
| [`asset_manifest`](src/asset_manifest.rs) | Declares the default theme, required paths, and expected formats. |
| [`asset_resolver`](src/asset_resolver.rs), [`asset_distribution`](src/asset_distribution.rs) | Select the runtime resource root. Cargo build scripts copy assets from the active checkout into the target profile's `assets/` directory. |
| [`asset_store`](src/asset_store.rs) | Loads and indexes an asset set for consumers, including its maximum dimensions and optional image payloads. |
| [`asset_validation`](src/asset_validation.rs), [`asset_error`](src/asset_error.rs) | Check required files and supported formats and describe path, read, and validation failures. |
| [`asset_restore`](src/asset_restore.rs), [`embedded_defaults`](src/embedded_defaults.rs) | Repair default files or provide in-memory fallbacks using the same embedded bytes. |

## Asset formats

The default manifest contains 24 items: 20 text assets, 3 TOML art sets, and 1 clock font.

- Text assets contain only printable ASCII; TOML art sets use schema v1.
- Images support GIF, JPEG, PNG, and WebP; paths must be relative paths within the asset root.
- Language resources live in `locales/<code>/`, containing `manifest.toml` and recursively read `.ftl` files.
- Default image bytes are shared through a static asset table; validation, repair, and in-memory fallback reuse the same data.
- Hosts without image support can load every catalog, font, and text asset without reading PNG payloads. Reload preserves this choice; normal and recovery loaders still require declared images.
- `app_icons.toml` defines ASCII artwork and optional images by built-in application ID. Home and Launcher use the same application catalog.
- Older themes can still supply their former icon files. The loader combines them in memory by app ID, using explicit Launcher entries when both files define the same app. Older default themes receive missing icons from the bundled theme.
- All assets contribute to dimension calculations; updating assets may change the minimum terminal size.

Asset roots are resolved in this order:

1. `TUNDRA_ASCII_ASSETS_DIR`;
2. `assets/` alongside the program;
3. `/usr/share/tundraux/assets` on Linux.

## Interface language resources

The English and Chinese packs mirror each other. Look in [`assets/locales/en-US/modules/`](assets/locales/en-US/modules/) and [`assets/locales/zh-CN/modules/`](assets/locales/zh-CN/modules/) for UI text. For example:

| File | Interface text |
| --- | --- |
| `ui-launcher.ftl` | Launcher search, result and organize controls. Built-in search uses the active language pack’s app names and descriptions. |
| `ui-home.ftl` | Home shortcut actions and icon editing |
| `common/notifications.ftl` | Notification history, clear and open-log actions, and previous desktop exit messages |
| `help.ftl` | Ctrl/Cmd selection, Shift ranges, Shift+Enter, and Tab focus navigation with arrows in the focused region |
| `management-common.ftl` | Persistent expert areas on management pages |
| `management.ftl` | Management prompts, package and command choices for adding an installed program to Launcher, and the AutoAdmin dialog title |
| `app-explorer.ftl` | Archive creation and extraction, password retries, progress, and entry errors |
| `ui-explorer.ftl` | Short toolbar labels, path controls, and the Open and Save pickers. The theme supplies the toolbar’s ASCII icons. |
| `shell-messages.ftl` | Editor file-picker prompt for text, Markdown, PDF, Word, and Excel, Office conversion feedback, workspace clipboard feedback, and terminal menu actions |
| `ui-editor.ftl` | Editor controls and PDF preview page controls, progress, and errors |
| `ui-shell.ftl`, `ui-auth.ftl` | Global Tasks button, the shared terminal-size prompt, and first-time setup including completion with default appearance |
| `ui-explorer.ftl` subfolder search controls | English and Chinese labels for the search scope, search progress and skipped entries |
| `shell-command-line.ftl` | Command Line startup hint, missing interpreter message, close confirmation, completion candidates, typo suggestions, and Right-key cycling guidance |
| `management-network-disks.ftl` | Connection states and reasons configuration is unavailable or read-only |
| `settings.ftl` | Top bar contents and alignment, Command Line shell choices, animation phase fields, validation, reset, save, and cancel actions |
| `settings.ftl`, `ui-settings.ftl` | Update mode, build selection, Git tree, update notes, and confirmation |
| `ui-style-preview.ftl`, `ui-test.ftl` | Neutral compact Tea preview, one-character task feedback, and primary and danger sample-action labels |
| `workspace.ftl` | Three-button window controls, incremental split menus, app/window search and grouping, minimum dimensions, close prompts, and watchdog CLI messages |
| `update-progress.ftl` | Update stages and confirmation that an unchanged Vim engine is reused without rebuilding |

## Default asset repair

The header’s Esc labels include the key name and the current action in both bundled languages.

Startup checks default graphics and the Chinese and English language resources. Missing or corrupt default files are restored from embedded content, while healthy custom content remains unchanged.

If the asset directory cannot be written, the program uses in-memory copies and reports the repair result. See [Language resources and recovery](../i18n/docs/localization.md) for language message IDs, argument contracts, and fallback rules.

## Validation

```sh
cargo test --locked -p ascii-assets
python scripts/check-embedded-assets.py target/release/tundra-shell.exe target/release/tundra-cli.exe
```

On Linux/macOS, use program paths without `.exe`. The check script verifies that each default image is embedded exactly once; if a platform does not use images at all, add `--allow-missing` to check only for duplicates.

Asset files are in `assets/`; see [Debug interface previews](../shell/docs/previews.md) for interface preview commands.
