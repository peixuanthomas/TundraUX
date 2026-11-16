# Testing guide

Run all commands from the repository root. Automated tests cover core workflows, data safety, and permission boundaries; real terminals, touchscreens, system authorization, and device writes require separate acceptance checks on the corresponding platform.

## Routine checks

```sh
cargo fmt --check
cargo check --workspace --locked
cargo test --workspace --locked
cargo build --locked -p shell -p cli -p weathr
python3 scripts/check-localization.py
bash scripts/check-weathr-dependency-boundary.sh
```

The fixed test suite makes no HTTP requests and starts no HTTP test servers. Add `--offline` when dependencies are cached; add `-- --test-threads=1` when investigating concurrency issues.

When resources are limited, use:

```sh
CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 cargo test --workspace --locked
```

## Targeted tests

```sh
cargo test --locked -p app
cargo test --locked -p ui
cargo test --locked -p shell
cargo test --locked -p storage
cargo test --locked -p identity
cargo test --locked -p platform
cargo test --locked -p system-services --no-default-features
cargo test --locked -p system-services --features runtime
```

The minimum supported version is Rust 1.95. After changing the toolchain or dependencies, check that minimum version:

```sh
rustup toolchain install 1.95.0 --profile minimal
cargo +1.95.0 check --workspace --all-targets --all-features --locked
cargo +1.95.0 test --workspace --locked
```

CI configuration is in [cross-platform.yml](../.github/workflows/cross-platform.yml), [linux.yml](../.github/workflows/linux.yml), and the corresponding release workflows. Run Linux-related checks in WSL or native Linux, and Windows and macOS checks on their respective platforms.

## Linux PTY and system operations

Build Shell, CLI, and Vim before running real-terminal checks:

```sh
cargo build --locked -p shell -p cli
python3 scripts/build-vim.py --output target/debug/vim
python3 scripts/linux-shell-smoke.py target/debug/tundra-shell
TUNDRA_PTY_CHECK_CRASH=1 python3 scripts/linux-shell-smoke.py target/debug/tundra-shell
python3 scripts/tests/shell-control.py target/debug/tundra-shell
python3 scripts/tests/workspace-splits.py target/debug/tundra-shell
```

The workspace split script checks two-stage Ctrl+W input, cancellation, T layouts, four-pane limits, disabled actions, resize recovery, full screen, minimized-instance reuse, and native terminal Ctrl+W.

These scripts use isolated XDG directories and real PTYs to check initial setup, file workflows, mouse and keyboard input, Command Line, external UI control, crash reports, and terminal restoration. `TUNDRA_PTY_MOUSE_EVENT_COUNT` adjusts the number of events in the mouse-event flood test.

Readiness and global-button targeting use automation snapshots and control IDs. Page assertions use business content and painted screens, so changes to decorative frames or bar coordinates do not change the workflow contract. Shell and its companion CLI must come from the same build.

Home application entries and the OS settings entry use current controls and page identity. The unsupported-systemd check verifies the actual PID 1 process and D-Bus requirement through F4, rejects authorization as a remedy, and returns through Settings to Home. Status details must open on release and close back onto the same page.

Network and Packages open the shared Management page; Logs opens its own page. More actions uses the current control rectangle and actual menu state for press, drag-out, and release checks. The malformed-input regression still uses physical End and Enter after checking that the last Home entry is System Status.

The resize workflow first checks the imported document in Vim's native viewport, its Insert indicator, Esc, undo, and return to the standard editor without saving. Permission drafts check Space toggling, Enter advancing to the next field, and cancellation without applying filesystem changes.

The native Trash round-trip test creates only temporary items:

```sh
cargo test --locked -p platform --test native_trash_smoke -- --ignored --nocapture
```

Isolated service and scheduled-task tests use private systemd, D-Bus, mount, PID, network, and cgroup environments:

```sh
scripts/tests/isolated-systemd.sh /absolute/path/to/platform-test-binary ORIGINAL_USER_UID selection
```

