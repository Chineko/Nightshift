mod content;
mod panel;
mod panel_cache;
mod panel_state;
mod placement;
mod raster;
mod surface;

use std::env;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, Sender},
};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use ash::vk::{self, Handle};
use nightshift_pose::{PoseTracker, SleepingPose as DetectedPose};
use nightshift_protocol::{
    AppState, BrightnessState, ClientMessage, PROTOCOL_VERSION, ServerMessage, SleepingPose,
    StateEvent, WorkerReport,
};
use openxr as xr;
use xr::OverlaySessionCreateFlagsEXTX;
use xr::sys::Handle as XrHandle;

use content::PanelContent;
use panel::PanelRenderer;
use panel_cache::PanelBitmapCache;
use panel_state::PanelStateTracker;
use placement::PanelPose;
use surface::{SurfaceController, SurfaceTarget, panel_pose_to_protocol};

const VIEW_TYPE: xr::ViewConfigurationType = xr::ViewConfigurationType::PRIMARY_STEREO;
const VIEW_COUNT: u32 = 2;
const DIMMER_TEXTURE_SIZE: u32 = 4;
const DEFAULT_PLACEMENT: u32 = 4;
const DEFAULT_TRANSITION_SECONDS: f32 = 3.0;
const MAX_TRANSITION_SECONDS: f32 = 120.0;
const RECONNECT_DELAY: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Debug, PartialEq)]
struct OverlayTarget {
    brightness: f32,
    transition_seconds: f32,
}

#[derive(Clone, Debug, PartialEq)]
enum RuntimeUpdate {
    Brightness(OverlayTarget),
    Surface(SurfaceTarget),
    Panel(PanelContent),
}

fn sanitize_brightness(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.02, 1.0)
    } else {
        1.0
    }
}

fn sanitize_transition_seconds(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, MAX_TRANSITION_SECONDS)
    } else {
        0.0
    }
}

fn overlay_target(brightness: f32, transition_seconds: f32) -> OverlayTarget {
    OverlayTarget {
        brightness: sanitize_brightness(brightness),
        transition_seconds: sanitize_transition_seconds(transition_seconds),
    }
}

fn brightness_target(brightness: &BrightnessState, fade_seconds: f32) -> OverlayTarget {
    if brightness.transitioning {
        overlay_target(brightness.target, fade_seconds)
    } else {
        overlay_target(brightness.current, 0.0)
    }
}

