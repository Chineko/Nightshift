use anyhow::{Context, Result, anyhow};
use ash::vk::{self, Handle};
use openxr as xr;

use crate::placement::PanelPose;

pub const PANEL_TEXTURE_WIDTH: u32 = 8;
pub const PANEL_TEXTURE_HEIGHT: u32 = 8;
pub const PANEL_WIDTH_M: f32 = 0.72;
pub const PANEL_HEIGHT_M: f32 = 0.42;

pub struct PanelRenderer {
    swapchain: xr::Swapchain<xr::Vulkan>,
    images: Vec<vk::Image>,
    rect: xr::Rect2Di,
    subresource: vk::ImageSubresourceRange,
    initialized: bool,
}

impl PanelRenderer {
    pub fn new(session: &xr::Session<xr::Vulkan>, format: vk::Format) -> Result<Self> {
        let swapchain = session
            .create_swapchain(&xr::SwapchainCreateInfo {
                create_flags: xr::SwapchainCreateFlags::EMPTY,
                usage_flags: xr::SwapchainUsageFlags::COLOR_ATTACHMENT
                    | xr::SwapchainUsageFlags::TRANSFER_DST,
                format: format.as_raw() as u32,
                sample_count: 1,
                width: PANEL_TEXTURE_WIDTH,
                height: PANEL_TEXTURE_HEIGHT,
                face_count: 1,
                array_size: 1,
                mip_count: 1,
            })
            .context("creating positioned panel swapchain")?;

        let images = swapchain
            .enumerate_images()
            .context("enumerating positioned panel swapchain images")?
            .into_iter()
            .map(vk::Image::from_raw)
            .collect::<Vec<_>>();

        Ok(Self {
            swapchain,
            images,
            rect: xr::Rect2Di {
                offset: xr::Offset2Di { x: 0, y: 0 },
                extent: xr::Extent2Di {
                    width: PANEL_TEXTURE_WIDTH as i32,
                    height: PANEL_TEXTURE_HEIGHT as i32,
                },
            },
            subresource: vk::ImageSubresourceRange {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                base_mip_level: 0,
                level_count: 1,
                base_array_layer: 0,
                layer_count: 1,
            },
            initialized: false,
        })
    }

    pub unsafe fn ensure_initialized(
        &mut self,
        device: &ash::Device,
        queue: vk::Queue,
        command_buffer: vk::CommandBuffer,
        fence: vk::Fence,
    ) -> Result<()> {
        if self.initialized {
            return Ok(());
        }

        let image_index = self
            .swapchain
            .acquire_image()
            .context("acquiring positioned panel image")?;
        self.swapchain
            .wait_image(xr::Duration::INFINITE)
            .context("waiting for positioned panel image")?;
        let image = *self
            .images
            .get(image_index as usize)
            .ok_or_else(|| anyhow!("runtime returned invalid panel swapchain image index {image_index}"))?;

        unsafe {
            device
                .wait_for_fences(&[fence], true, u64::MAX)
                .context("waiting for previous panel submission")?;
            device
                .reset_fences(&[fence])
                .context("resetting panel fence")?;
            device
                .reset_command_buffer(command_buffer, vk::CommandBufferResetFlags::empty())
                .context("resetting panel command buffer")?;
            device
                .begin_command_buffer(
                    command_buffer,
                    &vk::CommandBufferBeginInfo::default()
                        .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
                )
                .context("beginning panel command buffer")?;

            let to_transfer = [vk::ImageMemoryBarrier::default()
                .src_access_mask(vk::AccessFlags::empty())
                .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .old_layout(vk::ImageLayout::UNDEFINED)
                .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(image)
                .subresource_range(self.subresource)];
            device.cmd_pipeline_barrier(
                command_buffer,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &to_transfer,
            );

            // A deliberately simple opaque surface for the first positioned-overlay runtime slice.
            // Rich content belongs in a later rendering/UI checkpoint; geometry and lifecycle come first.
            device.cmd_clear_color_image(
                command_buffer,
                image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &vk::ClearColorValue {
                    float32: [0.035, 0.04, 0.055, 1.0],
                },
                &[self.subresource],
            );

            let to_compositor = [vk::ImageMemoryBarrier::default()
                .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .dst_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_READ)
                .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .new_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(image)
                .subresource_range(self.subresource)];
            device.cmd_pipeline_barrier(
                command_buffer,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &to_compositor,
            );

            device
                .end_command_buffer(command_buffer)
                .context("ending panel command buffer")?;
            let command_buffers = [command_buffer];
            let submits = [vk::SubmitInfo::default().command_buffers(&command_buffers)];
            device
                .queue_submit(queue, &submits, fence)
                .context("submitting positioned panel clear")?;
            device
                .wait_for_fences(&[fence], true, u64::MAX)
                .context("waiting for positioned panel clear")?;
        }

        self.swapchain
            .release_image()
            .context("releasing positioned panel image")?;
        self.initialized = true;
        Ok(())
    }

    pub fn layer<'a>(
        &'a self,
        space: &'a xr::Space,
        pose: PanelPose,
    ) -> xr::CompositionLayerQuad<'a, xr::Vulkan> {
        xr::CompositionLayerQuad::new()
            .pose(panel_pose_to_xr(pose))
            .sub_image(
                xr::SwapchainSubImage::new()
                    .swapchain(&self.swapchain)
                    .image_array_index(0)
                    .image_rect(self.rect),
            )
            .eye_visibility(xr::EyeVisibility::BOTH)
            .space(space)
            .size(xr::Extent2Df {
                width: PANEL_WIDTH_M,
                height: PANEL_HEIGHT_M,
            })
    }
}

pub fn panel_pose_to_xr(pose: PanelPose) -> xr::Posef {
    xr::Posef {
        orientation: xr::Quaternionf {
            x: pose.orientation_xyzw[0],
            y: pose.orientation_xyzw[1],
            z: pose.orientation_xyzw[2],
            w: pose.orientation_xyzw[3],
        },
        position: xr::Vector3f {
            x: pose.position[0],
            y: pose.position[1],
            z: pose.position[2],
        },
    }
}
