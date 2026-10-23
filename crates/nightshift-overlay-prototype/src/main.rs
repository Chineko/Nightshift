use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use ash::vk::{self, Handle};
use openxr as xr;
use xr::OverlaySessionCreateFlagsEXTX;
use xr::sys::Handle as XrHandle;

const VIEW_TYPE: xr::ViewConfigurationType = xr::ViewConfigurationType::PRIMARY_STEREO;
const VIEW_COUNT: u32 = 2;
const TEST_TEXTURE_SIZE: u32 = 4;
const DEFAULT_PLACEMENT: u32 = 4;
const DEFAULT_BRIGHTNESS: f32 = 0.20;

fn main() -> Result<()> {
    let brightness = parse_flag_f32("--brightness")?
        .unwrap_or(DEFAULT_BRIGHTNESS)
        .clamp(0.0, 1.0);
    let placement = parse_flag_u32("--placement")?.unwrap_or(DEFAULT_PLACEMENT);
    let alpha = 1.0 - brightness;

    println!("Nightshift OpenXR dimmer prototype");
    println!("  placement  : {placement}");
    println!("  brightness : {:.0}%", brightness * 100.0);
    println!("  dim alpha  : {:.0}%", alpha * 100.0);
    println!("  Ctrl+C to stop");
    println!();
    println!("If VRChat dims but WayVR stays bright, the tiny evil square has succeeded.");

    let running = Arc::new(AtomicBool::new(true));
    let signal = running.clone();
    ctrlc::set_handler(move || signal.store(false, Ordering::Relaxed))
        .context("installing Ctrl+C handler")?;

    run_overlay(running, brightness, placement)
}

fn parse_flag_f32(name: &str) -> Result<Option<f32>> {
    parse_flag(name, |value| {
        value
            .parse::<f32>()
            .map_err(|e| anyhow!("invalid {name} value {value:?}: {e}"))
    })
}

fn parse_flag_u32(name: &str) -> Result<Option<u32>> {
    parse_flag(name, |value| {
        value
            .parse::<u32>()
            .map_err(|e| anyhow!("invalid {name} value {value:?}: {e}"))
    })
}

fn parse_flag<T>(name: &str, parse: impl Fn(&str) -> Result<T>) -> Result<Option<T>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let mut index = 0;

    while index < args.len() {
        let arg = &args[index];
        if arg == name {
            let value = args
                .get(index + 1)
                .ok_or_else(|| anyhow!("{name} requires a value"))?;
            return parse(value).map(Some);
        }

        if let Some(value) = arg.strip_prefix(&format!("{name}=")) {
            return parse(value).map(Some);
        }

        index += 1;
    }

    Ok(None)
}

#[allow(clippy::too_many_lines)]
fn run_overlay(running: Arc<AtomicBool>, brightness: f32, placement: u32) -> Result<()> {
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
                application_name: "nightshift-overlay-prototype",
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

        let (session, mut frame_waiter, mut frame_stream) =
            xr::Session::<xr::Vulkan>::from_raw(
                xr_instance.clone(),
                raw_session,
                Box::new(()),
            );

        let local_space = session
            .create_reference_space(xr::ReferenceSpaceType::LOCAL, xr::Posef::IDENTITY)
            .context("creating LOCAL reference space")?;

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
                format: i64::from(format.as_raw()),
                sample_count: 1,
                width: TEST_TEXTURE_SIZE,
                height: TEST_TEXTURE_SIZE,
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
        let rect = xr::Rect2Di {
            offset: xr::Offset2Di { x: 0, y: 0 },
            extent: xr::Extent2Di {
                width: TEST_TEXTURE_SIZE as i32,
                height: TEST_TEXTURE_SIZE as i32,
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
                            println!("Overlay session running. Tiny evil square deployed.");
                        }
                        xr::SessionState::STOPPING => {
                            session.end().context("ending overlay session")?;
                            session_running = false;
                        }
                        xr::SessionState::EXITING | xr::SessionState::LOSS_PENDING => {
                            break 'main_loop;
                        }
                        _ => {}
                    },
                    xr::Event::InstanceLossPending(_) => break 'main_loop,
                    xr::Event::EventsLost(event) => {
                        eprintln!("OpenXR reports {} lost events", event.lost_event_count());
                    }
                    _ => {}
                }
            }

            if !session_running {
                std::thread::sleep(Duration::from_millis(50));
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

            let (_, views) = session
                .locate_views(VIEW_TYPE, frame_state.predicted_display_time, &local_space)
                .context("locating stereo views")?;
            if views.len() != VIEW_COUNT as usize {
                bail!("expected {VIEW_COUNT} views, runtime returned {}", views.len());
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
                float32: [0.0, 0.0, 0.0, 1.0 - brightness],
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

            // OpenXR owns the image again after release_image(), so the GPU must be done first.
            // Otherwise the compositor and our command buffer can race over the same four pixels,
            // which is an impressively efficient way to invent modern art.
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

            frame_stream
                .end(
                    frame_state.predicted_display_time,
                    blend_mode,
                    &[&layer],
                )
                .context("submitting dimmer composition layer")?;
        }

        if session_running {
            let _ = session.request_exit();
        }

        vk_device.device_wait_idle().ok();

        // Destroy OpenXR objects before destroying the Vulkan device they refer to.
        drop(swapchain);
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

fn choose_color_format(formats: &[i64]) -> Result<vk::Format> {
    let preferred = [
        vk::Format::R8G8B8A8_SRGB,
        vk::Format::B8G8R8A8_SRGB,
        vk::Format::R8G8B8A8_UNORM,
        vk::Format::B8G8R8A8_UNORM,
    ];

    preferred
        .into_iter()
        .find(|candidate| formats.contains(&i64::from(candidate.as_raw())))
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
