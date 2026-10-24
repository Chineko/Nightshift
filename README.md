# Nightshift

Nightshift is a lightweight Linux-native VRChat sleep companion designed for Quest PCVR setups using WiVRn, with optional WayVR integration.

The project is intentionally split so Nightshift can run on its own and coexist with WayVR as an optional companion rather than depending on it.

## Current architecture

- `nightshift-protocol` — shared state, commands, events, capability handshake, and versioned IPC types.
- `nightshift-daemon` — source of truth and event-driven Unix-socket server.
- `nightshift-ui` — desktop UI client. It keeps IPC off the UI thread and requests repaints only when state changes.
- `nightshift-overlay-prototype` — OpenXR worker that provides the full-screen sleep dimmer, HMD pose reporting, and the first world-locked positioned panel surface.

The daemon socket is created at:

```text
$XDG_RUNTIME_DIR/nightshift/nightshift.sock
```

Clients receive one full state snapshot after connecting. After that, the daemon broadcasts only changed state sections. If a client falls behind, it receives a fresh full snapshot to recover.

## Run locally

Start the daemon:

```bash
cargo run -p nightshift-daemon
```

Then start the desktop UI:

```bash
cargo run -p nightshift-ui
```

Start the OpenXR worker while WiVRn/Monado is running:

```bash
cargo run -p nightshift-overlay --release
```

The UI and workers reconnect automatically if the daemon is restarted.

## UI behavior

The Home view currently includes:

- greeting using the connected VRChat display name when available
- vertical sleep-mode control card
- current brightness control
- sleep brightness target/status
- heart-rate, OSC, VRChat status, and invite-rule summaries

The Settings view includes sleep and automation controls, heart-rate provider discovery/selection and HR tuning, and positioned-overlay visibility/placement controls. Controller shortcuts are intentionally deferred until later project polish.

## Overlay behavior

Nightshift's OpenXR worker owns two separate composition responsibilities inside its placement-4 overlay session:

- a stereo projection-layer dimmer that follows canonical daemon brightness/fade state
- a world-locked `CompositionLayerQuad` panel submitted after the dimmer, so the Nightshift panel itself remains undimmed

`ShowOverlay`/`HideOverlay`, `spawn_distance_m`, and `spawn_mode` control the positioned panel only; they do not disable or reposition the sleep dimmer. `InFront` captures the HMD-centered VIEW pose once on the hidden-to-visible edge and then leaves the panel world-locked. `RestoreLastPosition` reuses the most recent valid panel pose remembered by the running overlay process, falling back to `InFront` when no remembered pose exists.

The first panel renderer is intentionally minimal: it renders a fixed-size dark surface while positioning, lifecycle, compositor ordering, and daemon control are validated. Rich panel content, direct grabbing/repositioning, and persistence of the panel transform across overlay-worker restarts remain future work.

Nightshift uses OpenXR overlay placement 4. Current WayVR uses a higher placement, so WayVR can continue to render above Nightshift when both are running.

## Performance principles

Nightshift is designed around event-driven updates:

- no UI polling loop for daemon state
- no blocking work on the UI thread
- deduplicated backend state broadcasts
- brightness slider IPC is rate-limited while dragging
- positioned-panel texture initialization happens once and the HMD VIEW pose is only located when a new panel spawn pose is needed
- external integrations should use events where possible and the lowest practical polling frequency otherwise

## License

GPL-3.0-only. See `LICENSE`.
