# weathr

`weathr` renders scenes from shared weather data. Shell embeds it for the idle weather screen; the host supplies its data and manages terminal modes and return to the workspace. The crate also exposes a standalone display API for callers that need a blocking loop.

## Modules

| Location | Responsibility |
| --- | --- |
| [`embedded`](src/embedded.rs) | `EmbeddedDisplay` consumes host snapshots and produces a pane-sized Ratatui buffer. |
| [`launch`](src/launch.rs), [`app`](src/app.rs), [`app_state`](src/app_state.rs) | Expose the blocking display and host-control interface; run the display loop and track weather, clock, location, and interaction state. |
| [`scene/`](src/scene/) | Composes the outdoor world, overlay, and lock-screen surfaces. `world/` chooses ground, house, decorations, and style. |
| [`animation/`](src/animation/), [`animation_manager`](src/animation_manager.rs) | Animate per-scene elements such as clouds, precipitation, birds, and the moon, and manage which effects are active. |
| [`render/`](src/render/), [`theme/`](src/theme/), [`assets`](src/assets.rs) | Draw the scene and clock for the available terminal capabilities, resolve theme colors, and supply embedded ASCII art. |
| [`localization`](src/localization.rs), [`error`](src/error.rs) | Adapt the host's translation function and report weather asset or terminal errors. |

## Embedded rendering

`EmbeddedDisplay::new` accepts a weather snapshot, a translation function, and dimensions. The host can pass its true-color decision through `EmbeddedDisplay::new_with_color_support`. `render` returns a Ratatui Buffer for one frame and supports resizing. It does not read terminal input, switch terminal modes, or write directly to the terminal.

Weather, day/night, season, and animation together determine scenes such as houses, trees, clouds, rain and snow, and moon phases. Weather data and caching are provided by [system-services](../system-services/README.md), and Shell can prefetch them at startup.

Background tasks continue while the workspace is idle, and a key press or tap returns to the previous page. The Shell worker also handles external UI snapshot and wake requests for this embedded surface.

## Standalone display API

The blocking API supports 12/24-hour time, resizing, and an exit choice translated into `ShellLockscreenResult`. It is a library entry point, separate from Shell's embedded idle screen.

`run_display_blocking_with_control` accepts `DisplayControl`, allowing its caller to read rendered frames and supply normal terminal input.

## Validation

```sh
cargo test --locked -p weathr
bash scripts/check-weathr-dependency-boundary.sh
```

The dependency-boundary check ensures that data rendering does not introduce a reverse dependency on Shell or the system runtime.
