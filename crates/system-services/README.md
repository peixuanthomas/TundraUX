# system-services

`system-services` defines snapshots for weather, time, storage, networking, and system metrics. Enabling the `runtime` feature also provides platform collection, network requests, caching, and background refresh.

| Build configuration | Content | Consumers |
| --- | --- | --- |
| Default, no features | Shared data in `model/` | app, weathr |
| `features = ["runtime"]` | Shared data and `runtime/` services | Shell |

Data consumers do not need to pull in `reqwest`, `platform`, `watchdog`, or `time`. Shell enables the full runtime; Cargo can unify this feature with bare model dependencies in a workspace build. Storage, network, and metric snapshots support serialization so isolated app hosts consume the same sampled state. Requests for active sampling and immediate refresh are forwarded to Shell through SessionServices.

## Modules

| Location | Responsibility |
| --- | --- |
| [`model/`](src/model/) | `SystemSnapshot` combines revision and observation time with `metrics`, `network`, `storage`, `time`, and `weather` data. Each source can describe its own freshness or failure. |
| [`runtime/mod.rs`](src/runtime/mod.rs), [`config`](src/runtime/config.rs) | Start supervised workers and expose a handle for refresh and configuration requests plus snapshot subscriptions. |
| [`runtime/system_status`](src/runtime/system_status.rs) | Samples OS metrics through platform, maps native results to shared models, and refreshes fast and slow sources on their own schedules. |
| [`runtime/weather`](src/runtime/weather.rs), [`location`](src/runtime/location.rs), [`cache`](src/runtime/cache.rs) | Fetch and normalize weather, resolve the location, and reuse or persist weather and location results. |
| [`runtime/time_sync`](src/runtime/time_sync.rs), [`telemetry`](src/runtime/telemetry.rs) | Produce time from system time or a monotonic anchor; record collection, fallback and recovery events. |

The Shell worker collects the data and passes system status to SessionServices. AppHosts consume that shared snapshot; they do not independently poll the machine. Weathr renders weather data supplied by its host.

## System metrics

Sampling runs every 5 seconds in the foreground and every 30 seconds in the background, with explicit immediate refresh also available. Each metric is independently marked stale or unavailable; one failure does not affect other metrics. Network link status describes only the local interface link. Collection does not read MAC/SSID or probe external connectivity.

System Status displays data according to the current process's OS permissions. Operating-system write operations check permissions separately.

## Weather and time

The application and startup prefetch use Open-Meteo. Weather memory and disk caches last 300 seconds; location, address, and geocoding caches last 24 hours. Explicit refresh bypasses the cache.

System-time mode reads system time for every snapshot and also uses the system's local timezone on Linux. Network-time mode advances from the most recent successful synchronization using a monotonic clock. See [time](../time/README.md) for details.

## Validation

```sh
cargo test --locked -p system-services --no-default-features
cargo test --locked -p system-services --features runtime
bash scripts/check-weathr-dependency-boundary.sh
```

Runtime tests isolate weather caches and watchdog files in a temporary directory per test and clean them up after the background workers stop.