fn overlay_pose_report_if_changed(
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
struct Fade {
    start: f32,
    target: f32,
    started: Instant,
    duration_seconds: f32,
}

impl Fade {
    fn new(start: f32, target: f32, duration_seconds: f32) -> Self {
        Self {
            start: sanitize_brightness(start),
            target: sanitize_brightness(target),
            started: Instant::now(),
            duration_seconds: sanitize_transition_seconds(duration_seconds),
        }
    }

    fn sample(self) -> (f32, f32, bool) {
        if self.duration_seconds <= f32::EPSILON {
            return (self.target, 1.0, true);
        }

        let linear = (self.started.elapsed().as_secs_f32() / self.duration_seconds).clamp(0.0, 1.0);
        let smooth = linear * linear * (3.0 - 2.0 * linear);
        let brightness = self.start + (self.target - self.start) * smooth;
        (brightness, linear, linear >= 1.0)
    }
}

fn main() -> Result<()> {
    let placement = parse_flag_u32("--placement")?.unwrap_or(DEFAULT_PLACEMENT);
    let hydra_mode = has_flag("--hydra-mode");

    println!("Nightshift OpenXR overlay");
    println!("  placement  : {placement}");
    println!("  daemon IPC : enabled");
    println!("  fallback fade default : {DEFAULT_TRANSITION_SECONDS:.1}s");
    println!("  hydra mode : {}", if hydra_mode { "AWAKENED" } else { "sleeping" });
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
    spawn_ipc_client(running.clone(), updates_tx, reports_rx, hydra_mode);

    run_overlay(running, updates_rx, reports_tx, placement, hydra_mode)
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

fn socket_path() -> Result<PathBuf> {
    let runtime_dir = env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .context("XDG_RUNTIME_DIR is not set")?;
    Ok(runtime_dir.join("nightshift").join("nightshift.sock"))
}

fn hidden_surface_target() -> SurfaceTarget {
    SurfaceTarget::from(&nightshift_protocol::OverlayState::default())
}

fn spawn_ipc_client(
    running: Arc<AtomicBool>,
    updates: Sender<RuntimeUpdate>,
    reports: Receiver<WorkerReport>,
    hydra_mode: bool,
) {
    thread::spawn(move || {
        let path = match socket_path() {
            Ok(path) => path,
            Err(error) => {
                eprintln!("Nightshift overlay IPC disabled: {error:#}");
                return;
            }
        };

        let mut announced_waiting = false;

        while running.load(Ordering::Relaxed) {
            match UnixStream::connect(&path) {
                Ok(stream) => {
                    announced_waiting = false;
                    println!("Nightshift overlay connected to daemon IPC.");
                    if let Err(error) =
                        run_ipc_connection(stream, &updates, &reports, &running, hydra_mode)
                    {
                        if running.load(Ordering::Relaxed) {
                            eprintln!("Nightshift overlay IPC disconnected: {error:#}");
                        }
                    }
                    let _ = updates.send(RuntimeUpdate::Surface(hidden_surface_target()));
                }
                Err(error) => {
                    if !announced_waiting {
                        eprintln!(
                            "Nightshift overlay waiting for daemon at {} ({error})",
                            path.display()
                        );
                        announced_waiting = true;
                    }
                }
            }

            if running.load(Ordering::Relaxed) {
                thread::sleep(RECONNECT_DELAY);
            }
        }
    });
}

fn write_client_message(writer: &mut UnixStream, message: &ClientMessage) -> Result<()> {
    let mut json = serde_json::to_vec(message).context("serializing daemon IPC message")?;
    json.push(b'\n');
    writer.write_all(&json).context("sending daemon IPC message")?;
    writer.flush().context("flushing daemon IPC message")?;
    Ok(())
}

fn flush_worker_reports(
    writer: &mut UnixStream,
    reports: &Receiver<WorkerReport>,
    hydra_mode: bool,
) -> Result<()> {
    while let Ok(report) = reports.try_recv() {
        if hydra_mode {
            println!("[Hydra/Pose] report={report:?}");
        }
        write_client_message(writer, &ClientMessage::Report { report })?;
    }
    Ok(())
}

fn run_ipc_connection(
    stream: UnixStream,
    updates: &Sender<RuntimeUpdate>,
    reports: &Receiver<WorkerReport>,
    running: &AtomicBool,
    hydra_mode: bool,
) -> Result<()> {
    stream
        .set_read_timeout(Some(Duration::from_millis(250)))
        .context("setting daemon IPC read timeout")?;

    let mut writer = stream.try_clone().context("cloning daemon IPC socket")?;
    write_client_message(
        &mut writer,
        &ClientMessage::Hello {
            protocol: PROTOCOL_VERSION,
            client: "nightshift-overlay".into(),
            client_version: env!("CARGO_PKG_VERSION").into(),
        },
    )?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    let mut fade_seconds = DEFAULT_TRANSITION_SECONDS;
    let mut panel_state = PanelStateTracker::default();

    while running.load(Ordering::Relaxed) {
        flush_worker_reports(&mut writer, reports, hydra_mode)?;
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => bail!("daemon closed the socket"),
            Ok(_) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                continue;
            }
            Err(error) => return Err(error).context("reading daemon IPC"),
        }

        let message: ServerMessage =
            serde_json::from_str(line.trim_end()).context("decoding daemon IPC message")?;

        match message {
            ServerMessage::Hello { protocol, .. } => {
                if protocol != PROTOCOL_VERSION {
                    bail!(
                        "daemon protocol mismatch: overlay={}, daemon={protocol}",
                        PROTOCOL_VERSION
                    );
                }
                if hydra_mode {
                    println!("[Hydra/IPC] handshake=accepted protocol={protocol}");
                }
            }
            ServerMessage::FullState { state } => {
                fade_seconds = sanitize_transition_seconds(state.settings.fade_duration_seconds);
                panel_state = PanelStateTracker::from_state(state.clone());
                send_state_targets(updates, &state, fade_seconds)?;
                updates
                    .send(RuntimeUpdate::Panel(panel_state.content().clone()))
                    .map_err(|_| anyhow!("overlay renderer stopped"))?;
                if hydra_mode {
                    println!(
                        "[Hydra/IPC] snapshot target={:.2}% fade={:.2}s surface_visible={}",
                        state.brightness.target * 100.0,
                        fade_seconds,
                        state.overlay.visible
                    );
                }
            }
            ServerMessage::Event { event } => {
                if let Some(content) = panel_state.apply_event(&event) {
                    updates
                        .send(RuntimeUpdate::Panel(content))
                        .map_err(|_| anyhow!("overlay renderer stopped"))?;
                }

                match event {
                    StateEvent::BrightnessChanged(brightness) => {
                        let target = brightness_target(&brightness, fade_seconds);
                        updates
                            .send(RuntimeUpdate::Brightness(target))
                            .map_err(|_| anyhow!("overlay renderer stopped"))?;

                        if hydra_mode {
                            println!(
                                "[Hydra/IPC] brightness target={:.2}% transition={:.2}s daemon_transitioning={}",
                                target.brightness * 100.0,
                                target.transition_seconds,
                                brightness.transitioning
                            );
                        }
                    }
                    StateEvent::OverlayChanged(overlay) => {
                        let target = SurfaceTarget::from(&overlay);
                        updates
                            .send(RuntimeUpdate::Surface(target))
                            .map_err(|_| anyhow!("overlay renderer stopped"))?;
                        if hydra_mode {
                            println!(
                                "[Hydra/IPC] surface visible={} distance={:.2}m mode={:?}",
                                target.visible, target.distance_m, target.spawn_mode
                            );
                        }
                    }
                    StateEvent::SettingsChanged(settings) => {
                        fade_seconds = sanitize_transition_seconds(settings.fade_duration_seconds);
                        if hydra_mode {
                            println!("[Hydra/IPC] fade default={fade_seconds:.2}s");
                        }
                    }
                    _ => {}
                }
            }
            ServerMessage::Error { message } => bail!("daemon rejected overlay: {message}"),
        }
    }

    Ok(())
}

