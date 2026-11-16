# Linux workspace performance — 2026-11-16

The standard candidate meets the measured P95 goals for all six built-in AppHost startups and all eleven ordinary interaction series on this Linux machine. Both versions completed the full matrix without a capture failure. This measures completed PTY output, not a terminal emulator's pixel presentation.

Each version has 1,723 rows: 300 fresh AppHosts, 300 restores, 1,100 interactions, three resource points, and 20 native-child observations. Candidate: 23/23 sufficiently sampled budget groups within P95 limits; original baseline: 1/23. Slow individual samples remain included. CommandLine and Vim child readiness are independent series and are not covered by the six built-in host startup budget.

## Conditions and endpoints

- Linux x86_64, Intel Xeon Platinum 8370C, five visible vCPUs with a four-CPU cgroup quota, 16GiB cgroup memory, no swap; rustc/cargo 1.99.0, LLVM 23.1.1.
- Both builds: release opt-level 3, thin LTO, codegen-units 1, panic unwind, incremental off, two compilation jobs. Builds and benchmarks use the exclusive `/tmp/tundra-validation.lock`; no compilation or other PTY tests ran during either matrix.
- Full/default motion; `TERM=xterm-256color`, `COLORTERM=truecolor`, `LC_ALL=C.UTF-8`, isolated HOME/XDG. The plain PTY does not answer graphics probes, so image capability is absent and the normal Linux ASCII interface is used. Startup and one-window layout are 120×36; two/four-window layouts are 217×39.
- Actual Python runner source `8350f92e01557cc0c2e388676f3e304a4b4637d1`, equivalent master commit `d7e5cdbbd61d647204449682642496848dc5dbab`, both script SHA256 `26bbaced96c4d682d6457b786b27a87c44fc4ef7c6e00d8acc2e627a275ea202`; 25 lightweight harness tests pass. The runner is separate from the immutable Rust revisions below.
- `observation_condition=zero-rpc-sealed-pty-capture-v1`; detailed trace off, including inherited `TUNDRA_PERF_TRACE`; ordinary product logs retained. All 3,440 timed captures have zero automation RPCs and no process/resource probes. Verification is after capture sealing and cannot supply the endpoint.
- Start: immediately before the physical PTY key/mouse write, on Python's monotonic clock. End: earliest complete matching business frame at Ratatui's final cursor state/position commands. Blank/Starting frames do not qualify. Business controls exclude changing metric values, clock digits and nonessential footer text. Focus matches the search field only.
- The actual first process supplies the unknown signature during a 20-second no-RPC capture; its earliest matching frame supplies its latency. Later samples stop at their first match with the same 20-second deadline. There is no extra prewarming process, timeout retry, slow-sample replacement or filtering.
- Every application has 50 distinct new AppHost PIDs: one first opening of that application in its isolated Shell and 49 repeated fresh processes. A different Home/picker page is visible before activation; earlier measured window IDs and their business signature must be absent. Editor uses Launcher as its picker.
- Restore measures complete business content visible again, including a retained old frame. Same PID and current actionable viewport/fence are verified after sealing; this does not establish an exact earlier fresh-frame or interactive-ready time.
- Percentiles use nearest rank (`sorted[ceil(N*p)-1]`). Counts above budget use unrounded raw durations. The ordered batches ran candidate first (499.883s), then baseline (851.801s). No historical polling-active or trace-on measurements enter this comparison.

## Fresh built-in AppHosts

N=50 per application/version, including the first opening. Goal: P95 ≤100ms. Values are milliseconds; `>100` counts individual slow samples.

| Application | Baseline P50 / P95 / P99 | Baseline >100 | Candidate P50 / P95 / P99 | Candidate >100 |
| --- | ---: | ---: | ---: | ---: |
| Launcher | 402.084 / 485.856 / 588.848 | 50 | 35.800 / 56.916 / 73.296 | 0 |
| Settings | 405.218 / 502.635 / 673.851 | 50 | 31.067 / 44.403 / 60.341 | 0 |
| Explorer | 427.120 / 598.195 / 639.590 | 50 | 36.585 / 62.924 / 68.686 | 0 |
| Clock | 441.654 / 567.580 / 607.081 | 50 | 34.396 / 61.725 / 102.147 | 1 |
| Editor | 416.202 / 498.728 / 526.296 | 50 | 42.508 / 79.566 / 106.164 | 2 |
| SystemStatus | 405.660 / 468.865 / 670.390 | 50 | 38.011 / 53.919 / 76.353 | 0 |

