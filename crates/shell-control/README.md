# shell-control

`shell-control` defines the public local UI-control protocol shared by `tundra-cli ui`, Shell, and UI frame capture. The version is **1** and error codes are independent of the interface language.

## Wire types

All types are in [`src/lib.rs`](src/lib.rs).

| Types | Responsibility |
| --- | --- |
| `Rect`, `Node`, `Snapshot` | Screen and control geometry, visible text, focus, and action availability. Coordinates are terminal cells, with `(0, 0)` at the top left. |
| `Action`, `Target`, `Button`, `Phase` | Describe an input or query, a control or coordinate target, and pointer or key details. |
| `Request`, `Response`, `ControlError`, `SessionInfo` | Request/version envelope, result or stable error, and available Shell sessions. The size constants bound each request and response. |

## Integration

[platform](../platform/README.md) supplies connections, [ui](../ui/README.md) captures the frame, and [shell](../shell/README.md) checks configuration and translates requests into normal input. Private worker connections and watchdog recovery messages use [workspace-protocol](../workspace-protocol/README.md).

A `wake` request exits idle weather and returns the subsequent Shell screen. On ordinary pages, it only returns the current snapshot without sending a key.

Snapshot numbers increase with rendering, but automatic updates do not invalidate old numbers. After real user actions, Shell rejects control requests until a snapshot is read again; explicitly specifying a number from before the user action is still rejected, except on weather screens.

## Validation

```sh
cargo test --locked -p shell-control
```

See [UI control](../cli/docs/ui-control.md) for enabling the feature, examples, timeout rules, and real terminal tests.
