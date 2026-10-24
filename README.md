# Nightshift

Nightshift is a lightweight Linux-native VRChat sleep companion designed for Quest PCVR setups using WiVRn, with optional WayVR integration.

The project is intentionally split so Nightshift can run on its own and coexist with WayVR as an optional companion rather than depending on it.

## Current architecture

The workspace is intentionally split into small policy, transport, worker, and UI crates:

- `nightshift-protocol` — shared state, commands, events, capability handshake, and versioned IPC types.
- `nightshift-daemon` — canonical source of truth, persistence, sleep-state transitions, and event-driven Unix-socket server.
- `nightshift-ui` — desktop UI client. It keeps IPC off the UI thread and requests repaints only when state changes.
- `nightshift-overlay-prototype` (`nightshift-overlay`) — OpenXR worker for the full-screen sleep dimmer, HMD pose reporting, and the first world-locked positioned panel surface.
- `nightshiftctl` — command-line client for inspecting daemon state and exercising supported controls.
- `nightshift-osc` — OSC/OSCQuery runtime worker that connects VRChat avatar parameters to daemon automation.
- `nightshift-osc-script` — bounded OSC automation script engine.
- `nightshift-invites` — invite policy/rule evaluation independent of VRChat transport.
- `nightshift-gogo` — GoGo Loco preset/state policy.
- `nightshift-pose` — sleeping-pose classification logic.
- `nightshift-hr` — heart-rate calm-period/sleep detector.
- `nightshift-hr-ble` — BLE heart-rate discovery and measurement provider.
- `nightshift-hr-worker` — daemon-connected heart-rate provider lifecycle worker.
- `nightshift-vrchat-api` — HTTP/pipeline transport helpers for VRChat API operations.
- `nightshift-vrchat-session` — desktop keyring-backed VRChat session persistence.
- `nightshift-vrchat-auth` — VRChat login and two-factor authentication service.
- `nightshift-vrchat-status` — sleep/wake VRChat status policy.
- `nightshift-vrchat-worker` — authenticated VRChat runtime worker for status, invites, and pipeline events.

The daemon socket is created at:

```text
$XDG_RUNTIME_DIR/nightshift/nightshift.sock
```

Clients receive one full state snapshot after connecting. After that, the daemon broadcasts only changed state sections. If a client falls behind, it receives a fresh full snapshot to recover.

### IPC trust boundary

Nightshift's local IPC security boundary is the Unix user account. The daemon creates its runtime directory with mode `0700` and its socket with mode `0600`, so other Unix users cannot connect through the normal filesystem path. Processes running as the same user are trusted: client names in the handshake are routing/authorization labels, not cryptographic identities, and a malicious same-UID process could send ordinary commands or impersonate a worker label. If Nightshift ever exposes IPC outside the owning user account, the protocol needs a stronger authenticated peer model before that boundary is widened.

Persisted settings live under the user's Nightshift config directory. The daemon secures that directory to `0700`, writes config and recovery files with mode `0600`, uses an atomic temporary-file replacement, and preserves unparseable config bytes as `config.json.bak` before falling back to defaults.

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

`ShowOverlay`/`HideOverlay`, `spawn_distance_m`, and `spawn_mode` control the positioned panel only; they do not disable or reposition the sleep dimmer. `InFront` captures the HMD-centered VIEW pose once on the hidden-to-visible edge and then leaves the panel world-locked. `RestoreLastPosition` prefers the most recent valid pose remembered by the running overlay process, then the validated pose persisted by the daemon, and falls back to `InFront` when neither exists. Valid captured poses are reported only when the remembered pose changes and are normalized before persistence; panel visibility itself is never persisted.

The positioned panel renders live Nightshift status on a fixed-size dark surface. Positioning, lifecycle, compositor ordering, daemon control, and restart-safe remembered placement are now wired. Direct grabbing/repositioning and richer interactive panel controls remain future work.

Nightshift uses OpenXR overlay placement 4. Current WayVR uses a higher placement, so WayVR can continue to render above Nightshift when both are running.

## Performance principles

Nightshift is designed around event-driven updates:

- no UI polling loop for daemon state
- no blocking work on the UI thread
- deduplicated backend state broadcasts
- brightness slider IPC is rate-limited while dragging
- VRChat favorite-friend membership is cached briefly per authenticated session to avoid repeatedly paging the favorites API during invite bursts
- positioned-panel texture initialization happens once and the HMD VIEW pose is only located when a new panel spawn pose is needed
- remembered panel poses are reported only when they change rather than once per rendered frame
- external integrations should use events where possible and the lowest practical polling frequency otherwise

## License

GPL-3.0-only. See `LICENSE`.
