# Nightshift

Nightshift is a lightweight Linux-native VRChat sleep companion designed for Quest PCVR setups using WiVRn, with optional WayVR integration.

The project is intentionally split so Nightshift can run on its own and WayVR can discover it at runtime as an optional client.

## Current architecture

- `nightshift-protocol` — shared state, commands, events, capability handshake, and versioned IPC types.
- `nightshift-daemon` — source of truth and event-driven Unix-socket server.
- `nightshift-ui` — desktop UI client. It keeps IPC off the UI thread and requests repaints only when state changes.
- `nightshift-overlay-prototype` — OpenXR full-screen dimmer and HMD pose worker. It follows canonical daemon brightness/fade state; it is not the future positioned WayVR/panel surface.

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

The UI reconnects automatically if the daemon is restarted.

## UI behavior

The Home view currently includes:

- greeting using the connected VRChat display name when available
- vertical sleep-mode control card
- current brightness control
- sleep brightness target/status
- heart-rate, OSC, VRChat status, and invite-rule summaries

The Settings view currently includes sleep and automation controls plus heart-rate provider discovery/selection and HR tuning. Additional rule editing, controller shortcut settings, and WayVR-specific overlay behavior will be added as their backends are wired in.

## Overlay behavior

The current OpenXR overlay prototype is a stereo projection-layer dimmer used for Nightshift brightness/fade behavior. Because it covers the headset views, meter-based spawn distance and positioned-panel placement modes do not apply to that dimmer.

The daemon protocol/config already carries bounded `spawn_distance_m` and `spawn_mode` settings for a future positioned overlay surface. Those settings are intentionally not presented as active dimmer controls until a WayVR or OpenXR quad/panel backend consumes them. `ShowOverlay`/`HideOverlay` likewise remain reserved for that positioned surface rather than disabling the sleep dimmer.

## Performance principles

Nightshift is designed around event-driven updates:

- no UI polling loop for daemon state
- no blocking work on the UI thread
- deduplicated backend state broadcasts
- brightness slider IPC is rate-limited while dragging
- external integrations should use events where possible and the lowest practical polling frequency otherwise

## License

GPL-3.0-only. See `LICENSE`.
