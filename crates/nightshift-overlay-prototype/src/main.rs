mod content;
mod controller_input;
mod home_layout;
mod interaction;
mod ipc;
mod laser;
mod layout;
mod panel;
mod panel_cache;
mod panel_state;
mod placement;
mod raster;
mod runtime;
mod runtime_state;
mod surface;
mod swapchain_wait;
mod widget;

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

use anyhow::{Context, Result, anyhow};

use ipc::spawn_ipc_client;
use runtime::{DEFAULT_PLACEMENT, run_overlay};
use runtime_state::DEFAULT_TRANSITION_SECONDS;

// Hydra IPC diagnostics live in ipc.rs; keep the [Hydra/IPC] contract healthy.

fn main() -> Result<()> {
    let placement = parse_flag_u32("--placement")?.unwrap_or(DEFAULT_PLACEMENT);
    let hydra_mode = has_flag("--hydra-mode");

    println!("Nightshift OpenXR overlay");
    println!("  placement  : {placement}");
    println!("  daemon IPC : enabled");
    println!("  fallback fade default : {DEFAULT_TRANSITION_SECONDS:.1}s");
    println!(
        "  hydra mode : {}",
        if hydra_mode { "AWAKENED" } else { "sleeping" }
    );
    println!("  Ctrl+C to stop");
    println!();
    println!("The tiny evil square now takes orders from the daemon.");
    if hydra_mode {
        println!("Hydra has entered the chat. Terminal dignity is no longer guaranteed.");
    }

    let running = Arc::new(AtomicBool::new(true));
    let signal = running.clone();
    ctrlc::set_handler(move || signal.store(false, Ordering::Relaxed))
        .context("installing Ctrl+C handler")?;

    let (updates_tx, updates_rx) = mpsc::channel();
    let (reports_tx, reports_rx) = mpsc::channel();
    let (commands_tx, commands_rx) = mpsc::channel();
    spawn_ipc_client(
        running.clone(),
        updates_tx,
        reports_rx,
        commands_rx,
        hydra_mode,
    );

    run_overlay(
        running,
        updates_rx,
        reports_tx,
        commands_tx,
        placement,
        hydra_mode,
    )
}

fn parse_flag_u32(name: &str) -> Result<Option<u32>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let mut index = 0;

    while index < args.len() {
        let arg = &args[index];
        if arg == name {
            let value = args
                .get(index + 1)
                .ok_or_else(|| anyhow!("{name} requires a value"))?;
            return value
                .parse::<u32>()
                .map(Some)
                .map_err(|error| anyhow!("invalid {name} value {value:?}: {error}"));
        }

        if let Some(value) = arg.strip_prefix(&format!("{name}=")) {
            return value
                .parse::<u32>()
                .map(Some)
                .map_err(|error| anyhow!("invalid {name} value {value:?}: {error}"));
        }

        index += 1;
    }

    Ok(None)
}

fn has_flag(name: &str) -> bool {
    std::env::args().skip(1).any(|arg| arg == name)
}

#[cfg(test)]
use ipc::hidden_surface_target;
#[cfg(test)]
use nightshift_protocol::{BrightnessState, WorkerReport};
#[cfg(test)]
use placement::PanelPose;
#[cfg(test)]
use runtime_state::{
    Fade, MAX_TRANSITION_SECONDS, OverlayTarget, brightness_target, overlay_pose_report_if_changed,
    sanitize_brightness, sanitize_transition_seconds,
};
#[cfg(test)]
use surface::panel_pose_to_protocol;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brightness_targets_follow_transition_state() {
        let transitioning = BrightnessState {
            current: 0.9,
            target: 0.1,
            sleep_target: 0.06,
            transitioning: true,
            manual_override: false,
        };
        assert_eq!(
            brightness_target(&transitioning, 4.0),
            OverlayTarget {
                brightness: 0.1,
                transition_seconds: 4.0,
            }
        );

        let settled = BrightnessState {
            current: 0.65,
            target: 0.1,
            sleep_target: 0.06,
            transitioning: false,
            manual_override: true,
        };
        assert_eq!(
            brightness_target(&settled, 4.0),
            OverlayTarget {
                brightness: 0.65,
                transition_seconds: 0.0,
            }
        );
    }

    #[test]
    fn overlay_targets_reject_non_finite_values_and_bound_ranges() {
        assert_eq!(sanitize_brightness(f32::NAN), 1.0);
        assert_eq!(sanitize_brightness(f32::INFINITY), 1.0);
        assert_eq!(sanitize_brightness(-5.0), 0.02);
        assert_eq!(sanitize_brightness(8.0), 1.0);

        assert_eq!(sanitize_transition_seconds(f32::NAN), 0.0);
        assert_eq!(sanitize_transition_seconds(f32::INFINITY), 0.0);
        assert_eq!(sanitize_transition_seconds(-1.0), 0.0);
        assert_eq!(sanitize_transition_seconds(999.0), MAX_TRANSITION_SECONDS);
    }

    #[test]
    fn overlay_pose_reports_are_deduplicated() {
        let first = PanelPose {
            position: [1.0, 2.0, 3.0],
            orientation_xyzw: [0.0, 0.0, 0.0, 1.0],
        };
        let second = PanelPose {
            position: [4.0, 5.0, 6.0],
            orientation_xyzw: [0.0, 0.0, 0.0, 1.0],
        };
        let mut reported = None;

        assert_eq!(overlay_pose_report_if_changed(&mut reported, None), None);
        assert_eq!(
            overlay_pose_report_if_changed(&mut reported, Some(first)),
            Some(WorkerReport::OverlayRuntimePose {
                pose: panel_pose_to_protocol(first),
            })
        );
        assert_eq!(overlay_pose_report_if_changed(&mut reported, Some(first)), None);
        assert_eq!(
            overlay_pose_report_if_changed(&mut reported, Some(second)),
            Some(WorkerReport::OverlayRuntimePose {
                pose: panel_pose_to_protocol(second),
            })
        );
    }

    #[test]
    fn fade_constructor_sanitizes_inputs() {
        let fade = Fade::new(f32::NAN, f32::NEG_INFINITY, f32::INFINITY);
        assert_eq!(fade.start, 1.0);
        assert_eq!(fade.target, 1.0);
        assert_eq!(fade.duration_seconds, 0.0);
    }

    #[test]
    fn hidden_surface_target_is_safe_by_default() {
        let target = hidden_surface_target();
        assert!(!target.visible);
        assert_eq!(target.distance_m, 1.0);
    }
}
