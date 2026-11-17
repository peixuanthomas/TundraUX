# Large-file editor support

Highlighting, soft wrapping, search, and line-change markers run their expensive calculations in the background and cache results by viewport or data block. The main event loop never rescans the whole document for them.

## Computation and caching

- **Syntax highlighting:** the viewport keeps tokens only for the logical lines it needs. Parser state is saved every 128 lines, up to 2,048 checkpoints. Edits reuse the unaffected prefix; the first jump to the end of a file parses the prefix in the background to recover multiline comment and string state.
- **Soft wrapping:** background passes build cumulative display-line counts for logical lines, and display lines are generated on demand. ASCII positions are computed directly; long Unicode lines save a byte checkpoint every 256 display lines. A width change rebuilds the index.
- **Search:** plain queries scan the Rope in 64 KiB windows, keeping enough context for the query length and Unicode word boundaries. Interactive search caches at most 512 match positions while it keeps counting the full total; F3 reuses results within a page.
- **Replacement:** previews use the same chunked engine. Before applying changes, the editor checks revision, document generation, original text, and grapheme boundaries; the whole batch is one undo operation. Full previews keep at most 10,000 matches or 8 MiB of data.
- **Background tasks:** files over 64 KiB use background threads. Each feature keeps at most one running task and one pending task. New requests cancel old ones, and stale results are never written back.
- **Line-change markers:** background comparison first skips unchanged prefixes and suffixes. Incremental word counting for a single extremely long ASCII line checks only a fixed stretch on either side of the edit.

## Reproducing benchmarks

```sh
cargo build --release -p app --example editor_scalability
python3 scripts/benchmark-editor.py > editor.csv 2> editor-rss.csv
python3 scripts/benchmark-editor.py --sizes 1 10 --fixtures dense > dense.csv 2> dense-rss.csv
```

The default matrix covers 1, 10, and 100 MiB files with extremely long single ASCII lines, ASCII logs, Chinese, Emoji, and CRLF logs. Each scenario runs in its own process and measures, in order, first-screen highlighting, end-of-file highlighting, soft wrapping, search, and reflow with line markers after an edit.

Metric definitions:

- `request_ms`: main-thread time before the call returns.
- `total_ms`: total time, including background completion and the 1ms polling interval.
- `extra_peak_heap_mib`: peak increase over allocations already live when the operation started.
- Linux `VmHWM`: peak RSS of the whole process, including the opened file.

## Reference results

2026-10-13, Linux x86_64, AMD EPYC 9V74, Rust 1.99.0, release build. These numbers come from a single run and are for comparing implementations, not CI timing thresholds.

| Extremely long single ASCII line | Search request (ms) | Search completion (ms) | Extra peak search heap (MiB) |
| --- | ---: | ---: | ---: |
| 1 MiB | 0.137 | 2.288 | 0.124 |
| 10 MiB | 0.179 | 4.463 | 0.128 |
| 100 MiB | 0.135 | 25.403 | 0.128 |

| 100 MiB scenario | Wrap request (ms) | Wrap completion (ms) | Edit and reflow request (ms) | Edit and reflow completion (ms) | Peak RSS (MiB) |
| --- | ---: | ---: | ---: | ---: | ---: |
| Extremely long single ASCII line | 0.018 | 3.235 | 3.067 | 6.775 | 216.5 |
| ASCII logs | 0.048 | 860.124 | 0.434 | 398.413 | 277.8 |
| Chinese | 0.039 | 3784.134 | 0.361 | 254.921 | 217.0 |
| Emoji | 0.059 | 3591.546 | 0.345 | 312.699 | 263.6 |
| CRLF | 0.042 | 755.571 | 0.340 | 311.711 | 274.0 |

The 10 MiB dense scenario has 5,242,880 matches, of which the interactive cache keeps 512 positions. Extra peak search heap is 0.167 MiB, full counting takes 1.39 seconds, and the main-thread request takes 0.110ms.

## Implementation limits

- Regular expressions keep cross-line, anchor, and capture-group semantics and run after the background builds a contiguous string; cancellation takes effect after the engine call returns.
- Logical lines over 256 KiB, and all content after them, get no syntax highlighting. The text can still be edited and searched normally.
- Wrap indexes and line-change markers are stored by logical line. Highlighting distant content and the initial indexing pass can take several seconds of background work.
- `SearchEngine::find_all`, `find_all_rope`, and the full-document highlighting API return every result; the interactive UI uses the background viewport and paginated APIs.

## Regression checks

```sh
cargo test --locked -p app
cargo test --locked -p shell --lib editor
cargo test --locked -p shell --test editor
cargo test --locked -p ui --test editor
cargo check --workspace --all-targets --all-features --locked
python3 scripts/check-localization.py
```

`editor_scalability` covers equivalence between chunked and contiguous search, Unicode boundaries, dense pagination, cyclic navigation, stale tasks, replacement previews, long-line wrapping, and cache consistency after edits, undo, and width changes.