Run root-startup checks only in disposable Linux test environments:

```sh
sudo python3 scripts/tests/linux-root-startup.py target/debug/tundra-shell target/debug/tundra-cli
sudo python3 scripts/linux-shell-smoke.py target/debug/tundra-shell
```

Package queries use fixed fixtures and read-only smoke tests. Do not run destructive installation, removal, or system-upgrade tests on the development host; verify real writes in an isolated environment.

## Compact UI and motion acceptance

Run the semantic matrix after building Shell and CLI:

```sh
python3 scripts/tests/ui-refresh-smoke.py target/debug/tundra-shell
```

The script checks 108 × 20, 120 × 36, and 180 × 50 in English and Chinese, with truecolor and ANSI terminal capabilities, and full and reduced motion. It checks header control order, status visibility with the keyboard open, tap activation without hover, modal interception, resize during a dialog, wide and combining characters, and return navigation using current control rectangles. For a focused run, select `--language en-US --color truecolor --motion full`.

Also complete the keyboard-only, mouse-only, and touchscreen workflows in [UI requirements](UI-requirements.md), including rapid page changes, reversal, continuous resize, details and keyboard expansion, sorting, multi-selection, and expert areas. Ordinary pages and groups use whitespace instead of full frames. At 108 × 20, the header and window bar leave 18 content rows. Check that text remains readable on the first transition frame and that Vim/child-terminal sizing and pointer targets follow the same geometry.

For performance measurements, use a Release build at 120 × 36:

```sh
cargo build --release --locked -p shell -p cli
python3 scripts/tests/ui-refresh-smoke.py target/release/tundra-shell --samples 50 --output /tmp/tundra-ui-timings.json
```

The report includes the executable path, size and modification time, platform, Python version, sample count, and P95/max timing for a raw PTY search keystroke to its matching UTF-8 output. Snapshot verification happens after timing. The acceptance budgets at 120 × 36 are input-to-output P95 ≤ 50ms and draw P95 ≤ 16.7ms. The PTY echo series includes host and terminal scheduling; it does not isolate application input-to-render time or internal draw duration, and the JSON leaves draw P95 unset. Certify the draw budget only with actual draw instrumentation, recording the method and build. Report the observed echo metric under its own name and state any unmeasured budgets.

Scheduler unit tests cover completed transitions, reduced motion, idle deadlines, and skipped frames after slow output. PTY silence alone does not prove that the runtime has no idle animation timer. Weather/rainbow retain 30 FPS; active UI transitions use 60 FPS, and one-character busy indicators advance every 80ms.

### Workspace performance acceptance

Use the same Linux Release build environment, bundled Vim, full-motion setting, and terminal capabilities for baseline and final measurements. Stop unrelated compilation while sampling:

```sh
python3 -B -m unittest discover -s scripts/tests -p test_workspace_performance.py
flock /tmp/tundra-validation.lock python3 scripts/tests/workspace-performance.py /absolute/baseline/tundra-shell --output /tmp/tundra-perf-before
flock /tmp/tundra-validation.lock python3 scripts/tests/workspace-performance.py target/release/tundra-shell --output /tmp/tundra-perf-after --compare /tmp/tundra-perf-before/report.json
```

Use the same lock around compilation and heavy tests so they cannot overlap a benchmark. The script creates isolated HOME/XDG directories and uses raw PTY actions. For Launcher, Settings, Explorer, Clock, empty Editor, and System Status it records 50 independent AppHosts per application, checks new process IDs, and separates the first opening in that Shell, subsequent new processes, and restoring the same process. It records 100 valid samples per interaction type (typing, click, keyboard focus, and, where available, pane focus) in one-window, two-window, and 217×39 four-window layouts. Command Line's visible REPL prompt and Vim's first native buffer are separate child-ready series. `--phase` and the sample-count options support a short harness check; reduced-count runs do not certify acceptance.

