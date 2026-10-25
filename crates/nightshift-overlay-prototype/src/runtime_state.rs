use std::time::Instant;

use nightshift_protocol::{BrightnessState, WorkerReport};

use crate::{
    content::PanelContent,
    placement::PanelPose,
    surface::{SurfaceTarget, panel_pose_to_protocol},
};

pub(crate) const DEFAULT_TRANSITION_SECONDS: f32 = 3.0;
pub(crate) const MAX_TRANSITION_SECONDS: f32 = 120.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct OverlayTarget {
    pub(crate) brightness: f32,
    pub(crate) transition_seconds: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum RuntimeUpdate {
    Brightness(OverlayTarget),
    Surface(SurfaceTarget),
    Panel(PanelContent),
}

pub(crate) fn sanitize_brightness(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.02, 1.0)
    } else {
        1.0
    }
}

pub(crate) fn sanitize_transition_seconds(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, MAX_TRANSITION_SECONDS)
    } else {
        0.0
    }
}

pub(crate) fn overlay_target(brightness: f32, transition_seconds: f32) -> OverlayTarget {
    OverlayTarget {
        brightness: sanitize_brightness(brightness),
        transition_seconds: sanitize_transition_seconds(transition_seconds),
    }
}

pub(crate) fn brightness_target(brightness: &BrightnessState, fade_seconds: f32) -> OverlayTarget {
    if brightness.transitioning {
        overlay_target(brightness.target, fade_seconds)
    } else {
        overlay_target(brightness.current, 0.0)
    }
}

pub(crate) fn overlay_pose_report_if_changed(
    reported: &mut Option<PanelPose>,
    current: Option<PanelPose>,
) -> Option<WorkerReport> {
    if *reported == current {
        return None;
    }
    *reported = current;
    current.map(|pose| WorkerReport::OverlayRuntimePose {
        pose: panel_pose_to_protocol(pose),
    })
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Fade {
    pub(crate) start: f32,
    pub(crate) target: f32,
    pub(crate) started: Instant,
    pub(crate) duration_seconds: f32,
}

impl Fade {
    pub(crate) fn new(start: f32, target: f32, duration_seconds: f32) -> Self {
        Self {
            start: sanitize_brightness(start),
            target: sanitize_brightness(target),
            started: Instant::now(),
            duration_seconds: sanitize_transition_seconds(duration_seconds),
        }
    }

    pub(crate) fn sample(self) -> (f32, f32, bool) {
        if self.duration_seconds <= f32::EPSILON {
            return (self.target, 1.0, true);
        }

        let linear = (self.started.elapsed().as_secs_f32() / self.duration_seconds).clamp(0.0, 1.0);
        let smooth = linear * linear * (3.0 - 2.0 * linear);
        let brightness = self.start + (self.target - self.start) * smooth;
        (brightness, linear, linear >= 1.0)
    }
}
