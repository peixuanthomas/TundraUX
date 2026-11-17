# workspace-protocol

Private IPC messages and framing for the workspace guardian, Shell, app hosts, and shared services. The crate depends on neither terminal rendering nor application state. The external CLI `ui` commands continue to use [shell-control](../shell-control/README.md).

## Modules

| Location | Responsibility |
| --- | --- |
| [`lib.rs`](src/lib.rs) | Common protocol and frame-limit constants, worker identity, roles, layout generations, and handshake envelope. |
| [`supervision`](src/supervision.rs) | Guardian messages for progress, launch, resource registration and shutdown, plus the separately versioned watchdog CLI request and response. |
| [`window`](src/window.rs), [`window/cell_encoding`](src/window/cell_encoding.rs) | Shell/AppHost viewport, input, lifecycle, and draw messages; bounded cell compression and structured cursor, image, heading, and control positions. |
| [`service`](src/service.rs) | SessionServices request/reply types, per-domain revisions and errors, and lossless native path encoding. |
| [`transport`](src/transport.rs) | Length-prefixed JSON, bounded pending bytes, and async payload read or synchronous polling helpers. Transport authentication is provided by platform. |

## Connections and identities

| Connection | Messages |
| --- | --- |
| Guardian and workers | Identity handshake, event-loop progress, probes, process state, shutdown, and Windows desktop opens |
| Shell and app host | Window initialization, viewport, input, Launcher open requests, close, and rendered frames |
| Worker and shared services | Request IDs, configuration, profile, authorization policy, file operations, and system status |
| Embedded Linux CLI and guardian | Ancestry-checked resource registration and retirement |

`RunId` identifies one guardian invocation. `SessionId` identifies one app opening and is never reused within that run. `WorkerId` includes the process generation so that delayed close and kill requests cannot affect another worker. Window coordinates count terminal cells from the content area's top left. `LayoutGeneration` binds viewport-dependent input and frames to one layout. App minimums are content dimensions, with a baseline of 106 × 16 cells. Shell reserves the outer window frame and global bars separately, preserving the 108 × 20 terminal minimum. Custom-program minimum reports are not reduced by this baseline.

## Encoding and backpressure

Each message has a four-byte big-endian length and a JSON body. Frame limits are 1 MiB for guardian worker traffic, 128 MiB for windows, and 8 MiB for shared services. Window cells carry displayable text and style in a gzip-compressed, base64-encoded JSON array, with limits of 256 Ki cells and 16 MiB of decompressed data. Image placements identify their visible regions and refer to the base64-encoded RGBA data from the same frame or the previous frame. The image limit is 80 MB of unique pixel data per window frame. Cursors, controls, and masks use structured data. `Framed` expects an authenticated nonblocking stream from the platform layer and bounds queued bytes and work per poll. Queue backpressure is reported as `WouldBlock`.

Window frames include the additive, default-false `ctrl_w_passthrough` flag. App hosts report when the editor canvas or a native terminal owns Ctrl+W, so Shell can preserve application input while handling workspace prefixes elsewhere.

Window frames also carry styled Markdown headings as text and geometry. The receiving Shell can use native text sizing after checking the pane's bounds.

## Window initialization and input receipts

The current private protocol version is **5**; the public watchdog CLI messages keep their separate version **1**. Version 5 carries system-user session identity in place of the former application account lifecycle.

Window frames carry monotonic sequences and processed input serials for tracing. A separate `confirmed_input_fence` completes only after the relevant operation receipt and acceptance of a frame for the current viewport. Background I/O resolves image references in wire order before retaining the newest frame and preserves first-frame startup timing data.

AppHost sends a reliable persisted external-control configuration receipt before confirming the related fence. The receipt carries configuration and authorization revisions and a Shell session identity, so Shell can honor it while waiting for the shared cache. `DebugPanicRequested` is also a distinct reliable Shell action; ordinary AppHost exit or panic does not imply this explicit test request.

`AppStarted` carries guardian-side spawn and readiness durations. AppHost attaches optional local startup durations to its first frame for Shell's startup log.

Window initialization can carry Shell's validated language snapshot and catalog. AppHost can use them when the snapshot matches its configuration. The language JSON is reference-counted within each process so queueing and cloning initialization do not copy its strings; its wire representation remains a JSON object.

Shell initializes each window with the latest viewport size, layout generation, and focus. AppHost also folds already received viewport updates into initialization before constructing its session, then applies retained input and control messages in order.

## Shared state

Configuration, profile, and authorization writes include the revision the caller read. File paths carried by `NativePath` use Unix bytes or Windows UTF-16 code units so they round trip even when they cannot be represented as UTF-8.

`ReadSessionSnapshot` returns one coherent view of configuration, authorization, profile, clock, file operations, and system status. Each domain retains its own revision and authorization or read error. An `Authorized` operation also checks the authorization revision before executing a queued write, including writes submitted by Shell.

`transport::read_payload` separates asynchronous I/O from JSON decoding, and `encode_message` produces bounded complete messages for background writers. The synchronous `Framed` path attempts writes immediately after enqueueing.

## Validation

```sh
cargo test --locked -p workspace-protocol
```

For process roles and protocol consumers, see the [workspace architecture](../../docs/README-TECHNICAL.md#process-architecture).
