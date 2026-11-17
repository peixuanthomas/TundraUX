# Linux workspace performance — 2026-11-17

The standard candidate has 23/23 sufficiently sampled P95 checks within budget on this Linux machine. Both versions completed the full matrix without a capture failure. This measures completed PTY output, not a terminal emulator's pixel presentation.

Each version has 1,723 rows: 300 fresh AppHosts, 300 restores, 1,100 interactions, three resource points, and 20 native-child observations. The original baseline has 1/23 P95 checks within budget. Slow individual samples remain included. CommandLine and Vim child readiness are independent series and are not covered by the six built-in host startup budget.

## Conditions and endpoints

- Linux x86_64, Intel Xeon Platinum 8370C, five visible vCPUs with a four-CPU cgroup quota, 16GiB cgroup memory, no swap; rustc/cargo 1.99.0, LLVM 23.1.1.
- Both builds: release opt-level 3, thin LTO, codegen-units 1, panic unwind, incremental off, two compilation jobs. Builds and benchmarks use the exclusive `/tmp/tundra-validation.lock`; no compilation or other PTY tests ran during either matrix.
- Full/default motion; `TERM=xterm-256color`, `COLORTERM=truecolor`, `LC_ALL=C.UTF-8`, isolated HOME/XDG. The plain PTY does not answer graphics probes, so image capability is absent and the normal Linux ASCII interface is used. Startup and one-window layout are 120×36; two/four-window layouts are 217×39.
- Actual Python runner `8350f92e01557cc0c2e388676f3e304a4b4637d1`, master-equivalent commit `d7e5cdbbd61d647204449682642496848dc5dbab`, SHA256 `26bbaced96c4d682d6457b786b27a87c44fc4ef7c6e00d8acc2e627a275ea202`; 25 lightweight harness tests pass. The runner is separate from the immutable Rust revisions below.
- `observation_condition=zero-rpc-sealed-pty-capture-v1`; detailed trace off, including inherited `TUNDRA_PERF_TRACE`; ordinary product logs retained. All 3,440 timed captures have zero automation RPCs and no process/resource probes. Verification is after capture sealing and cannot supply the endpoint.
- Start: immediately before the physical PTY key/mouse write, on Python's monotonic clock. End: earliest complete matching business frame at Ratatui's final cursor state/position commands. Blank/Starting frames do not qualify. Business controls exclude changing metric values, clock digits and nonessential footer text. Focus matches the search field only.
- The actual first process supplies the unknown signature during a 20-second no-RPC capture; its earliest matching frame supplies its latency. Later samples stop at their first match with the same 20-second deadline. There is no extra prewarming process, timeout retry, slow-sample replacement or filtering.
- Every application has 50 distinct new AppHost PIDs: one first opening of that application in its isolated Shell and 49 repeated fresh processes. A different Home/picker page is visible before activation; earlier measured window IDs and their business signature must be absent. Editor uses Launcher as its picker.
- Restore measures complete business content visible again, including a retained old frame. Same PID and current actionable viewport/fence are verified after sealing; this does not establish an exact earlier fresh-frame or interactive-ready time.
- Percentiles use nearest rank (`sorted[ceil(N*p)-1]`). Counts above budget use unrounded raw durations. The unchanged original919 baseline was collected first, on 2026-11-16 (851.801s); the measured cd59be5d candidate was collected afterward, on 2026-11-17 (494.078s). The baseline was reused without resampling. Historical e2c12ef6, polling-active and trace-on measurements are preserved separately and do not enter this comparison.

## Fresh built-in AppHosts

N=50 per application/version, including the first opening. Goal: P95 ≤100ms. Values are milliseconds; `>100` counts individual slow samples.

| Application | Baseline P50 / P95 / P99 | Baseline >100 | Candidate P50 / P95 / P99 | Candidate >100 |
| --- | ---: | ---: | ---: | ---: |
| Launcher | 402.084 / 485.856 / 588.848 | 50 | 28.155 / 74.369 / 126.830 | 1 |
| Settings | 405.218 / 502.635 / 673.851 | 50 | 28.649 / 42.068 / 48.502 | 0 |
| Explorer | 427.120 / 598.195 / 639.590 | 50 | 29.566 / 58.481 / 74.483 | 0 |
| Clock | 441.654 / 567.580 / 607.081 | 50 | 27.150 / 35.508 / 40.268 | 0 |
| Editor | 416.202 / 498.728 / 526.296 | 50 | 32.119 / 44.571 / 44.956 | 0 |
| SystemStatus | 405.660 / 468.865 / 670.390 | 50 | 33.247 / 43.222 / 71.835 | 0 |