Both endpoints use Python's monotonic clock. The end is the first matching complete PTY frame: Ratatui emits all changed cells before Crossterm's final cursor state and position commands and flush. Targets, layout, process IDs, and desired focus styles are prepared before physical input. From that input until capture is sealed, the script only reads the PTY, decodes VT cells, and compares local signatures. It sends no automation/snapshot RPCs and performs no process/resource probes in that interval. A guard rejects accidental RPCs. A single snapshot afterward verifies the result; frames arriving during verification cannot supply or move the sealed endpoint.

Before each measured new window, the previous measured window IDs must be absent from the taskbar, the active page must be the different Home/picker page, and a known target business signature must no longer be visible in the PTY. Each sample records that activation context and checks a newly observed process. This prevents a retained or buffered picture from an earlier instance from standing in for the new process's first frame.

An unknown application's first measured process is observed for 20 seconds without RPCs. This is the real first sample, not a separate prewarming process; its latency is backdated to the earliest matching business frame within that capture. The verified signature is then reused within that isolated Shell. Subsequent captures finish at the first matching frame, with a 20-second hard timeout. They never wait for terminal silence: changing clocks and nonessential status/footer values remain outside the original business-control regions. A missing target or failed verification is retained in `failed-attempts.jsonl`, leaves the run incomplete, and stops sampling without replacing the slow attempt. Discovery adds at most 160 seconds per complete run, not 20 seconds per sample.

A new-window signature jointly contains the nonempty interactive application controls. Blank regions and unchanged feedback fail validation. Input uses a unique marker in the prepared search field; click and keyboard focus use that field's verified focused/unfocused style, with the final logical focus checked afterward. Footer changes cannot complete focus capture. Pane focus uses the prepared title's selected style. The measurement covers terminal output completion, not terminal-emulator pixel presentation. It includes PTY and host scheduling, and may include more time than the Shell's internal input-to-output interval.

Restore measures complete business content becoming visible again. A retained frame can provide that content while a fresh viewport or input fence is still pending, so this metric does not prove exact interactive readiness or AppHost processing speed. After sealing, the script separately verifies the same process and current actionable viewport. Any extra readiness checks and their elapsed time are auxiliary observations and do not alter visible latency.

Vim readiness is the first changed complete empty terminal buffer with all expected left-column EOB `~` rows and the stable `[Scratch]` / `0,0-1` / `All` status line. Donation/sponsor intro text may be present or absent; its later disappearance cannot delay the endpoint. A partial EOB display or a blocking command prompt fails the cue. The target terminal area is prepared from the Editor page geometry, and one post-capture snapshot verifies native terminal mode.

Each output directory preserves the pre-run process/cgroup environment snapshot, `samples.jsonl`, raw `*.pty` streams and their monotonic `*.frames.jsonl` boundary indexes, per-session observation audits, failed attempts, application startup logs, optional Shell timing traces, build identity, P50/P95/P99 summaries, process-tree CPU/RSS point samples, and a readable `report.md`. Observation audits record zero capture RPCs and total verification/preparation response bytes. Metric-card titles prove dashboard readiness while their changing CPU/memory/network values remain outside the startup matcher. Existing `app-startup.total_us` ends at frame acceptance; `app-startup-presented` ends after output. The benchmark clears `TUNDRA_PERF_TRACE` by default, including inherited values, while retaining normal product logs. Use `--detailed-trace` only in a separate diagnostic run to enable per-frame/input timing logs; their serialization and writes add observation overhead. JSON and Markdown reports record `trace_enabled`, observation and native/focus endpoint versions, and build/trace acceptance eligibility. Comparisons reject incompatible observation, trace, endpoint, or diagnostic-build conditions. CPU/RSS points are collected outside capture after validation; CPU excludes exited workers, summed RSS includes shared mappings per process, and neither field is an endpoint or whole-run peak measurement.

