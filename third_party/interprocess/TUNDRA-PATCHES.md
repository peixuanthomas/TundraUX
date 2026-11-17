# TundraUX interprocess patch

This directory vendors the published `interprocess` 2.4.6 crate from crates.io. Its upstream revision is `dc7bf100a1fba7062c024b26b6a23091d387a761`; original source, README, and licenses are retained.

The only Rust source change replaces the unavailable `AsyncFd::register_with_interest` call in Unix asynchronous unnamed-pipe registration with Tokio 1.52.3's stable `AsyncFd::with_interest`. `FdOps` owns its descriptor and implements `AsRawFd`, as required by that safe API. Local-socket credential checks and Windows named-pipe behavior are unchanged. Published maintenance version 2.4.8 still contained the same unavailable call when checked.

The root `[patch.crates-io]` selects this source; Cargo.lock records the local package.