The candidate retains one observation above 100ms. The table records every affected application; P95 and per-observation counts have different meanings.

| Application | First opening baseline / candidate ms (N=1 each) | Repeat-new P95 baseline / candidate ms (N=49 each) |
| --- | ---: | ---: |
| Launcher | 401.088 / 39.420 | 485.856 / 74.369 |
| Settings | 374.584 / 27.324 | 502.635 / 42.068 |
| Explorer | 449.605 / 31.524 | 598.195 / 58.481 |
| Clock | 567.580 / 27.150 | 499.847 / 35.508 |
| Editor | 518.262 / 33.439 | 492.515 / 44.571 |
| SystemStatus | 416.794 / 36.645 | 468.865 / 43.222 |

The N=1 first-opening values are individual observations, not estimates of a cold-start population P95 or cold operating-system cache behavior.

## Restored business content visible

N=50 per application/version; visible-feedback check P95 ≤50ms. This table does not measure exact interactive readiness.

| Application | Baseline P50 / P95 / P99 | Baseline >50 | Candidate P50 / P95 / P99 | Candidate >50 |
| --- | ---: | ---: | ---: | ---: |
| Launcher | 3.708 / 66.165 / 130.315 | 4 | 2.963 / 4.458 / 5.108 | 0 |
| Settings | 3.818 / 68.472 / 70.910 | 7 | 3.606 / 5.120 / 9.996 | 0 |
| Explorer | 4.795 / 68.398 / 70.475 | 10 | 3.637 / 6.812 / 14.758 | 0 |
| Clock | 4.285 / 67.738 / 74.480 | 8 | 2.908 / 3.595 / 6.354 | 0 |
| Editor | 3.850 / 6.654 / 68.819 | 2 | 3.232 / 6.208 / 8.711 | 0 |
| SystemStatus | 4.459 / 70.311 / 78.147 | 8 | 3.234 / 6.105 / 12.736 | 0 |

## Ordinary visible feedback

N=100 per layout/action/version. Goal: P95 ≤50ms.

| Layout / action | Baseline P50 / P95 / P99 | Baseline >50 | Candidate P50 / P95 / P99 | Candidate >50 |
| --- | ---: | ---: | ---: | ---: |
| 1-window / input | 71.159 / 143.971 / 151.769 | 66 | 5.700 / 8.235 / 10.738 | 0 |
| 1-window / click | 71.896 / 131.556 / 142.974 | 69 | 6.322 / 9.964 / 18.062 | 0 |
| 1-window / focus | 53.586 / 105.193 / 132.936 | 52 | 5.509 / 8.228 / 9.503 | 0 |
| 2-window / input | 42.163 / 119.134 / 133.437 | 45 | 6.575 / 9.570 / 17.221 | 0 |
| 2-window / click | 57.392 / 126.637 / 137.889 | 56 | 7.004 / 10.643 / 12.317 | 0 |
| 2-window / focus | 72.268 / 119.498 / 133.271 | 57 | 6.052 / 9.251 / 12.685 | 0 |
| 2-window / pane-focus | 3.124 / 62.061 / 72.116 | 11 | 2.457 / 3.470 / 6.930 | 0 |
| 4-window / input | 40.146 / 103.538 / 131.911 | 32 | 5.893 / 8.912 / 9.802 | 0 |
| 4-window / click | 40.701 / 112.939 / 151.452 | 39 | 6.041 / 8.698 / 10.454 | 0 |
| 4-window / focus | 41.314 / 105.048 / 146.468 | 46 | 5.335 / 8.063 / 12.243 | 0 |
| 4-window / pane-focus | 3.172 / 58.577 / 69.143 | 9 | 2.682 / 4.503 / 5.660 | 0 |

The baseline has 482/1,100 interactions above 50ms; the candidate has 0/1,100. All slow samples remain included.

The following candidate samples exceed their per-observation threshold; their zero-based indices and original latencies are retained.

| Series | Sample index | Latency ms |
| --- | ---: | ---: |
| New Launcher | 18 | 126.830 |

## Native child readiness

