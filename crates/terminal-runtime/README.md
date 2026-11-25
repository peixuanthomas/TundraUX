# terminal-runtime

`terminal-runtime` manages host-terminal modes and isolated child terminals. Command Line, external programs, Vim, and scripts share this implementation of PTY/ConPTY, input encoding, terminal parsing, and process cleanup.

## Modules

| Module | Responsibility |
| --- | --- |
| [`session`](src/session.rs) | `TerminalGuard`, host modes and restoration, color and graphics capability detection |
| [`pty`](src/pty.rs), [`process_tree_linux`](src/process_tree_linux.rs) | Start the child, resize, read output, and clean up the process tree, including Linux-specific ownership and termination logic. |
| [`emulator`](src/emulator.rs), [`emulator/images`](src/emulator/images.rs) | In-memory WezTerm state, protocol replies and synchronized updates; decode and track child image placements within limits. |
| [`input`](src/input.rs), [`input_queue`](src/input_queue.rs) | Encode input for the child's current mode and serialize input and replies through a bounded supervised writer. |
| [`snapshot`](src/snapshot.rs) | Convert parsed cells, cursor, scrollback, and image state into shared renderable snapshots. |
| [`ansi`](src/ansi.rs) | Encode ANSI foreground colors used by terminal hosts. |

Shell decides when to take over or hand back the terminal; this crate applies and restores settings, while UI only renders snapshots.

## Host terminal

`TerminalGuard` manages raw mode, the alternate screen, mouse, focus, bracketed paste, and enhanced key reporting. Normal exit, temporary handoff of the terminal, and catchable failures all restore modes enabled by the program. On Windows, cleanup also releases the display if an inline image was interrupted.

Startup detection can read the terminal's base colors, default foreground/background, and true-color capability. Queries support fragmented replies and common terminators; rendering only reads cached results. WSL in Windows Terminal prefers win32-input-mode; other terminals use Kitty or xterm key reporting according to their capabilities.

## Child terminals

`PtyConfig`, `PtyProcess`, and `PtyExitStatus` are general process interfaces. The default configuration adds no application arguments or private environment variables; Shell decides how to launch the built-in CLI.

- `send_key`, `send_paste`, and `send_mouse` encode input according to the child-terminal modes active at send time.
- Mouse coordinates are converted from the host content area to child-terminal coordinates and sent using the Default, UTF-8, or SGR protocol.
- The input queue is capped at 4 MiB. Its sole writer thread is managed by watchdog and writes keys, protocol replies, mouse input, and paste in order.
- `resize` returns immediately when the size has not changed, avoiding redundant notifications and screen-revision changes.
- A program may report its minimum viewport with `OSC 1337;SetUserVar=TUNDRA_MIN_SIZE=<base64>` followed by BEL or ST. The decoded value is `<columns>x<rows>`, both positive decimal integers up to 65535. An empty value clears the report. Invalid values leave the last report in place.
- `PtyProcess::reported_minimum_size()` reads the current report. Report updates advance the output revision even when the screen is unchanged. The report stays out of rendered and recorded output and clears when the program exits.
- `into_output` cleans up the process tree and drains output, returning a `TerminalOutput` that supports scrollback, resizing, and snapshots.
- An output receiver can save the complete task history chunk by chunk in the reader thread, without the terminal's scrollback-line limit.
- New parsed output and EOF notify the optional platform host-loop callback after releasing the parser lock, so output can be displayed as soon as it arrives.

Unix uses process groups, and Windows uses Job Objects to clean up the corresponding child-process tree. macOS also terminates background process groups in the same terminal session, without affecting other terminal instances.
In managed sessions the independent guardian registers a separate Unix PTY session before the program starts. On Windows the guardian's Job Object contains the child at launch and retains the terminal's nested Job Object for cleanup.

## Terminal parsing and snapshots

Child-program output enters the WezTerm in-memory terminal rather than operating on the host directly. Clipboard operations, titles, unknown OSC sequences, and file downloads are not passed to the host terminal.

`ui_snapshot_with_revision`, `ui_snapshot_if_changed`, and `ui_snapshot_after_exit` can generate UI snapshots directly. Static frames reuse an `Arc`, updating on output, resize, or post-exit refresh. Snapshots preserve per-cell styles, italics, and DECSCUSR cursor shapes.

Kitty keyboard negotiation supports presses, repeats, releases, and extended keys. Kitty, Sixel, and iTerm2 images are parsed into shared RGBA data and crop rectangles, with an image-decoding budget of 20 million pixels and 80 million bytes; text task history does not store image payloads. Synchronized updates publish the screen after the end sequence or a 200ms timeout. Image tests replay the UI’s inline PNG output through the WezTerm parser and check its full source rectangle, final position, and cursor visibility during partial writes.

## Command Line host

Shell launches `tundra-cli repl --embedded` from its own binary directory, using `xterm-256color` and 2,000 lines of scrollback. Each page instance has an independent PTY and continues reading output while covered.

Shell handles Ctrl+C as copy and Ctrl+V as paste in the built-in Command Line, Launcher terminals, and script output. Ctrl+\ and the terminal menu's Interrupt action send the original `0x03` byte to the child. Esc and Ctrl+Shift+X go to the child CLI. The top-right Back button cleans up the current instance after confirmation. Vim uses its ordinary PTY input and native editing keys.

A normal `exit` returns to the original Launcher or Explorer, and the command-line host interprets the built-in CLI's special exit codes.

## Validation

```sh
cargo test --locked -p terminal-runtime
cargo test --locked -p terminal-runtime --features native-tests
```

Input encoding and protocol parsing use in-memory tests by default. Enable `native-tests` on a host with a working Unix PTY or Windows ConPTY to exercise real child I/O, process cleanup, and terminal keyboard reporting.

Related documentation: [Shell sessions](../shell/docs/session.md), [Command Line](../cli/docs/commands.md), [Local crossterm patch](../../third_party/crossterm/TUNDRA_PATCH.md), [Local WezTerm patch](../../third_party/wezterm-term/TUNDRA_PATCH.md).
