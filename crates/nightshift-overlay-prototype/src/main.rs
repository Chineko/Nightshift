use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use ash::{
    vk::{self, Handle},
};
use openxr as xr;
use xr::OverlaySessionCreateFlagsEXTX;
use xr::sys::Handle as XrHandle;

const VIEW_TYPE: xr::ViewConfigurationType = xr::ViewConfigurationType::PRIMARY_STEREO;
const VIEW_COUNT: u32 = 2;
const TEST_TEXTURE_SIZE: u32 = 4;
const DEFAULT_PLACEMENT: u32 = 4;
const DEFAULT_BRIGHTNESS: f32 = 0.20;

fn main() -> Result<()> {
    let brightness = parse_flag_f32("--brightness")?.unwrap_or(DEFAULT_BRIGHTNESS).clamp(0.0, 1.0);
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

fn parse_flag<T>(name: &str, parse: impl FnOnce(&str) -> Result<T> + Copy) -> Result<Option<T>> {
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

        let vk_instance = {
            let vk_instance_info = vk::InstanceCreateInfo::default().application_info(&vk_app_info);
            let raw = xr_instance
                .create_vulkan_instance(
                    system,
                    std::mem::transmute(vk_entry.static_fn().get_instance_proc_addr),
                    (&raw const vk_instance_info).cast(),
                )
                .context("OpenXR creating Vulkan instance")?
                .map_err(vk::Result::from_raw)
                .context("Vulkan creating instance")?;

            ash::Instance::load(
                vk_entry.static_fn(),
                vk::Instance::from_raw(raw as _),
            )
        };

        let vk_physical_device = vk::PhysicalDevice::from_raw(
            xr_instance
                .vulkan_graphics_device(system, vk_instance.handle().as_raw() as _)
                .context("getting OpenXR Vulkan physical device")? as _,
        );

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
        let queue_info = [vk::DeviceQueueCreateInfo::default()
            .queue_family_index(queue_family_index)
            .queue_priorities(&priorities)];
        let mut multiview = vk::PhysicalDeviceMultiviewFeatures {
            multiview: vk::TRUE,
            ..Default::default()
        };
        let device_info = vk::DeviceCreateInfo::default()
            .queue_create_infos(&queue_info)
            .push_next(&mut multiview);

        let vk_device = {
            let raw = xr_instance
                .create_vulkan_device(
                    system,
                    std::mem::transmute(vk_entry.static_fn().get_instance_proc_addr),
                    vk_physical_device.as_raw() as _,
                    (&raw const device_info).cast(),
                )
                .context("OpenXR creating Vulkan device")?
                .map_err(vk::Result::from_raw)
                .context("Vulkan creating device")?;

            ash::Device::load(vk_instance.fp_v1_0(), vk::Device::from_raw(raw as _))
        };

        let queue = vk_device.get_device_queue(queue_family_index, 0);

        let (session, mut frame_waiter, mut frame_stream) = {
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
            xr::Session::from_raw(xr_instance.clone(), raw_session, Box::new(()))
        };

        let local_space = session
            .create_reference_space(xr::ReferenceSpaceType::LOCAL, xr::Posef::IDENTITY)
            .context("creating LOCAL reference space")?;

        let formats = session
            .enumerate_swapchain_formats()
            .context("enumerating swapchain formats")?;
        let format = choose_color_format(&formats)?;
        println!("Vulkan format: {}", format.as_raw());

        let view_masks = [0b11];
        let correlation_masks = [0b11];
        let mut multiview_info = vk::RenderPassMultiviewCreateInfo::default()
            .view_masks(&view_masks)
            .correlation_masks(&correlation_masks);
        let color_attachment = [vk::AttachmentDescription {
            format,
            samples: vk::SampleCountFlags::TYPE_1,
            load_op: vk::AttachmentLoadOp::CLEAR,
            store_op: vk::AttachmentStoreOp::STORE,
            stencil_load_op: vk::AttachmentLoadOp::DONT_CARE,
            stencil_store_op: vk::AttachmentStoreOp::DONT_CARE,
            initial_layout: vk::ImageLayout::UNDEFINED,
            final_layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
            ..Default::default()
        }];
        let color_ref = [vk::AttachmentReference {
            attachment: 0,
            layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
        }];
        let subpass = [vk::SubpassDescription::default()
            .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
            .color_attachments(&color_ref)];

        let render_pass = vk_device
            .create_render_pass(
                &vk::RenderPassCreateInfo::default()
                    .attachments(&color_attachment)
                    .subpasses(&subpass)
                    .push_next(&mut multiview_info),
                None,
            )
            .context("creating Vulkan render pass")?;

        let mut swapchain = session
            .create_swapchain(&xr::SwapchainCreateInfo {
                create_flags: xr::SwapchainCreateFlags::EMPTY,
                usage_flags: xr::SwapchainUsageFlags::COLOR_ATTACHMENT,
                format: format.as_raw() as _,
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
            .context("enumerating swapchain images")?;
        let mut image_views = Vec::with_capacity(images.len());
        let mut framebuffers = Vec::with_capacity(images.len());

        for raw_image in images {
            let image = vk::Image::from_raw(raw_image);
            let image_view = vk_device
                .create_image_view(
                    &vk::ImageViewCreateInfo::default()
                        .image(image)
                        .view_type(vk::ImageViewType::TYPE_2D_ARRAY)
                        .format(format)
                        .subresource_range(vk::ImageSubresourceRange {
                            aspect_mask: vk::ImageAspectFlags::COLOR,
                            base_mip_level: 0,
                            level_count: 1,
                            base_array_layer: 0,
                            layer_count: VIEW_COUNT,
                        }),
                    None,
                )
                .context("creating swapchain image view")?;

            let attachments = [image_view];
            let framebuffer = vk_device
                .create_framebuffer(
                    &vk::FramebufferCreateInfo::default()
                        .render_pass(render_pass)
                        .attachments(&attachments)
                        .width(TEST_TEXTURE_SIZE)
                        .height(TEST_TEXTURE_SIZE)
                        .layers(1),
                    None,
                )
                .context("creating framebuffer")?;

            image_views.push(image_view);
            framebuffers.push(framebuffer);
        }

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

            vk_device
                .wait_for_fences(&[fence], true, u64::MAX)
                .context("waiting for render fence")?;
            vk_device.reset_fences(&[fence]).context("resetting render fence")?;
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

            let clear = [vk::ClearValue {
                color: vk::ClearColorValue {
                    float32: [0.0, 0.0, 0.0, 1.0 - brightness],
                },
            }];
            vk_device.cmd_begin_render_pass(
                command_buffer,
                &vk::RenderPassBeginInfo::default()
                    .render_pass(render_pass)
                    .framebuffer(framebuffers[image_index as usize])
                    .render_area(vk::Rect2D {
                        offset: vk::Offset2D { x: 0, y: 0 },
                        extent: vk::Extent2D {
                            width: TEST_TEXTURE_SIZE,
                            height: TEST_TEXTURE_SIZE,
                        },
                    })
                    .clear_values(&clear),
                vk::SubpassContents::INLINE,
            );
            vk_device.cmd_end_render_pass(command_buffer);
            vk_device
                .end_command_buffer(command_buffer)
                .context("ending command buffer")?;

            let command_buffers = [command_buffer];
            let submit = [vk::SubmitInfo::default().command_buffers(&command_buffers)];
            vk_device
                .queue_submit(queue, &submit, fence)
                .context("submitting dimmer clear")?;
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

        drop((swapchain, local_space, session, frame_waiter, frame_stream));

        vk_device.destroy_fence(fence, None);
        vk_device.destroy_command_pool(command_pool, None);
        for framebuffer in framebuffers {
            vk_device.destroy_framebuffer(framebuffer, None);
        }
        for image_view in image_views {
            vk_device.destroy_image_view(image_view, None);
        }
        vk_device.destroy_render_pass(render_pass, None);
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
        .find(|candidate| formats.contains(&(candidate.as_raw() as i64)))
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