N=10 per child/version. These have no specified built-in-host budget. `>100` is a reference count, not an acceptance threshold. CommandLine ends at its visible REPL prompt; Vim ends at its first complete empty EOB/Scratch buffer with blank first/command rows, regardless of donation/sponsor intro text.

| Child | Baseline P50 / P95 / P99 | Baseline >100 | Candidate P50 / P95 / P99 | Candidate >100 |
| --- | ---: | ---: | ---: | ---: |
| CommandLine | 463.373 / 684.736 / 684.736 | 10 | 145.657 / 225.249 / 225.249 | 10 |
| Vim | 90.990 / 151.944 / 151.944 | 2 | 56.421 / 59.887 / 59.887 | 0 |

CommandLine child P95 is 225.249ms and Vim child P95 is 59.887ms. Native-child readiness is independent of the six built-in host budget and must be reported separately.

## Resource points and normal startup logs

These are process-tree points after each layout's interactions, outside timed capture. RSS sums shared mappings per process; live accumulated CPU excludes exited workers and includes preparation/verification work. They are not per-action CPU, whole-run totals, peak memory or an idle-CPU test.

| Layout | Baseline RSS MiB / live CPU seconds | Candidate RSS MiB / live CPU seconds |
| --- | ---: | ---: |
| 1-window | 178.609 / 20.020 | 134.410 / 10.790 |
| 2-window | 233.051 / 29.580 | 166.020 / 14.830 |
| 4-window | 334.305 / 38.840 | 221.379 / 17.110 |

The following P95 milliseconds come only from each application's own 50-start launch session, excluding preparation and child hosts. `app-startup` ends when Shell accepts the first frame. Candidate `app-startup-presented` is recorded after successful terminal output. Both start at SpawnApp request, later than the physical activation start. Language and resources can overlap; init contains nested work. Do not sum these phase percentiles. Transfer is the logged `window_ipc_us` residual, including scheduling, not an isolated wire or queue measurement. Shell draw includes composition/diff/output; it is not pure PTY write time. Fine-grained input queues are not instrumented in these trace-off endpoint runs.

| Application / build | Guardian ready wait | Resources | Language | AppHost init | Frame transfer residual | Shell draw/output |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Launcher / baseline | 61.381 | 26.832 | 32.436 | 124.155 | 48.011 | — |
| Launcher / candidate | 13.102 | 1.062 | 9.986 | 17.517 | 8.186 | 2.337 |
| Settings / baseline | 81.679 | 29.925 | 38.525 | 112.314 | 63.561 | — |
| Settings / candidate | 10.547 | 1.449 | 10.040 | 11.814 | 6.758 | 1.854 |
| Explorer / baseline | 71.224 | 34.404 | 51.539 | 130.197 | 57.173 | — |
| Explorer / candidate | 20.397 | 0.928 | 8.495 | 9.936 | 8.345 | 1.855 |
| Clock / baseline | 64.169 | 32.668 | 35.977 | 120.038 | 48.726 | — |
| Clock / candidate | 10.607 | 0.883 | 6.262 | 7.705 | 5.633 | 1.630 |
| Editor / baseline | 82.355 | 36.988 | 40.772 | 139.701 | 66.152 | — |
| Editor / candidate | 15.093 | 0.847 | 7.383 | 9.435 | 6.168 | 2.279 |
| SystemStatus / baseline | 60.939 | 36.570 | 42.733 | 116.895 | 48.475 | — |
| SystemStatus / candidate | 17.674 | 1.316 | 9.813 | 10.970 | 8.803 | 2.831 |

## Functional scope

The measured optimization package cd59be5d passed 66 targeted Rust checks and eight required critical functional groups. At measurement time, the full debug-crash flow had a pre-existing failure reproduced against original919: the explicit debug request panicked AppHost and returned to Home instead of opening the promised Shell PANIC page. The user subsequently requested the separate explicit-debug routing fix 94966994 in the final delivery. That follow-up package receives focused functional validation only; its final source/binary identity and validation outcome will be recorded separately by integration. The complete performance matrix is not repeated after that fix. All performance numbers and measured binary hashes in this report remain those of cd59be5d; they must not be relabeled as measurements of the later patched binary. This report does not claim 9/9 functional groups for the measured package.

## Immutable identities

