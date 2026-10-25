from pathlib import Path

root = Path("crates/nightshift-overlay-prototype/src")

# Split the overlay executable into entrypoint, IPC, runtime, and runtime-state modules.
path = root / "main.rs"
text = path.read_text()
state_start = text.index("#[derive(Clone, Copy, Debug, PartialEq)]\nstruct OverlayTarget")
main_start = text.index("fn main() -> Result<()>")
ipc_start = text.index("fn socket_path() -> Result<PathBuf>")
runtime_start = text.index("fn protocol_pose(")
tests_start = text.index("#[cfg(test)]\nmod tests")

state = text[state_start:main_start]
state = state.replace("struct OverlayTarget {", "pub(crate) struct OverlayTarget {", 1)
state = state.replace("enum RuntimeUpdate {", "pub(crate) enum RuntimeUpdate {", 1)
state = state.replace("fn sanitize_brightness(", "pub(crate) fn sanitize_brightness(", 1)
state = state.replace("fn sanitize_transition_seconds(", "pub(crate) fn sanitize_transition_seconds(", 1)
state = state.replace("fn overlay_target(", "pub(crate) fn overlay_target(", 1)
state = state.replace("fn brightness_target(", "pub(crate) fn brightness_target(", 1)
state = state.replace(
    "fn overlay_pose_report_if_changed(",
    "pub(crate) fn overlay_pose_report_if_changed(",
    1,
)
state = state.replace("struct Fade {", "pub(crate) struct Fade {", 1)
state = state.replace("    brightness: f32,", "    pub(crate) brightness: f32,", 1)
state = state.replace(
    "    transition_seconds: f32,",
    "    pub(crate) transition_seconds: f32,",
    1,
)
for field in ("start", "target", "started", "duration_seconds"):
    state = state.replace(f"    {field}: ", f"    pub(crate) {field}: ", 1)
state = state.replace("    fn new(", "    pub(crate) fn new(", 1)
state = state.replace("    fn sample(", "    pub(crate) fn sample(", 1)

runtime_state = """use std::time::Instant;

use nightshift_protocol::{BrightnessState, WorkerReport};

use crate::{
    content::PanelContent,
    placement::PanelPose,
    surface::{SurfaceTarget, panel_pose_to_protocol},
};

pub(crate) const DEFAULT_TRANSITION_SECONDS: f32 = 3.0;
pub(crate) const MAX_TRANSITION_SECONDS: f32 = 120.0;

""" + state
(root / "runtime_state.rs").write_text(runtime_state)

ipc = text[ipc_start:runtime_start]
ipc = ipc.replace("fn hidden_surface_target(", "pub(crate) fn hidden_surface_target(", 1)
ipc = ipc.replace("fn spawn_ipc_client(", "pub(crate) fn spawn_ipc_client(", 1)
ipc_file = """use std::env;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{Receiver, Sender},
};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use nightshift_protocol::{
    AppState, ClientMessage, PROTOCOL_VERSION, ServerMessage, StateEvent, UiCommand, WorkerReport,
};

use crate::panel_state::PanelStateTracker;
use crate::runtime_state::{
    DEFAULT_TRANSITION_SECONDS, RuntimeUpdate, brightness_target, sanitize_transition_seconds,
};
use crate::surface::SurfaceTarget;

const RECONNECT_DELAY: Duration = Duration::from_secs(1);

""" + ipc
(root / "ipc.rs").write_text(ipc_file)