Acceptance targets are new built-in application action-to-first-business-frame P95 ≤100ms and ordinary interaction feedback P95 ≤50ms. Report each measured series and any failing scene rather than treating a hidden Starting label as readiness. The first-open series has one sample per isolated Shell in this run and must be reported separately from the larger repeated-new-process series.

## Real Vim regression tests

Ordinary workspace tests skip ignored cases that require a complete Vim. After building the engine, run them explicitly:

```sh
cargo build --locked -p shell -p cli
python3 scripts/build-vim.py --output target/debug/vim
TUNDRA_TEST_VIM="$PWD/target/debug/vim/vim" cargo test --locked -p vim-runtime --test real_vim -- --ignored --nocapture
python3 scripts/tests/editor-vim-smoke.py target/debug/tundra-shell target/debug/vim/vim
```

Runtime tests check buffer synchronization, undo, encoding, save failures, `:wq`, forced discard, and read-only protection. Shell smoke tests check the native display, Visual mode, registers, macros, multiple buffers, F8 synchronization, and terminal restoration. They compare Vim's dimensions with its terminal control at 108 × 20 and 140 × 45, including the bottom command row and SGR mouse rows and columns.

On Windows, use `python scripts/build-vim.py --output target/debug/vim` and set the following in PowerShell:

```powershell
$env:TUNDRA_TEST_VIM = (Resolve-Path target/debug/vim/vim.exe).Path
```

`editor-vim-smoke.py` uses Unix PTYs; Windows ConPTY regressions run through the corresponding Rust tests and CI. `TUNDRA_TEST_VIM` is for Rust regressions, `TUNDRA_VIM` overrides Shell's Vim path, and `TUNDRA_VIM_CLEAN=1` isolates user configuration.

## Child terminals, scripts, and Launcher

Real PTY checks for external TUIs:

```sh
python3 scripts/tests/embedded-tui-smoke.py target/debug/tundra-shell --nvim /absolute/path/to/nvim --yazi /absolute/path/to/yazi
```

This script covers startup through Command Line and Launcher, input, paste, mouse, resizing, exit, and terminal restoration. Image protocols use protocol fixtures; native images and physical touchscreens require acceptance checks on the corresponding devices.

Linux script workspace:

```sh
cargo test --locked -p app --test script
cargo test --locked -p script-runtime
cargo test --locked -p ui --test script
cargo test --locked -p shell --lib controller::script::tests
python3 scripts/tests/editor-script-smoke.py target/debug/tundra-shell
```

Add `--skip-vim` to the smoke script when only script execution and debugging need checking. For multi-document switching between Vim and scripts, also run:

```sh
TUNDRA_TEST_VIM="$PWD/target/debug/vim/vim" cargo test --locked -p shell --lib vim_host::script_tests -- --ignored
```

Launcher organize mode:

```sh
cargo test --locked -p shell --lib controller::launcher::settings::tests
cargo test --locked -p app --test launcher
python3 scripts/tests/launcher-organize-smoke.py target/debug/tundra-shell
```

## Test organization

```text
crates/<crate>/src/                 Implementation, test module declarations, and necessary test hooks
crates/<crate>/tests/unit/          Unit tests, fixtures, and helpers for private implementation
crates/<crate>/tests/*.rs           Integration tests through public APIs
crates/<crate>/tests/support/       Shared integration-test helpers
scripts/tests/                     Packaging, authorization, PTY, and system-service tests
```

Implementation modules include unit tests with `#[cfg(test)]` and `#[path = ".../tests/unit/..."] mod tests;`, preserving private-module scope. Do not give `tests/unit/` a separate `main.rs`, to avoid duplicate discovery by Cargo.

New tests should prioritize core workflows, data safety, authorization boundaries, error recovery, and user-visible regressions. Shared controls test common states and colors; page tests check hit detection, modal interception, password masking, wide characters, and resizing. When deleting cases, also remove unused fixtures and test entry points.

See [large-file editor support](editor-scalability.md) for reproduction steps and measurement definitions. Historical acceptance results remain in `docs/*validation*.md` and the corresponding specialized records.