| Item | Original baseline | Measured cd59be5d optimization |
| --- | --- | --- |
| Rust source | `919db6b585923f021f3213ccf0eed25deee724af` | `cd59be5d7ed0d401be09a5e475de7758ebb4bac6` |
| tundra-shell SHA256 | `2178246ca94fd3dfaf7ea239d825e95fe4356035be82eae288f51dd77400bd2f` | `c43eee4bd0e6fb935953514303afd0846a552292ca2b2f8386d69d90a00367a6` |
| tundra-cli SHA256 | `b7fc712c78e22fd981bbfa551c6ac178fc2cda6b875cf93e7a8b27bb9d086181` | `7d8e19439f4346c502b64230c6f577929b4caf5c0937cb2b3cd33b766b9b777c` |
| vim/vim SHA256 | `a425413781c5ec21199966c37c2b9f4100d36b5a5909c678183c39906c4da9a9` | `a425413781c5ec21199966c37c2b9f4100d36b5a5909c678183c39906c4da9a9` |
| Assets manifest SHA256 | `047ae46fc5f96c636556509070c5b7678c119028a820c39303fe5005617c67d6` | `61523cbfa6f6308570168565b2efed73bc05c8670cb95ce5593c5c5deca5f7ec` |
| samples.jsonl SHA256 | `baa29935d43a5c5bf4a09727eaa2caa0ccd72ac201b52fc691d867a2cd4755dd` | `66742864f75caeadd284c2068a94c6104354e1c60dd0d97b8169aa560a9446cf` |

Both bundles have 187 asset files; baseline 9,448,547 bytes, candidate 9,448,817. Only the two locale files with new busy messages differ; post-run manifests match pre-run identities. Manifest hashing uses component-wise sorted `Path.rglob` file paths and SHA256 of compact `json.dumps(rows, sort_keys=True, separators=(',', ':'))`, with each row containing path, size and file SHA256. Vim is byte-identical, version 9.2.1225, upstream commit `e9ced3cf4a029dd3816d81fcbd36422c372030d1` (source archive SHA256 `b9ee1dc93f79c9c2444a8d57b1615adf478b2d548290c9ba85df6ec496cded57`).

Offline audits verified every timed row against its original frame index timestamp/byte offset and seal, monotonic duration arithmetic, all fresh PIDs, same-PID restores and post-seal actionability. No raw sample or failed historical attempt was edited. Earlier polling-active, trace-on, altered-profile and failed-template runs remain separate and are excluded from these tables.

## Reproduce

Use the same Rust toolchain and Linux dependencies (including the native compiler and ncurses headers needed by the pinned Vim builder). From a repository containing these commits, create independent source and runner worktrees. Keep each build's own Cargo-generated adjacent assets; use one pinned Vim bundle for both. Use the shared lock for every build, heavy test and complete benchmark. On a different machine, retain its environment and report its results separately.

```sh
git worktree add --detach /tmp/tundra-perf-before-src 919db6b585923f021f3213ccf0eed25deee724af
git worktree add --detach /tmp/tundra-perf-after-src cd59be5d7ed0d401be09a5e475de7758ebb4bac6
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
for variant in before after; do
  flock /tmp/tundra-validation.lock python3 -B \
    /tmp/tundra-perf-runner/scripts/tests/workspace-performance.py \
    "/tmp/tundra-perf-$variant/release/tundra-shell" \
    --output "/tmp/tundra-perf-results-$variant" --phase all \
    --launch-samples 50 --interaction-samples 100 --child-samples 10
done
```

The runner clears optional detailed tracing itself. Each new output directory contains per-sample data, raw PTYs and frame indexes, failed-attempt retention, observation audits, startup logs, environment, hashes and readable/JSON reports. Do not reuse output directories or substitute faster retry samples. When measuring baseline first, pass `--compare /path/to/baseline/report.json` to the later candidate run. The recorded original919 baseline predates the measured cd59be5d candidate; it was reused after all observation conditions and runner identity matched. The two reports supply all table statistics.

For this task's review, the preserved [artifact index](/workspace/tundra-perf-artifacts/ARTIFACT-INDEX.md) links both full raw directories, the original failed pilot and excluded historical conditions. [Machine-readable comparison](/workspace/tundra-perf-artifacts/zero-rpc-formal-comparison.json) includes per-group targets/counts, all summary quantiles, slow counts, identities, phase diagnostics and resource points. These local links supplement the self-contained numbers, identities and reproduction steps above.
