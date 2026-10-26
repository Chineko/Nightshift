# Nightshift performance measurement checklist

Use the existing debug report as the before/after yardstick for performance changes. Measure the same workload on the same machine before and after each optimization slice.

## Record before and after

- Idle RSS for each Nightshift process.
- Thread count for each Nightshift process.
- Open file descriptor count for each Nightshift process.
- Cold start time until the desktop UI is responsive.
- Time to open the friend picker and display cached avatars.
- Subjective desktop UI smoothness while scrolling and resizing.
- IPC responsiveness while a VRChat status write or reconnect is in flight.

## Tokio runtime changes

Before switching a process to a current-thread runtime:

- Search that process for blocking `std::fs` and `std::process` calls.
- Check for long-held `std::sync::Mutex` guards on async paths.
- Move unavoidable blocking work behind `spawn_blocking` or an async equivalent.
- Compare IPC latency as well as RSS/thread count.
- Convert the HR worker last; btleplug/D-Bus behavior gets its own validation pass.

## Build profile changes

For dev dependency optimization, compare:

- Clean build time.
- Incremental rebuild time after editing one Nightshift crate.
- UI/overlay responsiveness from `target/debug`.

For release profile changes, compare:

- Binary sizes.
- Release build time.
- Startup time and idle RSS.

Change one class of optimization at a time so any regression has an obvious culprit. Hydra dislikes ambiguous benchmarks.
