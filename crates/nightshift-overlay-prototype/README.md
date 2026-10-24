# Nightshift OpenXR overlay worker

This crate hosts Nightshift's OpenXR compositor worker for Monado/WiVRn setups. It creates an `XR_EXTX_overlay` session at placement `4` and currently has three responsibilities:

- submit the tiny stereo projection-layer sleep dimmer
- report HMD orientation to the daemon's sleeping-pose detector
- submit a world-locked positioned `CompositionLayerQuad` controlled by canonical overlay state and populated with live Nightshift status

Current WayVR uses a higher overlay placement, so WayVR can continue to render above Nightshift while both are running.

## Run

Start WiVRn/Monado and connect the headset. Start the Nightshift daemon, then launch the overlay worker:

```fish
cargo run -p nightshift-daemon
cargo run -p nightshift-overlay --release
```

In another terminal, `nightshiftctl` can exercise the positioned surface:

```fish
cargo run -p nightshiftctl -- overlay-distance 1.25
cargo run -p nightshiftctl -- overlay-mode in-front
cargo run -p nightshiftctl -- overlay-show
cargo run -p nightshiftctl -- status
cargo run -p nightshiftctl -- overlay-hide
```

The same visibility, distance, and spawn-mode controls are available in the desktop Settings view.

## Expected positioned-panel behavior

- `overlay-show` captures a headset-centered VIEW pose at the configured distance and submits the panel in LOCAL/world space.
- Moving your head after showing the panel does not make it follow your head.
- `overlay-hide` removes the quad layer from subsequent rendered frames.
- `restore-last-position` reuses the most recent valid pose remembered by the running overlay process.
- If there is no remembered pose, restore mode falls back to spawning in front of the headset.
- Invalid HMD tracking does not create a malformed panel.
- A daemon IPC disconnect fails the positioned panel closed to hidden.

The remembered panel pose is currently process-local. Restarting the overlay worker clears it; persistent transforms and direct grabbing/repositioning are future work.

## Live panel content

The positioned panel renders a small CPU-generated status bitmap from canonical daemon state. It currently shows:

- Nightshift title
- sleep phase, including HR-detected sleep context
- current brightness and transition target
- heart-rate/provider state
- VRChat connectivity/status
- OSC and invite-automation state

Daemon snapshots and state events update an internal panel-state mirror. The bitmap is regenerated only when those visible lines change; unrelated state changes do not request a redraw, and identical content is deduplicated before any Vulkan upload. If the OpenXR session is restarted, the cache is invalidated so the latest status is uploaded again.

When testing on hardware, verify that changing sleep state, brightness, heart rate, VRChat status, OSC, or invite automation updates the corresponding line. Hide the panel, change state, then show it again and confirm the newest state appears rather than the value that was visible when it was hidden.

## Expected dimmer/ordering behavior

Use the normal Nightshift brightness/sleep controls while VRChat is visible. The dimmer remains a tiny black stereo projection layer with source-alpha blending.

Expected compositor behavior:

- VRChat / the main OpenXR session dims according to canonical Nightshift brightness.
- The positioned Nightshift panel is submitted after Nightshift's dimmer and remains undimmed.
- WayVR remains at normal brightness when running above Nightshift's placement-4 session.
- Moving your head does not uncover the full-screen dimmer.
- Ctrl+C removes Nightshift's OpenXR layers cleanly.

## Automated checks

CI runs:

```fish
cargo test -p nightshift-overlay
```

This covers placement geometry, finite-value validation, show/hide/restore state behavior, panel content generation, bitmap rasterization and deduplication, upload-format handling, and the quad-rendering API boundary. CI installs the OpenXR loader development package so these tests are linked and executed rather than only compile-checked.