fn send_state_targets(
    updates: &Sender<RuntimeUpdate>,
    state: &AppState,
    fade_seconds: f32,
) -> Result<()> {
    updates
        .send(RuntimeUpdate::Brightness(brightness_target(
            &state.brightness,
            fade_seconds,
        )))
        .map_err(|_| anyhow!("overlay renderer stopped"))?;
    updates
        .send(RuntimeUpdate::Surface(SurfaceTarget::from(&state.overlay)))
        .map_err(|_| anyhow!("overlay renderer stopped"))
}

fn protocol_pose(pose: DetectedPose) -> SleepingPose {
    match pose {
        DetectedPose::Unknown => SleepingPose::Unknown,
        DetectedPose::Front => SleepingPose::Front,
        DetectedPose::Back => SleepingPose::Back,
        DetectedPose::Left => SleepingPose::Left,
        DetectedPose::Right => SleepingPose::Right,
    }
}

fn report_pose(reports: &Sender<WorkerReport>, pose: SleepingPose) {
    let _ = reports.send(WorkerReport::SleepingPose { pose });
}

#[allow(clippy::too_many_lines)]
fn run_overlay(
    running: Arc<AtomicBool>,
    updates: Receiver<RuntimeUpdate>,
    reports: Sender<WorkerReport>,
    placement: u32,
    hydra_mode: bool,
) -> Result<()> {
    let entry = xr::Entry::linked();
    let available = entry
        .enumerate_extensions()
        .context("enumerating OpenXR extensions")?;

    if !available.khr_vulkan_enable2 {
        bail!("OpenXR runtime does not expose XR_KHR_vulkan_enable2");
    }
    if !available.extx_overlay {
        bail!("OpenXR runtime does not expose XR_EXTX_overlay");
    }

    let mut enabled = xr::ExtensionSet::default();
    enabled.khr_vulkan_enable2 = true;
    enabled.extx_overlay = true;

    let xr_instance = entry
        .create_instance(
            &xr::ApplicationInfo {
                application_name: "nightshift-overlay",
                application_version: 1,
                engine_name: "nightshift",
                engine_version: 1,
                api_version: xr::Version::new(1, 0, 32),
            },
            &enabled,
            &[],
        )
        .context("creating OpenXR instance")?;

    let props = xr_instance.properties().context("querying OpenXR runtime")?;
    println!("OpenXR runtime: {} {}", props.runtime_name, props.runtime_version);

    let system = xr_instance
        .system(xr::FormFactor::HEAD_MOUNTED_DISPLAY)
        .context("getting HMD system")?;

    let blend_mode = xr_instance
        .enumerate_environment_blend_modes(system, VIEW_TYPE)
        .context("enumerating environment blend modes")?
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("runtime returned no environment blend modes"))?;

    let vk_target_version = vk::make_api_version(0, 1, 1, 0);
    let vk_target_version_xr = xr::Version::new(1, 1, 0);
    let reqs = xr_instance
        .graphics_requirements::<xr::Vulkan>(system)
        .context("querying Vulkan requirements")?;

    if vk_target_version_xr < reqs.min_api_version_supported
        || vk_target_version_xr.major() > reqs.max_api_version_supported.major()
    {
        bail!(
            "OpenXR runtime requires Vulkan >= {} and < {}.0.0",
            reqs.min_api_version_supported,
            reqs.max_api_version_supported.major() + 1
        );
    }

    unsafe {
        let vk_entry = ash::Entry::load().context("loading Vulkan loader")?;
        let vk_app_info = vk::ApplicationInfo::default()
            .application_version(1)
            .engine_version(1)
            .api_version(vk_target_version);
        let vk_instance_info = vk::InstanceCreateInfo::default().application_info(&vk_app_info);

        let raw_vk_instance = xr_instance
            .create_vulkan_instance(
                system,
                std::mem::transmute(vk_entry.static_fn().get_instance_proc_addr),
                (&raw const vk_instance_info).cast(),
            )
            .context("OpenXR creating Vulkan instance")?
            .map_err(vk::Result::from_raw)
            .context("Vulkan creating instance")?;

        let vk_instance = ash::Instance::load(
            vk_entry.static_fn(),
            vk::Instance::from_raw(raw_vk_instance as u64),
        );

        let raw_physical_device = xr_instance
            .vulkan_graphics_device(system, vk_instance.handle().as_raw() as _)
            .context("getting OpenXR Vulkan physical device")?;
        let vk_physical_device = vk::PhysicalDevice::from_raw(raw_physical_device as u64);

        let queue_family_index = vk_instance
            .get_physical_device_queue_family_properties(vk_physical_device)
            .iter()
            .enumerate()
            .find_map(|(index, info)| {
                info.queue_flags
                    .contains(vk::QueueFlags::GRAPHICS)
                    .then_some(index as u32)
            })
            .ok_or_else(|| anyhow!("Vulkan device has no graphics queue"))?;

        let priorities = [1.0_f32];
        let queue_infos = [vk::DeviceQueueCreateInfo::default()
            .queue_family_index(queue_family_index)
            .queue_priorities(&priorities)];
        let vk_device_info = vk::DeviceCreateInfo::default().queue_create_infos(&queue_infos);

        let raw_vk_device = xr_instance
            .create_vulkan_device(
                system,
                std::mem::transmute(vk_entry.static_fn().get_instance_proc_addr),
                vk_physical_device.as_raw() as _,
                (&raw const vk_device_info).cast(),
            )
            .context("OpenXR creating Vulkan device")?
            .map_err(vk::Result::from_raw)
            .context("Vulkan creating device")?;

        let vk_device = ash::Device::load(
            vk_instance.fp_v1_0(),
            vk::Device::from_raw(raw_vk_device as u64),
        );
        let queue = vk_device.get_device_queue(queue_family_index, 0);

        let raw_session = create_overlay_session(
            &xr_instance,
            system,
            &xr::vulkan::SessionCreateInfo {
                instance: vk_instance.handle().as_raw() as _,
                physical_device: vk_physical_device.as_raw() as _,
                device: vk_device.handle().as_raw() as _,
                queue_family_index,
                queue_index: 0,
            },
            placement,
        )?;

        let (session, mut frame_waiter, mut frame_stream) = xr::Session::<xr::Vulkan>::from_raw(
            xr_instance.clone(),
            raw_session,
            Box::new(()),
        );

        let local_space = session
            .create_reference_space(xr::ReferenceSpaceType::LOCAL, xr::Posef::IDENTITY)
            .context("creating LOCAL reference space")?;
        let view_space = session
            .create_reference_space(xr::ReferenceSpaceType::VIEW, xr::Posef::IDENTITY)
            .context("creating VIEW reference space")?;

        let formats = session
            .enumerate_swapchain_formats()
            .context("enumerating swapchain formats")?;
        let format = choose_color_format(&formats)?;
        println!("Vulkan format: {}", format.as_raw());

        let mut swapchain = session
            .create_swapchain(&xr::SwapchainCreateInfo {
                create_flags: xr::SwapchainCreateFlags::EMPTY,
                usage_flags: xr::SwapchainUsageFlags::COLOR_ATTACHMENT
                    | xr::SwapchainUsageFlags::TRANSFER_DST,
                format: format.as_raw() as u32,
                sample_count: 1,
                width: DIMMER_TEXTURE_SIZE,
                height: DIMMER_TEXTURE_SIZE,
                face_count: 1,
                array_size: VIEW_COUNT,
                mip_count: 1,
            })
            .context("creating 4x4 dimmer swapchain")?;

        let images = swapchain
            .enumerate_images()
            .context("enumerating swapchain images")?
            .into_iter()
            .map(vk::Image::from_raw)
            .collect::<Vec<_>>();
        let mut panel_renderer = PanelRenderer::new(&session, format)?;

        let command_pool = vk_device
            .create_command_pool(
                &vk::CommandPoolCreateInfo::default()
                    .queue_family_index(queue_family_index)
                    .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER),
                None,
            )
            .context("creating command pool")?;
        let command_buffer = vk_device
            .allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(command_pool)
                    .command_buffer_count(1),
            )
            .context("allocating command buffer")?[0];
        let fence = vk_device
            .create_fence(
                &vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED),
                None,
            )
            .context("creating render fence")?;

        let mut event_storage = xr::EventDataBuffer::new();
        let mut session_running = false;
        let mut current_brightness = 1.0_f32;
        let mut fade: Option<Fade> = None;
        let mut surface_target = hidden_surface_target();
        let mut surface_controller = SurfaceController::default();
        let mut reported_saved_pose = None;
        let mut panel_content = PanelContent::from_state(&AppState::default());
        let mut panel_cache = PanelBitmapCache::default();
        let mut hydra_frame = 0_u64;
        let pose_clock = Instant::now();
        let mut pose_tracker = PoseTracker::new(0);
        let rect = xr::Rect2Di {
            offset: xr::Offset2Di { x: 0, y: 0 },
            extent: xr::Extent2Di {
                width: DIMMER_TEXTURE_SIZE as i32,
                height: DIMMER_TEXTURE_SIZE as i32,
            },
        };
        let subresource = vk::ImageSubresourceRange {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            base_mip_level: 0,
            level_count: 1,
            base_array_layer: 0,
            layer_count: VIEW_COUNT,
        };

        'main_loop: while running.load(Ordering::Relaxed) {
            while let Some(event) = xr_instance
                .poll_event(&mut event_storage)
                .context("polling OpenXR events")?
            {
                match event {
                    xr::Event::SessionStateChanged(event) => match event.state() {
                        xr::SessionState::READY => {
                            session.begin(VIEW_TYPE).context("beginning overlay session")?;
                            session_running = true;
                            let now_ms = pose_clock.elapsed().as_millis() as u64;
                            pose_tracker = PoseTracker::new(now_ms);
                            panel_cache.invalidate();
                            println!("Overlay session running. Tiny evil square deployed.");
                        }
                        xr::SessionState::STOPPING => {
                            session.end().context("ending overlay session")?;
                            session_running = false;
                            report_pose(&reports, SleepingPose::Unknown);
                        }
                        xr::SessionState::EXITING | xr::SessionState::LOSS_PENDING => {
                            report_pose(&reports, SleepingPose::Unknown);
                            break 'main_loop;
                        }
                        _ => {}
                    },
                    xr::Event::InstanceLossPending(_) => {
                        report_pose(&reports, SleepingPose::Unknown);
                        break 'main_loop;
                    }
                    xr::Event::EventsLost(event) => {
                        eprintln!("OpenXR reports {} lost events", event.lost_event_count());
                    }
                    _ => {}
                }
            }

            if !session_running {
                thread::sleep(Duration::from_millis(50));
                continue;
            }

            let frame_state = frame_waiter.wait().context("xrWaitFrame")?;
            frame_stream.begin().context("xrBeginFrame")?;

            if !frame_state.should_render {
                frame_stream
                    .end(frame_state.predicted_display_time, blend_mode, &[])
                    .context("xrEndFrame without layers")?;
                continue;
            }

            let mut latest_brightness = None;
            let mut latest_surface = None;
            let mut latest_panel = None;
            while let Ok(update) = updates.try_recv() {
                match update {
                    RuntimeUpdate::Brightness(update) => latest_brightness = Some(update),
                    RuntimeUpdate::Surface(update) => latest_surface = Some(update),
                    RuntimeUpdate::Panel(update) => latest_panel = Some(update),
                }
            }
            if let Some(update) = latest_surface {
                surface_target = update;
            }
            if let Some(update) = latest_panel {
                panel_content = update;
            }
            if let Some(update) = latest_brightness {
                if let Some(active_fade) = fade {
                    let (sampled, _, _) = active_fade.sample();
                    current_brightness = sampled;
                }

                let target = sanitize_brightness(update.brightness);
                let transition_seconds = sanitize_transition_seconds(update.transition_seconds);
                if transition_seconds <= f32::EPSILON {
                    current_brightness = target;
                    fade = None;
                } else {
                    fade = Some(Fade::new(
                        current_brightness,
                        target,
                        transition_seconds,
                    ));
                }
            }

            let mut progress = 1.0_f32;
            if let Some(active_fade) = fade {
                let (sampled, sampled_progress, finished) = active_fade.sample();
                current_brightness = sampled;
                progress = sampled_progress;
                if finished {
                    current_brightness = active_fade.target;
                    fade = None;
                    progress = 1.0;
                }
            }

            if hydra_mode {
                hydra_frame += 1;
                println!(
                    "[Hydra/OpenXR] frame={hydra_frame:06} progress={:6.2}% | [Hydra/Vulkan] brightness={:6.2}% alpha={:6.2}% | [Hydra/Rustc] clipboards=0",
                    progress * 100.0,
                    current_brightness * 100.0,
                    (1.0 - current_brightness) * 100.0,
                );
            }

            let (_, views) = session
                .locate_views(VIEW_TYPE, frame_state.predicted_display_time, &local_space)
                .context("locating stereo views")?;
            if views.len() != VIEW_COUNT as usize {
                bail!("expected {VIEW_COUNT} views, runtime returned {}", views.len());
            }

            let needs_spawn_tracking = surface_target.visible && surface_controller.active_pose().is_none();
            let (head_position, head_orientation) = if needs_spawn_tracking {
                let location = view_space
                    .locate(&local_space, frame_state.predicted_display_time)
                    .context("locating HMD VIEW space for panel spawn")?;
                let tracking_valid = location
                    .location_flags
                    .contains(xr::SpaceLocationFlags::POSITION_VALID)
                    && location
                        .location_flags
                        .contains(xr::SpaceLocationFlags::ORIENTATION_VALID);
                if tracking_valid {
                    (
                        [location.pose.position.x, location.pose.position.y, location.pose.position.z],
                        [
                            location.pose.orientation.x,
                            location.pose.orientation.y,
                            location.pose.orientation.z,
                            location.pose.orientation.w,
                        ],
                    )
                } else {
                    ([f32::NAN; 3], [f32::NAN; 4])
                }
            } else {
                ([0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0])
            };
            let panel_pose = surface_controller.sync(
                surface_target,
                head_position,
                head_orientation,
            );
            if let Some(report) = overlay_pose_report_if_changed(
                &mut reported_saved_pose,
                surface_controller.saved_pose(),
            ) {
                let _ = reports.send(report);
            }

            let now_ms = pose_clock.elapsed().as_millis() as u64;
            if let Some(stable_pose) = pose_tracker.advance(now_ms) {
                let pose = protocol_pose(stable_pose);
                if hydra_mode {
                    println!("[Hydra/Pose] stable={pose:?}");
                }
                report_pose(&reports, pose);
            }
            if pose_tracker.sample_due(now_ms) {
                let orientation = views[0].pose.orientation;
                pose_tracker.sample(
                    now_ms,
                    [
                        orientation.x as f64,
                        orientation.y as f64,
                        orientation.z as f64,
                        orientation.w as f64,
                    ],
                );
            }

            let image_index = swapchain.acquire_image().context("acquiring dimmer image")?;
            swapchain
                .wait_image(xr::Duration::INFINITE)
                .context("waiting for dimmer image")?;

            let image = *images
                .get(image_index as usize)
                .ok_or_else(|| anyhow!("runtime returned invalid swapchain image index {image_index}"))?;

            vk_device
                .wait_for_fences(&[fence], true, u64::MAX)
                .context("waiting for previous dimmer submission")?;
            vk_device
                .reset_fences(&[fence])
                .context("resetting dimmer fence")?;
            vk_device
                .reset_command_buffer(command_buffer, vk::CommandBufferResetFlags::empty())
                .context("resetting command buffer")?;
            vk_device
                .begin_command_buffer(
                    command_buffer,
                    &vk::CommandBufferBeginInfo::default()
                        .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
                )
                .context("beginning command buffer")?;

            let to_transfer = [vk::ImageMemoryBarrier::default()
                .src_access_mask(vk::AccessFlags::empty())
                .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .old_layout(vk::ImageLayout::UNDEFINED)
                .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(image)
                .subresource_range(subresource)];
            vk_device.cmd_pipeline_barrier(
                command_buffer,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &to_transfer,
            );

            let clear_color = vk::ClearColorValue {
                float32: [0.0, 0.0, 0.0, 1.0 - current_brightness],
            };
            vk_device.cmd_clear_color_image(
                command_buffer,
                image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &clear_color,
                &[subresource],
            );

            let to_compositor = [vk::ImageMemoryBarrier::default()
                .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .dst_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_READ)
                .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .new_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(image)
                .subresource_range(subresource)];
            vk_device.cmd_pipeline_barrier(
                command_buffer,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &to_compositor,
            );

            vk_device
                .end_command_buffer(command_buffer)
                .context("ending command buffer")?;
            let command_buffers = [command_buffer];
            let submits = [vk::SubmitInfo::default().command_buffers(&command_buffers)];
            vk_device
                .queue_submit(queue, &submits, fence)
                .context("submitting dimmer clear")?;

            vk_device
                .wait_for_fences(&[fence], true, u64::MAX)
                .context("waiting for dimmer clear to finish")?;
            swapchain.release_image().context("releasing dimmer image")?;

            let projection_views = [
                xr::CompositionLayerProjectionView::new()
                    .pose(views[0].pose)
                    .fov(views[0].fov)
                    .sub_image(
                        xr::SwapchainSubImage::new()
                            .swapchain(&swapchain)
                            .image_array_index(0)
                            .image_rect(rect),
                    ),
                xr::CompositionLayerProjectionView::new()
                    .pose(views[1].pose)
                    .fov(views[1].fov)
                    .sub_image(
                        xr::SwapchainSubImage::new()
                            .swapchain(&swapchain)
                            .image_array_index(1)
                            .image_rect(rect),
                    ),
            ];
            let layer = xr::CompositionLayerProjection::new()
                .layer_flags(xr::CompositionLayerFlags::BLEND_TEXTURE_SOURCE_ALPHA)
                .space(&local_space)
                .views(&projection_views);

            if let Some(panel_pose) = panel_pose {
                if let Some(bitmap) = panel_cache.render_if_changed(&panel_content) {
                    panel_renderer.upload_rgba(
                        &vk_instance,
                        vk_physical_device,
                        &vk_device,
                        queue,
                        command_buffer,
                        fence,
                        format,
                        &bitmap.rgba,
                    )?;
                    if hydra_mode {
                        println!("[Hydra/Panel] status parchment refreshed; rectangle knows too much.");
                    }
                } else {
                    panel_renderer.ensure_initialized(&vk_device, queue, command_buffer, fence)?;
                }
                let panel_layer = panel_renderer.layer(&local_space, panel_pose);
                frame_stream
                    .end(
                        frame_state.predicted_display_time,
                        blend_mode,
                        &[&layer, &panel_layer],
                    )
                    .context("submitting dimmer and positioned panel layers")?;
            } else {
                frame_stream
                    .end(
                        frame_state.predicted_display_time,
                        blend_mode,
                        &[&layer],
                    )
                    .context("submitting dimmer composition layer")?;
            }
        }

        report_pose(&reports, SleepingPose::Unknown);

        if session_running {
            let _ = session.request_exit();
        }

        vk_device.device_wait_idle().ok();

        drop(panel_renderer);
        drop(swapchain);
        drop(view_space);
        drop(local_space);
        drop(frame_stream);
        drop(frame_waiter);
        drop(session);

        vk_device.destroy_fence(fence, None);
        vk_device.destroy_command_pool(command_pool, None);
        vk_device.destroy_device(None);
        vk_instance.destroy_instance(None);
    }

    println!("Nightshift dimmer stopped cleanly.");
    Ok(())
}

