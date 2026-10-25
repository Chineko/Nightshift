use std::sync::{
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

fn next_invite_profile(
    profile: nightshift_protocol::InviteProfile,
) -> nightshift_protocol::InviteProfile {
    use nightshift_protocol::InviteProfile;
    match profile {
        InviteProfile::Disabled => InviteProfile::Friends,
        InviteProfile::Friends => InviteProfile::Favorites,
        InviteProfile::Favorites => InviteProfile::Whitelist,
        InviteProfile::Whitelist => InviteProfile::Custom,
        InviteProfile::Custom => InviteProfile::Disabled,
    }
}

fn next_vrchat_status(
    status: nightshift_protocol::VrchatStatus,
) -> nightshift_protocol::VrchatStatus {
    use nightshift_protocol::VrchatStatus;
    match status {
        VrchatStatus::Active => VrchatStatus::AskMe,
        VrchatStatus::AskMe => VrchatStatus::Busy,
        VrchatStatus::Busy => VrchatStatus::Offline,
        VrchatStatus::Offline => VrchatStatus::JoinMe,
        VrchatStatus::JoinMe => VrchatStatus::Active,
    }
}

fn panel_command(
    action: PanelAction,
    content: &PanelContent,
    current_brightness: f32,
) -> Option<UiCommand> {
    match action {
        PanelAction::EnterSleep => Some(UiCommand::EnterSleep),
        PanelAction::Wake => Some(UiCommand::Wake),
        PanelAction::BrightnessDown => Some(UiCommand::SetBrightness(sanitize_brightness(
            current_brightness - 0.1,
        ))),
        PanelAction::BrightnessUp => Some(UiCommand::SetBrightness(sanitize_brightness(
            current_brightness + 0.1,
        ))),
        PanelAction::ToggleHeartRate => {
            Some(UiCommand::SetHrMonitoring(!content.heart_rate_enabled))
        }
        PanelAction::ToggleOsc => Some(UiCommand::SetOscAutomation(!content.osc_enabled)),
        PanelAction::CycleInviteProfile => Some(UiCommand::SetInviteProfile(next_invite_profile(
            content.invite_profile,
        ))),
        PanelAction::CycleVrchatStatus => Some(UiCommand::SetVrchatSleepStatus(
            next_vrchat_status(content.sleep_vrchat_status),
        )),
        PanelAction::RemoveSocial(index) => {
            let mut rules = content.invite_rules.clone();
            let list = match content.social_tab {
                SocialTab::Whitelist => &mut rules.whitelist,
                SocialTab::Blacklist => &mut rules.blacklist,
            };
            let index = index as usize;
            if index < list.len() {
                list.remove(index);
                Some(UiCommand::SetInviteRules(rules))
            } else {
                None
            }
        }
        PanelAction::NavigateHome
        | PanelAction::NavigateSettings
        | PanelAction::SelectWhitelist
        | PanelAction::SelectBlacklist => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ActiveGrab {
    hand: ControllerHand,
    grab: PanelGrab,
}

#[allow(clippy::too_many_lines)]
pub(crate) fn run_overlay(
    running: Arc<AtomicBool>,
    updates: Receiver<RuntimeUpdate>,
    reports: Sender<WorkerReport>,
    commands: Sender<UiCommand>,
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

    let props = xr_instance
        .properties()
        .context("querying OpenXR runtime")?;
    println!(
        "OpenXR runtime: {} {}",
        props.runtime_name, props.runtime_version
    );

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

        let (session, mut frame_waiter, mut frame_stream) =
            xr::Session::<xr::Vulkan>::from_raw(xr_instance.clone(), raw_session, Box::new(()));

        let local_space = session
            .create_reference_space(xr::ReferenceSpaceType::LOCAL, xr::Posef::IDENTITY)
            .context("creating LOCAL reference space")?;
        let view_space = session
            .create_reference_space(xr::ReferenceSpaceType::VIEW, xr::Posef::IDENTITY)
            .context("creating VIEW reference space")?;
        let controller_input = match ControllerInput::new(&xr_instance, &session) {
            Ok(input) => Some(input),
            Err(error) => {
                eprintln!("Nightshift panel controller input disabled: {error:#}");
                None
            }
        };

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
        let mut laser_renderer = LaserRenderer::new(&session, format)?;

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
        let mut panel_page = PanelPage::Home;
        let mut social_tab = SocialTab::Whitelist;
        let mut panel_content =
            PanelContent::from_state(&AppState::default()).with_view(panel_page, social_tab);
        let mut panel_cache = PanelBitmapCache::default();
        let mut hydra_frame = 0_u64;
        let pose_clock = Instant::now();
        let mut pose_tracker = PoseTracker::new(0);
        let mut input_error_reported = false;
        let mut active_grab: Option<ActiveGrab> = None;
        let mut active_pointer_hand: Option<ControllerHand> = None;
        let mut pointer_smoother = PointerSmoother::default();
        let mut pointer_visual_clock = Instant::now();
        let mut laser_easter_egg_fired = false;
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
                            session
                                .begin(VIEW_TYPE)
                                .context("beginning overlay session")?;
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
                panel_content = update.with_view(panel_page, social_tab);
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
                    fade = Some(Fade::new(current_brightness, target, transition_seconds));
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
                bail!(
                    "expected {VIEW_COUNT} views, runtime returned {}",
                    views.len()
                );
            }

            let hmd_position = [
                (views[0].pose.position.x + views[1].pose.position.x) * 0.5,
                (views[0].pose.position.y + views[1].pose.position.y) * 0.5,
                (views[0].pose.position.z + views[1].pose.position.z) * 0.5,
            ];

            let needs_spawn_tracking =
                surface_target.visible && surface_controller.active_pose().is_none();
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
                        [
                            location.pose.position.x,
                            location.pose.position.y,
                            location.pose.position.z,
                        ],
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
            if !surface_target.visible {
                active_grab = None;
            }
            let mut panel_pose =
                surface_controller.sync(surface_target, head_position, head_orientation);
            if let Some(report) = overlay_pose_report_if_changed(
                &mut reported_saved_pose,
                surface_controller.saved_pose(),
            ) {
                let _ = reports.send(report);
            }

            let mut hovered_action = None;
            let mut raw_pointer_segment = None;
            if let Some(controller_input) = controller_input.as_ref() {
                match controller_input.samples(
                    &session,
                    frame_state.predicted_display_time,
                    &local_space,
                ) {
                    Ok(samples) => {
                        input_error_reported = false;
                        if surface_target.visible {
                            if let Some(current_panel_pose) = panel_pose {
                                if let Some(active) = active_grab {
                                    active_pointer_hand = Some(active.hand);
                                    if let Some(sample) = samples
                                        .iter()
                                        .copied()
                                        .find(|sample| sample.hand == active.hand)
                                    {
                                        if sample.grip_just_released || !sample.grip_down {
                                            if let Some(committed_pose) =
                                                surface_controller.active_pose()
                                            {
                                                let _ = surface_controller
                                                    .remember_pose(committed_pose);
                                                panel_pose = Some(committed_pose);
                                                if hydra_mode {
                                                    println!(
                                                        "[Hydra/Input] hand={:?} grab=released position=({:.3},{:.3},{:.3})",
                                                        sample.hand,
                                                        committed_pose.position[0],
                                                        committed_pose.position[1],
                                                        committed_pose.position[2]
                                                    );
                                                }
                                            }
                                            active_grab = None;
                                        } else if let Some(controller_pose) = sample.controller_pose
                                            && let Some(moved_pose) =
                                                update_panel_grab(active.grab, controller_pose)
                                            && surface_controller.move_active_pose(moved_pose)
                                        {
                                            panel_pose = Some(moved_pose);
                                        }
                                    }
                                } else {
                                    let hits = [
                                        samples[0].ray.and_then(|ray| {
                                            hit_test_panel_with_distance(
                                                ray,
                                                current_panel_pose,
                                                PANEL_WIDTH_M,
                                                PANEL_HEIGHT_M,
                                            )
                                        }),
                                        samples[1].ray.and_then(|ray| {
                                            hit_test_panel_with_distance(
                                                ray,
                                                current_panel_pose,
                                                PANEL_WIDTH_M,
                                                PANEL_HEIGHT_M,
                                            )
                                        }),
                                    ];
                                    active_pointer_hand = select_active_pointer_hand(
                                        &samples,
                                        [hits[0].is_some(), hits[1].is_some()],
                                        active_pointer_hand,
                                    );

                                    if let Some(hand) = active_pointer_hand {
                                        let index = match hand {
                                            ControllerHand::Left => 0,
                                            ControllerHand::Right => 1,
                                        };
                                        if let (Some(ray), Some(hit)) =
                                            (samples[index].ray, hits[index])
                                        {
                                            let sample = samples[index];
                                            raw_pointer_segment = Some(LaserSegment {
                                                ray,
                                                distance_m: hit.distance_m,
                                            });
                                            hovered_action = action_at_uv_for_page(
                                                hit.uv,
                                                panel_page,
                                                panel_content.sleep_action_is_wake(),
                                            );

                                            if sample.trigger_just_pressed {
                                                if let Some(action) = hovered_action {
                                                    if hydra_mode && !laser_easter_egg_fired {
                                                        println!(
                                                            "[Hydra/Input] I'M FIRIN' MAH LAZER"
                                                        );
                                                        laser_easter_egg_fired = true;
                                                    }
                                                    if hydra_mode {
                                                        println!(
                                                            "[Hydra/Input] hand={:?} action={action:?} uv=({:.3},{:.3})",
                                                            sample.hand, hit.uv.u, hit.uv.v
                                                        );
                                                    }
                                                    match action {
                                                        PanelAction::NavigateHome => {
                                                            panel_page = PanelPage::Home;
                                                            panel_content
                                                                .set_view(panel_page, social_tab);
                                                            panel_cache.invalidate();
                                                        }
                                                        PanelAction::NavigateSettings => {
                                                            panel_page = PanelPage::Settings;
                                                            panel_content
                                                                .set_view(panel_page, social_tab);
                                                            panel_cache.invalidate();
                                                        }
                                                        PanelAction::SelectWhitelist => {
                                                            social_tab = SocialTab::Whitelist;
                                                            panel_content
                                                                .set_view(panel_page, social_tab);
                                                            panel_cache.invalidate();
                                                        }
                                                        PanelAction::SelectBlacklist => {
                                                            social_tab = SocialTab::Blacklist;
                                                            panel_content
                                                                .set_view(panel_page, social_tab);
                                                            panel_cache.invalidate();
                                                        }
                                                        _ => {
                                                            if let Some(command) = panel_command(
                                                                action,
                                                                &panel_content,
                                                                current_brightness,
                                                            ) {
                                                                let _ = commands.send(command);
                                                            }
                                                        }
                                                    }
                                                }
                                            }

                                            if sample.grip_just_pressed {
                                                if let Some(controller_pose) =
                                                    sample.controller_pose
                                                    && let Some(grab) = begin_panel_grab(
                                                        controller_pose,
                                                        current_panel_pose,
                                                    )
                                                {
                                                    if hydra_mode && !laser_easter_egg_fired {
                                                        println!(
                                                            "[Hydra/Input] I'M FIRIN' MAH LAZER"
                                                        );
                                                        laser_easter_egg_fired = true;
                                                    }
                                                    active_grab = Some(ActiveGrab {
                                                        hand: sample.hand,
                                                        grab,
                                                    });
                                                    if hydra_mode {
                                                        println!(
                                                            "[Hydra/Input] hand={:?} grab=started uv=({:.3},{:.3})",
                                                            sample.hand, hit.uv.u, hit.uv.v
                                                        );
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        } else {
                            active_pointer_hand = None;
                        }
                    }
                    Err(error) if !input_error_reported => {
                        eprintln!("Nightshift panel input sample failed: {error:#}");
                        input_error_reported = true;
                    }
                    Err(_) => {}
                }
            }

            let pointer_now = Instant::now();
            let pointer_delta = pointer_now
                .duration_since(pointer_visual_clock)
                .as_secs_f32();
            pointer_visual_clock = pointer_now;
            let smoothed_beam_segment = match raw_pointer_segment {
                Some(segment) => {
                    Some(pointer_smoother.update(segment, smoothing_alpha(pointer_delta)))
                }
                None => {
                    pointer_smoother.reset();
                    None
                }
            };

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

            let image_index = swapchain
                .acquire_image()
                .context("acquiring dimmer image")?;
            swapchain
                .wait_image(xr::Duration::INFINITE)
                .context("waiting for dimmer image")?;

            let image = *images.get(image_index as usize).ok_or_else(|| {
                anyhow!("runtime returned invalid swapchain image index {image_index}")
            })?;

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
            swapchain
                .release_image()
                .context("releasing dimmer image")?;

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
                if let Some(bitmap) = panel_cache.render_if_changed(&panel_content, hovered_action)
                {
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
                        println!(
                            "[Hydra/Panel] status parchment refreshed; rectangle knows too much."
                        );
                    }
                } else {
                    panel_renderer.ensure_initialized(&vk_device, queue, command_buffer, fence)?;
                }
                let panel_layer = panel_renderer.layer(&local_space, panel_pose);
                if let Some(cursor_segment) = raw_pointer_segment {
                    laser_renderer.ensure_initialized(&vk_device, queue, command_buffer, fence)?;
                    if let Some(pointer_layers) = laser_renderer.layers(
                        &local_space,
                        cursor_segment,
                        smoothed_beam_segment,
                        hmd_position,
                        surface_target.laser_enabled,
                    ) {
                        if let Some(beam_layer) = pointer_layers.beam {
                            frame_stream
                                .end(
                                    frame_state.predicted_display_time,
                                    blend_mode,
                                    &[&layer, &panel_layer, &beam_layer, &pointer_layers.cursor],
                                )
                                .context("submitting dimmer, panel, laser, and cursor layers")?;
                        } else {
                            frame_stream
                                .end(
                                    frame_state.predicted_display_time,
                                    blend_mode,
                                    &[&layer, &panel_layer, &pointer_layers.cursor],
                                )
                                .context("submitting dimmer, panel, and cursor layers")?;
                        }
                    } else {
                        frame_stream
                            .end(
                                frame_state.predicted_display_time,
                                blend_mode,
                                &[&layer, &panel_layer],
                            )
                            .context("submitting dimmer and positioned panel layers")?;
                    }
                } else {
                    frame_stream
                        .end(
                            frame_state.predicted_display_time,
                            blend_mode,
                            &[&layer, &panel_layer],
                        )
                        .context("submitting dimmer and positioned panel layers")?;
                }
            } else {
                frame_stream
                    .end(frame_state.predicted_display_time, blend_mode, &[&layer])
                    .context("submitting dimmer composition layer")?;
            }
        }

        report_pose(&reports, SleepingPose::Unknown);

        if session_running {
            let _ = session.request_exit();
        }

        vk_device.device_wait_idle().ok();

        drop(laser_renderer);
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
