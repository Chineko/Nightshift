# Nightshift OpenXR dimmer prototype

This crate exists only to validate compositor ordering with Monado/WiVRn and WayVR.

It creates an `XR_EXTX_overlay` session at placement `4` (WayVR currently uses placement `5`) and submits a tiny 4x4 black stereo projection layer with source-alpha blending. The compositor stretches the layer over the normal stereo views, so the GPU workload is intentionally tiny.

## Test

Start WiVRn and connect the headset, launch VRChat, then launch WayVR normally so a WayVR panel is visible.

Run:

```fish
cargo run -p nightshift-overlay-prototype --release -- --brightness 0.20
```

Expected result:

- VRChat / the main OpenXR session becomes roughly 20% as bright.
- WayVR stays at normal brightness.
- Moving your head does not uncover the dimmer.
- Ctrl+C removes the dimmer cleanly.

Useful variants:

```fish
cargo run -p nightshift-overlay-prototype --release -- --brightness 0.50
cargo run -p nightshift-overlay-prototype --release -- --brightness 0.06
cargo run -p nightshift-overlay-prototype --release -- --brightness 1.00
```

To experiment with compositor ordering:

```fish
cargo run -p nightshift-overlay-prototype --release -- --brightness 0.20 --placement 4
```

If placement 4 unexpectedly lands above WayVR, try 3 and 6 and record which components dim. This is prototype code; once ordering is confirmed, brightness/fade control will move behind the Nightshift daemon and the overlay renderer will stop repainting unchanged pixels every frame.