The candidate retains three observations above 100ms: Clock one and Editor two. No built-in startup P95 group exceeds its target.

| Application | First opening baseline / candidate ms (N=1 each) | Repeat-new P95 baseline / candidate ms (N=49 each) |
| --- | ---: | ---: |
| Launcher | 401.088 / 33.144 | 485.856 / 56.916 |
| Settings | 374.584 / 37.491 | 502.635 / 44.403 |
| Explorer | 449.605 / 37.405 | 598.195 / 62.924 |
| Clock | 567.580 / 31.540 | 499.847 / 61.725 |
| Editor | 518.262 / 42.508 | 492.515 / 79.566 |
| SystemStatus | 416.794 / 32.712 | 468.865 / 53.919 |

The N=1 first-opening values are individual observations, not estimates of a cold-start population P95 or cold operating-system cache behavior.

## Restored business content visible

N=50 per application/version; visible-feedback check P95 ≤50ms. This table does not measure exact interactive readiness.

| Application | Baseline P50 / P95 / P99 | Baseline >50 | Candidate P50 / P95 / P99 | Candidate >50 |
| --- | ---: | ---: | ---: | ---: |
| Launcher | 3.708 / 66.165 / 130.315 | 4 | 3.387 / 5.791 / 7.023 | 0 |
| Settings | 3.818 / 68.472 / 70.910 | 7 | 3.775 / 7.290 / 15.582 | 0 |
| Explorer | 4.795 / 68.398 / 70.475 | 10 | 3.904 / 5.469 / 6.413 | 0 |
| Clock | 4.285 / 67.738 / 74.480 | 8 | 3.426 / 5.310 / 7.371 | 0 |
| Editor | 3.850 / 6.654 / 68.819 | 2 | 3.717 / 8.103 / 16.116 | 0 |
| SystemStatus | 4.459 / 70.311 / 78.147 | 8 | 3.397 / 5.820 / 8.854 | 0 |

## Ordinary visible feedback

N=100 per layout/action/version. Goal: P95 ≤50ms.

| Layout / action | Baseline P50 / P95 / P99 | Baseline >50 | Candidate P50 / P95 / P99 | Candidate >50 |
| --- | ---: | ---: | ---: | ---: |
| 1-window / input | 71.159 / 143.971 / 151.769 | 66 | 6.344 / 10.998 / 21.741 | 0 |
| 1-window / click | 71.896 / 131.556 / 142.974 | 69 | 7.039 / 10.419 / 12.861 | 0 |
| 1-window / focus | 53.586 / 105.193 / 132.936 | 52 | 6.049 / 9.938 / 17.074 | 0 |
| 2-window / input | 42.163 / 119.134 / 133.437 | 45 | 7.200 / 10.902 / 15.691 | 0 |
| 2-window / click | 57.392 / 126.637 / 137.889 | 56 | 7.876 / 11.953 / 26.736 | 0 |
| 2-window / focus | 72.268 / 119.498 / 133.271 | 57 | 6.639 / 10.466 / 14.862 | 0 |
| 2-window / pane-focus | 3.124 / 62.061 / 72.116 | 11 | 2.636 / 3.773 / 5.047 | 0 |
| 4-window / input | 40.146 / 103.538 / 131.911 | 32 | 6.209 / 11.559 / 34.308 | 1 |
| 4-window / click | 40.701 / 112.939 / 151.452 | 39 | 6.990 / 14.448 / 62.681 | 2 |
| 4-window / focus | 41.314 / 105.048 / 146.468 | 46 | 6.126 / 9.555 / 15.444 | 1 |
| 4-window / pane-focus | 3.172 / 58.577 / 69.143 | 9 | 2.934 / 7.241 / 13.681 | 1 |

The candidate retains five interactions above 50ms, all in four panes (input one, click two, focus one, pane-focus one). All eleven interaction P95 groups meet the goal. The baseline has 482/1,100 interactions above 50ms; the candidate has 5/1,100.

The following candidate samples exceed their per-observation threshold; their zero-based indices and original latencies are retained, including the 121.122ms focus sample.

| Series | Sample index | Latency ms |
| --- | ---: | ---: |
| New Clock | 26 | 102.147 |
| New Editor | 39 | 106.164 |
| New Editor | 45 | 102.416 |
| 4-window / pane-focus | 26 | 62.279 |
| 4-window / input | 28 | 86.222 |
| 4-window / click | 83 | 89.931 |
| 4-window / focus | 94 | 121.122 |
| 4-window / click | 97 | 62.681 |

