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
- `restore-last-position` first reuses the most recent valid pose remembered by the running overlay process, then falls back to the valid pose persisted by the daemon.
- If there is no remembered pose, restore mode falls back to spawning in front of the headset.
- Valid newly captured poses are reported to the daemon only when the remembered pose changes; the daemon normalizes and persists them.
- Invalid or non-finite pose reports are ignored and never replace the last valid persisted pose.
- Visibility is deliberately not persisted: restarting the overlay worker does not make a hidden panel reappear by itself.
- A daemon IPC disconnect fails the positioned panel closed to hidden.

### Restart/restore hardware check

1. Set mode to `in-front`, show the panel, and confirm it appears at the configured distance.
2. Move your head and confirm the panel stays world-locked.
3. Hide the panel, stop only the overlay worker, and start the overlay worker again while leaving the daemon running.
4. Set mode to `restore-last-position` and show the panel. Confirm it returns to the same remembered world pose rather than spawning at the current headset position.
5. Hide the panel again, restart the daemon and overlay worker, select `restore-last-position`, and show the panel. Confirm the daemon-restored persisted pose is still used.
6. During either restart, confirm the panel remains hidden until an explicit show command is issued.

This persistence stores only the last validated world pose. Direct grabbing/repositioning is still future work; once such movement exists, the same change-deduplicated report path can persist the new parking spot.

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

## OpenXR anti-hang hardware readiness check

Run this check before treating swapchain-wait changes as headset-validated. Record the Nightshift commit, OpenXR runtime, headset, and whether VRChat/WayVR are running so failures can be reproduced.

1. Launch the daemon and release overlay worker with the headset already connected. Confirm the dimmer, positioned panel, and controller laser/cursor all render and remain responsive.
2. Exercise `overlay-show` and `overlay-hide` repeatedly while changing brightness. Confirm there are no multi-second stalls, frozen layers, or compositor instability.
3. Leave the panel visible and move the controller across interactive targets. Confirm panel and laser swapchains continue updating while the dimmer is active.
4. Interrupt the OpenXR lifecycle once by disconnecting/reconnecting the headset or stopping/restarting the compositor, whichever is normal for the test setup. The overlay may exit on session/runtime loss, but it must not remain hung indefinitely.
5. If a swapchain wait is forced to time out, confirm the worker reports a label-specific `timed out waiting for OpenXR swapchain image` error after the bounded retry budget instead of freezing. The retry budget is eight waits of 250 ms each, and retries stay on the already acquired image.
6. After any timeout, runtime loss, or deliberate Ctrl+C exit, start the overlay worker again. Confirm it reconnects cleanly and panel/dimmer rendering resumes without needing to restart the Nightshift daemon.
7. Repeat a short show/hide/dim cycle after recovery and confirm VRChat/WayVR remain responsive and layer ordering is unchanged.

Acceptance criteria: no indefinite swapchain wait, no acquire-on-retry behavior, no rendering or release before wait readiness, actionable error output on failure, clean process recovery, and unchanged normal rendering behavior.

## Automated checks

CI runs:

```fish
cargo test -p nightshift-overlay
```

This covers placement geometry, finite-value validation, show/hide/restore state behavior, persisted-pose restore precedence, change-deduplicated pose reporting, panel content generation, bitmap rasterization and deduplication, upload-format handling, and the quad-rendering API boundary. CI installs the OpenXR loader development package so these tests are linked and executed rather than only compile-checked.
