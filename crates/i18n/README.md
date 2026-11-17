# i18n

`i18n` handles Fluent language-pack loading, validation, default-resource repair, and immutable language snapshots. It shares a resource root with `ascii-assets`; language packs reside in `root/locales/<code>/`.

## Modules

| Location | Responsibility |
| --- | --- |
| [`catalog`](src/catalog.rs) | Discovers packs, parses manifests, and normalizes language codes. |
| [`resource`](src/resource.rs), [`contract`](src/contract.rs) | Load and validate Fluent sources, then check message identifiers, variables, and fallback against the generated English contract. |
| [`snapshot`](src/snapshot.rs) | Builds immutable bundles, repairs default resources during startup, resolves fallbacks, and serializes language data for workers. |
| [`scope`](src/scope.rs) | Applies the active snapshot to the current thread and exposes the fixed-language diagnostic renderer. |
| [`message`](src/message.rs), [`error`](src/error.rs) | Define retained messages, raw text, typed arguments, localized errors, and loading/repair diagnostics. |
| [`build.rs`](build.rs), [`build_support.rs`](build_support.rs) | Embed the built-in catalogs and generate message IDs and contracts from the source resources. |

## Language packs

Each language pack contains a `manifest.toml`:

```toml
format_version = 1
code = "zh-CN"
native_name = "简体中文"
```

`.ftl` files in `common`, `modules`, and `recovery` are loaded recursively. `zh-Hans` is an input alias for `zh-CN`; directories and manifests use the canonical code. `LanguageCatalog::built_in()` provides English and Chinese language choices without accessing the filesystem.

Keep message IDs and argument names in sync between the bundled Chinese and English packs. Translate labels and diagnostics through Fluent; keep CLI flags and JSON status values stable across languages.

## Snapshots and messages

```rust
use std::sync::Arc;
use i18n::{LanguageSnapshot, LocalizedText};

let load = LanguageSnapshot::load_startup("assets", "zh-CN", 1);
let snapshot = Arc::new(load.snapshot);
i18n::with_snapshot(&snapshot, || {
    let label = i18n::tr!("notifications-action-ok");
    let retained = i18n::msg!("resources-repaired", count = 1, files = "manifest.toml");
    let raw = LocalizedText::from("external command output");
    assert_eq!(raw.render_current(), "external command output");
});
```

- `msg!` retains a message ID and typed arguments; `tr!` formats immediately.
- `enter_snapshot` returns a thread-bound RAII guard. Scopes can nest and are isolated between threads.
- `current_snapshot()` clones the current thread's snapshot; background helpers can transfer language resources through `export()` / `restore()`.
- An authenticated peer can rebuild bundles from an exported snapshot with `restore_validated()` and reuse the sender's reference and argument checks. The serialized language catalog carries language choices. The discovering process reports its diagnostics.
- `String` and `&str` always represent raw text when converted to `LocalizedText` and are not translated.
- Rendering reads only memory, without disk access or log writes.

Snapshots separately retain the current language, English fallback, built-in English, and minimal emergency resources. Missing messages or formatting failures fall back through these layers; unknown IDs appear as `[message-id]`.

During snapshot construction, identical Fluent text is parsed once and shared through `Arc<FluentResource>`. The English and built-in fallback layers also share an `Arc<FluentBundle>` when disk English is empty or its complete text sequence matches the built-in resources. Custom English retains its own bundle and override order. Sharing stays within that snapshot, so publishing another language or generation preserves earlier snapshots.

## Loading and validation

Strict loading checks manifests, Fluent syntax, duplicate IDs, references and cycles, functions, and English argument contracts. `NUMBER` is supported. Missing translations are recorded in loading diagnostics and fall back to English.

Startup loading can skip damaged translation files and continue with built-in English and Chinese resources. Existing snapshots remain unchanged when disk files change; after a successful reload, the caller publishes a new snapshot.

The build script embeds English from `crates/ascii-assets/assets/locales/en-US` and generates:

- `i18n::ids` message constants;
- `MESSAGE_IDS`;
- `MESSAGE_CONTRACTS` and `message_contract(id)`.

Default English resources and damaged English or Chinese files are repaired using temporary files in the same directory and atomic renames. Healthy files retain their original bytes, missing entries are appended to the appropriate files, and write failures retain the in-memory corrections and return structured diagnostics.

`render_diagnostic` always uses built-in English and is suitable for error `Display` implementations and logs. `LocalizedError` retains a stable `event_code`, a message, and an optional cause chain.

## Documentation and validation

See [Language resources and recovery](docs/localization.md) for usage and resource recovery.

```sh
cargo test --locked -p i18n
python3 scripts/check-localization.py
```

Tests cover catalogs, repair, argument contracts, plural fallback, snapshot lifetimes, thread isolation, and serialization.