## Native child readiness

N=10 per child/version. These have no specified built-in-host budget. `>100` is a reference count, not an acceptance threshold. CommandLine ends at its visible REPL prompt; Vim ends at its first complete empty EOB/Scratch buffer with blank first/command rows, regardless of donation/sponsor intro text.

| Child | Baseline P50 / P95 / P99 | Baseline >100 | Candidate P50 / P95 / P99 | Candidate >100 |
| --- | ---: | ---: | ---: | ---: |
| CommandLine | 463.373 / 684.736 / 684.736 | 10 | 173.103 / 430.576 / 430.576 | 10 |
| Vim | 90.990 / 151.944 / 151.944 | 2 | 56.145 / 73.879 / 73.879 | 0 |

CommandLine P95 remains 430.576ms. This result must not be described as all application or native-process startups completing within 100ms.

## Resource points and normal startup logs

These are process-tree points after each layout's interactions, outside timed capture. RSS sums shared mappings per process; live accumulated CPU excludes exited workers and includes preparation/verification work. They are not per-action CPU, whole-run totals, peak memory or an idle-CPU test.

| Layout | Baseline RSS MiB / live CPU seconds | Candidate RSS MiB / live CPU seconds |
| --- | ---: | ---: |
| 1-window | 178.609 / 20.020 | 133.938 / 12.380 |
| 2-window | 233.051 / 29.580 | 165.254 / 16.280 |
| 4-window | 334.305 / 38.840 | 224.133 / 19.490 |

The following P95 milliseconds come only from each application's own 50-start launch session, excluding preparation and child hosts. `app-startup` ends when Shell accepts the first frame. Candidate `app-startup-presented` is recorded after successful terminal output. Both start at SpawnApp request, later than the physical activation start. Language and resources can overlap; init contains nested work. Do not sum these phase percentiles. Transfer is the logged `window_ipc_us` residual, including scheduling, not an isolated wire or queue measurement. Shell draw includes composition/diff/output; it is not pure PTY write time. Fine-grained input queues are not instrumented in these trace-off endpoint runs.

| Application / build | Guardian ready wait | Resources | Language | AppHost init | Frame transfer residual | Shell draw/output |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Launcher / baseline | 61.381 | 26.832 | 32.436 | 124.155 | 48.011 | — |
| Launcher / candidate | 20.909 | 1.331 | 17.076 | 18.782 | 9.782 | 3.884 |
| Settings / baseline | 81.679 | 29.925 | 38.525 | 112.314 | 63.561 | — |
| Settings / candidate | 14.079 | 1.013 | 11.901 | 12.295 | 7.094 | 3.148 |
| Explorer / baseline | 71.224 | 34.404 | 51.539 | 130.197 | 57.173 | — |
| Explorer / candidate | 20.903 | 1.542 | 11.019 | 14.875 | 11.394 | 2.580 |
| Clock / baseline | 64.169 | 32.668 | 35.977 | 120.038 | 48.726 | — |
| Clock / candidate | 24.583 | 1.357 | 10.658 | 12.569 | 8.344 | 3.080 |
| Editor / baseline | 82.355 | 36.988 | 40.772 | 139.701 | 66.152 | — |
| Editor / candidate | 25.873 | 1.159 | 15.030 | 19.488 | 7.717 | 4.441 |
| SystemStatus / baseline | 60.939 | 36.570 | 42.733 | 116.895 | 48.475 | — |
| SystemStatus / candidate | 15.728 | 1.079 | 14.931 | 16.828 | 11.701 | 2.485 |

## Immutable identities

| Item | Original baseline | Standard candidate |
| --- | --- | --- |
| Rust source | `919db6b585923f021f3213ccf0eed25deee724af` | `e2c12ef6045addc49305eab6d04ba2ca95dde8fc` |
| tundra-shell SHA256 | `2178246ca94fd3dfaf7ea239d825e95fe4356035be82eae288f51dd77400bd2f` | `ec5ac164dea072e3af0cc5c6095e10c94ae50cdc0167b74dd0920950c9eaa9b6` |
| tundra-cli SHA256 | `b7fc712c78e22fd981bbfa551c6ac178fc2cda6b875cf93e7a8b27bb9d086181` | `43ea68653ee2c5031383bd82042b4d677c336512ec67ee1435387e82d9e3bb44` |
| vim/vim SHA256 | `a425413781c5ec21199966c37c2b9f4100d36b5a5909c678183c39906c4da9a9` | `a425413781c5ec21199966c37c2b9f4100d36b5a5909c678183c39906c4da9a9` |
| Assets manifest SHA256 | `047ae46fc5f96c636556509070c5b7678c119028a820c39303fe5005617c67d6` | `61523cbfa6f6308570168565b2efed73bc05c8670cb95ce5593c5c5deca5f7ec` |
| samples.jsonl SHA256 | `baa29935d43a5c5bf4a09727eaa2caa0ccd72ac201b52fc691d867a2cd4755dd` | `08ca032a00b678acd0ccf5c59c53b51f061e6e40bdd8a7c514471f190137be77` |