runtime = text[runtime_start:tests_start]
runtime = runtime.replace("fn run_overlay(", "pub(crate) fn run_overlay(", 1)
runtime_file = """use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{Receiver, Sender},
};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use ash::vk::{self, Handle};
use nightshift_pose::{PoseTracker, SleepingPose as DetectedPose};
use nightshift_protocol::{AppState, SleepingPose, UiCommand, WorkerReport};
use openxr as xr;
use xr::OverlaySessionCreateFlagsEXTX;
use xr::sys::Handle as XrHandle;

use crate::content::{PanelContent, SocialTab};
use crate::controller_input::{ControllerHand, ControllerInput, select_active_pointer_hand};
use crate::interaction::{
    PanelAction, PanelGrab, PanelPage, action_at_uv_for_page, begin_panel_grab,
    hit_test_panel_with_distance, update_panel_grab,
};
use crate::ipc::hidden_surface_target;
use crate::laser::{LaserRenderer, LaserSegment, PointerSmoother, smoothing_alpha};
use crate::panel::{PANEL_HEIGHT_M, PANEL_WIDTH_M, PanelRenderer};
use crate::panel_cache::PanelBitmapCache;
use crate::placement::PanelPose;
use crate::runtime_state::{
    Fade, RuntimeUpdate, overlay_pose_report_if_changed, sanitize_brightness,
    sanitize_transition_seconds,
};
use crate::surface::SurfaceController;

const VIEW_TYPE: xr::ViewConfigurationType = xr::ViewConfigurationType::PRIMARY_STEREO;
const VIEW_COUNT: u32 = 2;
const DIMMER_TEXTURE_SIZE: u32 = 4;
pub(crate) const DEFAULT_PLACEMENT: u32 = 4;

""" + runtime
(root / "runtime.rs").write_text(runtime_file)

main_body = text[main_start:ipc_start]
tests = text[tests_start:]
new_main = """mod content;
mod controller_input;
mod interaction;
mod ipc;
mod laser;
mod panel;
mod panel_cache;
mod panel_state;
mod placement;
mod raster;
mod runtime;
mod runtime_state;
mod surface;

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

""" + main_body + """
#[cfg(test)]
use ipc::hidden_surface_target;
#[cfg(test)]
use nightshift_protocol::{BrightnessState, WorkerReport};
#[cfg(test)]
use placement::PanelPose;
#[cfg(test)]
use runtime_state::{
    Fade, MAX_TRANSITION_SECONDS, OverlayTarget, brightness_target,
    overlay_pose_report_if_changed, sanitize_brightness, sanitize_transition_seconds,
};
#[cfg(test)]
use surface::panel_pose_to_protocol;

""" + tests
path.write_text(new_main)

# Split raster drawing into page/layout, primitives, and font modules.
path = root / "raster.rs"
text = path.read_text()
home_start = text.index("    fn draw_home(")
settings_start = text.index("    fn draw_settings(")
primitives_start = text.index("    fn rounded_rect(")
impl_end = text.index("\n}\n\nfn social_initial", primitives_start)
social_start = text.index("fn social_initial", impl_end)
glyph_start = text.index("fn glyph5x7", social_start)
tests_start = text.index("#[cfg(test)]", glyph_start)

prefix = text[:home_start]
prefix = prefix.replace("content::{PanelContent, SocialTab}", "content::PanelContent", 1)
prefix = "mod font;\nmod home;\nmod primitives;\nmod settings;\n\n" + prefix
path.write_text(prefix + "}\n\n" + text[tests_start:])

home = text[home_start:settings_start]
home = home.replace("    fn draw_home(", "    pub(super) fn draw_home(", 1)
(root / "raster").mkdir(exist_ok=True)
(root / "raster" / "home.rs").write_text("use super::*;\n\nimpl PanelBitmap {\n" + home + "}\n")

settings = text[settings_start:primitives_start]
settings = settings.replace("    fn draw_settings(", "    pub(super) fn draw_settings(", 1)
social_helpers = text[social_start:glyph_start]
(root / "raster" / "settings.rs").write_text(
    "use super::*;\n\nimpl PanelBitmap {\n" + settings + "}\n\n" + social_helpers
)

primitives = text[primitives_start:impl_end]
primitives = primitives.replace("    fn ", "    pub(super) fn ")
(root / "raster" / "primitives.rs").write_text(
    "use super::{PanelBitmap, font::glyph5x7};\n\nimpl PanelBitmap {\n" + primitives + "}\n"
)

glyph = text[glyph_start:tests_start]
glyph = glyph.replace("fn glyph5x7(", "pub(super) fn glyph5x7(", 1)
(root / "raster" / "font.rs").write_text(glyph)