fn choose_color_format(formats: &[u32]) -> Result<vk::Format> {
    let preferred = [
        vk::Format::R8G8B8A8_SRGB,
        vk::Format::B8G8R8A8_SRGB,
        vk::Format::R8G8B8A8_UNORM,
        vk::Format::B8G8R8A8_UNORM,
    ];

    preferred
        .into_iter()
        .find(|candidate| formats.contains(&(candidate.as_raw() as u32)))
        .ok_or_else(|| anyhow!("runtime exposes no simple RGBA8 swapchain format: {formats:?}"))
}

unsafe fn create_overlay_session(
    instance: &xr::Instance,
    system: xr::SystemId,
    info: &xr::vulkan::SessionCreateInfo,
    placement: u32,
) -> Result<xr::sys::Session> {
    let overlay = xr::sys::SessionCreateInfoOverlayEXTX {
        ty: xr::sys::SessionCreateInfoOverlayEXTX::TYPE,
        next: std::ptr::null(),
        create_flags: OverlaySessionCreateFlagsEXTX::EMPTY,
        session_layers_placement: placement,
    };
    let binding = xr::sys::GraphicsBindingVulkanKHR {
        ty: xr::sys::GraphicsBindingVulkanKHR::TYPE,
        next: (&raw const overlay).cast(),
        instance: info.instance,
        physical_device: info.physical_device,
        device: info.device,
        queue_family_index: info.queue_family_index,
        queue_index: info.queue_index,
    };
    let session_info = xr::sys::SessionCreateInfo {
        ty: xr::sys::SessionCreateInfo::TYPE,
        next: (&raw const binding).cast(),
        create_flags: xr::SessionCreateFlags::default(),
        system_id: system,
    };

    let mut out = xr::sys::Session::NULL;
    let result = unsafe {
        (instance.fp().create_session)(instance.as_raw(), &raw const session_info, &raw mut out)
    };

    if result.into_raw() >= 0 {
        Ok(out)
    } else {
        Err(anyhow!("xrCreateSession failed: {result:?}"))
    }
}

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
        assert_eq!(
            overlay_pose_report_if_changed(&mut reported, Some(first)),
            None
        );
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