Both bundles have 187 asset files; baseline 9,448,547 bytes, candidate 9,448,817. Only the two locale files with new busy messages differ; post-run manifests match pre-run identities. Manifest hashing uses component-wise sorted `Path.rglob` file paths and SHA256 of compact `json.dumps(rows, sort_keys=True, separators=(',', ':'))`, with each row containing path, size and file SHA256. Vim is byte-identical, version 9.2.1225, upstream commit `e9ced3cf4a029dd3816d81fcbd36422c372030d1` (source archive SHA256 `b9ee1dc93f79c9c2444a8d57b1615adf478b2d548290c9ba85df6ec496cded57`).

Offline audits verified every timed row against its original frame index timestamp/byte offset and seal, monotonic duration arithmetic, all fresh PIDs, same-PID restores and post-seal actionability. No raw sample or failed historical attempt was edited. Earlier polling-active, trace-on, altered-profile and failed-template runs remain separate and are excluded from these tables.

## Reproduce

Use the same Rust toolchain and Linux dependencies (including the native compiler and ncurses headers needed by the pinned Vim builder). From a repository containing these commits, create independent source and runner worktrees. Keep each build's own Cargo-generated adjacent assets; use one pinned Vim bundle for both. Use the shared lock for every build, heavy test and complete benchmark. On a different machine, retain its environment and report its results separately.

```sh
git worktree add --detach /tmp/tundra-perf-before-src 919db6b585923f021f3213ccf0eed25deee724af
git worktree add --detach /tmp/tundra-perf-after-src e2c12ef6045addc49305eab6d04ba2ca95dde8fc
git worktree add --detach /tmp/tundra-perf-runner d7e5cdbbd61d647204449682642496848dc5dbab
for variant in before after; do
  flock /tmp/tundra-validation.lock env CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 \
    CARGO_PROFILE_RELEASE_OPT_LEVEL=3 CARGO_PROFILE_RELEASE_LTO=thin \
    CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 \
    cargo build --locked --release -p shell -p cli \
    --manifest-path "/tmp/tundra-perf-$variant-src/Cargo.toml" \
    --target-dir "/tmp/tundra-perf-$variant"
done
flock /tmp/tundra-validation.lock python3 /tmp/tundra-perf-runner/scripts/build-vim.py \
  --output /tmp/tundra-perf-before/release/vim
mkdir -p /tmp/tundra-perf-after/release/vim
cp -a /tmp/tundra-perf-before/release/vim/. /tmp/tundra-perf-after/release/vim/
python3 -B -m unittest discover -s /tmp/tundra-perf-runner/scripts/tests \
  -p test_workspace_performance.py
for variant in after before; do
  flock /tmp/tundra-validation.lock python3 -B \
    /tmp/tundra-perf-runner/scripts/tests/workspace-performance.py \
    "/tmp/tundra-perf-$variant/release/tundra-shell" \
    --output "/tmp/tundra-perf-results-$variant" --phase all \
    --launch-samples 50 --interaction-samples 100 --child-samples 10
done
```

The runner clears optional detailed tracing itself. Each new output directory contains per-sample data, raw PTYs and frame indexes, failed-attempt retention, observation audits, startup logs, environment, hashes and readable/JSON reports. Do not reuse output directories or substitute faster retry samples. When measuring baseline first, pass `--compare /path/to/baseline/report.json` to the later candidate run. The actual batches above use candidate-first order and the two reports supply all table statistics.

For this task's review, the preserved [artifact index](/workspace/tundra-perf-artifacts/ARTIFACT-INDEX.md) links both full raw directories, the original failed pilot and excluded historical conditions. [Machine-readable comparison](/workspace/tundra-perf-artifacts/zero-rpc-formal-comparison.json) includes per-group targets/counts, all summary quantiles, slow counts, identities, phase diagnostics and resource points. These local links supplement the self-contained numbers, identities and reproduction steps above.
