# Nightshift

Nightshift is a lightweight Linux-native VRChat sleep companion designed for Quest PCVR setups using WiVRn, with optional WayVR integration.

The project is intentionally split so Nightshift can run on its own and coexist with WayVR as an optional companion rather than depending on it.

## Current architecture

The workspace is intentionally split into small policy, transport, worker, and UI crates:

- `nightshift-protocol` — shared state, commands, events, capability handshake, and versioned IPC types.
- `nightshift-daemon` — canonical source of truth, persistence, sleep-state transitions, and event-driven Unix-socket server.
- `nightshift-ui` — desktop UI client. It keeps IPC off the UI thread and requests repaints only when state changes.
- `nightshift-overlay-prototype` (`nightshift-overlay`) — OpenXR worker for the full-screen sleep dimmer, HMD pose reporting, and the world-locked positioned status/control panel.
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

With WiVRn/Monado running and Fish installed, the full local stack is one command:

```fish
scripts/nightshift-dev.fish --full
```

`--full` builds and starts the daemon, OSC worker, BLE heart-rate worker, VRChat runtime worker, OpenXR overlay, and desktop UI. Heart-rate scanning still follows the persisted monitoring setting, so starting the worker does not enable monitoring by itself. The VRChat worker uses the stored keyring session when one is available and otherwise remains disconnected. Add `--hydra` for verbose overlay diagnostics.

The launcher keeps its lightweight historical default: running it with no options starts only the daemon, OSC worker, and OpenXR overlay. Optional components can also be selected individually with `--ui`, `--hr`, and `--vrchat`. Ctrl+C stops every child the launcher started.

For manual development, the components can still be started separately. Start the daemon:

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

Workers reconnect automatically if the daemon is restarted.

## UI behavior

The Home view currently includes:

- greeting using the connected VRChat display name when available
- vertical sleep-mode control card
- current brightness control
- sleep brightness target/status
- heart-rate, OSC, VRChat status, and invite-rule summaries

The Settings view includes sleep and automation controls, heart-rate provider discovery/selection and HR tuning, and positioned-overlay visibility/placement controls. Global controller shortcuts are intentionally deferred until later project polish.

## Overlay behavior

Nightshift's OpenXR worker owns two separate composition responsibilities inside its placement-4 overlay session:

- a stereo projection-layer dimmer that follows canonical daemon brightness/fade state
- a world-locked `CompositionLayerQuad` panel submitted after the dimmer, so the Nightshift panel itself remains undimmed

`ShowOverlay`/`HideOverlay`, `spawn_distance_m`, and `spawn_mode` control the positioned panel only; they do not disable or reposition the sleep dimmer. `InFront` captures the HMD-centered VIEW pose once on the hidden-to-visible edge and then leaves the panel world-locked. `RestoreLastPosition` prefers the most recent valid pose remembered by the running overlay process, then the validated pose persisted by the daemon, and falls back to `InFront` when neither exists. Valid captured poses are reported only when the remembered pose changes and are normalized before persistence; panel visibility itself is never persisted.

The positioned panel renders live Nightshift status on a fixed-size dark surface and includes four controller-selectable controls: `SLEEP`, `WAKE`, `DARKER`, and `BRIGHT`. Nightshift ray-tests OpenXR aim poses against the world-locked quad, draws a lightweight laser while a controller is aimed at the panel, and highlights the hovered button without rerastering for every pointer movement. The panel is composited before the laser quads so the pointer remains visible on top of the tablet surface. Trigger/select is reserved for UI activation and sends button actions through the same daemon command path used by the desktop UI. On Oculus Touch-compatible interaction profiles, squeeze/grip is reserved for panel manipulation: pressing grip while aimed at the panel captures the full controller-relative transform, so the panel can translate, turn, tilt, and angle naturally while held. Releasing commits the resulting world pose once through the existing remembered-pose persistence path; live drag frames do not write persistence state. Misses and the gaps between buttons remain non-actions rather than snapping to a nearby control. Brightness buttons adjust the current canonical brightness in 10-percentage-point steps. The simple-controller profile remains a UI-select fallback, but it does not provide Nightshift with a separate grab control. If controller-action setup is unavailable on a runtime, the status panel continues rendering while interaction is disabled.

For a combined hardware smoke test, show the panel and confirm the laser appears while either controller is aimed at it and button hover follows the target. Press trigger on `SLEEP`, `WAKE`, `DARKER`, and `BRIGHT`; trigger should activate UI only and should never move the panel. Then aim at the panel, hold the controller grip/squeeze, move and rotate your hand across all axes, and release. Confirm the panel follows position and orientation without an obvious jump, remains world-locked and undimmed after release, and restores the committed position and orientation after an overlay restart when `RestoreLastPosition` is used. With `--hydra`, the first successful button activation or grab per overlay process emits the one-shot `[Hydra/Input] I'M FIRIN' MAH LAZER` Easter egg; normal button and grab diagnostics remain secret-free.

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
