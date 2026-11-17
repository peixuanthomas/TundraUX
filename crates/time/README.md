# time

`time` provides `NetworkClock`, `ClockDisplay`, and `ClockSnapshot`, handling time advancement, HTTP time synchronization, and timezone conversion. Alarms and timer tasks belong to [app](../app/README.md).

## Modules

| Location | Responsibility |
| --- | --- |
| [`clock`](src/clock.rs) | Holds the system/network clock mode, configured timezone and last successful time sample. Produces UTC and localized time snapshots. |
| [`network`](src/network.rs) | Validates a custom server URL, requests HTTP(S) time samples, parses `Date` headers, and reports source failures. |

## How it works

1. By default, request HTTP `Date` response headers from Google, Cloudflare, and Microsoft in sequence every 5 minutes. Each connection timeout and total request timeout is 5 seconds.
2. After success, retain the UTC time and a monotonic `Instant`, advancing the current value by the elapsed time.
3. Use `chrono-tz` to convert to the configured IANA timezone; timezone rules handle daylight saving time.
4. On synchronization failure, retain the last successful sample; use the system clock if no sample is available.

HTTP time is used for application display and does not change the operating-system time.

`NetworkClock::operating_system()` reads system time and the local timezone on every call. Linux Shell uses this mode. On Windows and macOS, application settings can select the system clock, default HTTP(S) servers, or a custom address; a custom address is saved after successful validation. The [system-services runtime](../system-services/README.md) schedules shared time refreshes.

## Validation

```sh
cargo test --locked --manifest-path crates/time/Cargo.toml
```

Run from the repository root to avoid confusion with the workspace dependency of the same name. Tests cover offline date parsing, server-address validation, and local-clock calculations without making HTTP requests.
