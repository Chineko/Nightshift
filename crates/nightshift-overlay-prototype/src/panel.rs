use anyhow::{Context, Result, anyhow, bail};
use ash::vk::{self, Handle};
use openxr as xr;

use crate::placement::PanelPose;

pub const PANEL_TEXTURE_WIDTH: u32 = 768;
pub const PANEL_TEXTURE_HEIGHT: u32 = 448;
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
        let image = self.image(image_index)?;

        unsafe {
            prepare_command_buffer(device, command_buffer, fence)?;

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

            device.cmd_clear_color_image(
                command_buffer,
                image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &vk::ClearColorValue {
                    float32: [0.035, 0.04, 0.055, 1.0],
                },
                &[self.subresource],
            );

            transition_to_compositor(device, command_buffer, image, self.subresource);
            submit_and_wait(device, queue, command_buffer, fence)?;
        }

        self.swapchain
            .release_image()
            .context("releasing positioned panel image")?;
        self.initialized = true;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub unsafe fn upload_rgba(
        &mut self,
        instance: &ash::Instance,
        physical_device: vk::PhysicalDevice,
        device: &ash::Device,
        queue: vk::Queue,
        command_buffer: vk::CommandBuffer,
        fence: vk::Fence,
        format: vk::Format,
        rgba: &[u8],
    ) -> Result<()> {
        let upload = prepare_upload_bytes(rgba, format)?;
        let image_index = self
            .swapchain
            .acquire_image()
            .context("acquiring positioned panel image for upload")?;
        self.swapchain
            .wait_image(xr::Duration::INFINITE)
            .context("waiting for positioned panel image for upload")?;
        let image = self.image(image_index)?;

        let buffer = unsafe {
            device.create_buffer(
                &vk::BufferCreateInfo::default()
                    .size(upload.len() as u64)
                    .usage(vk::BufferUsageFlags::TRANSFER_SRC)
                    .sharing_mode(vk::SharingMode::EXCLUSIVE),
                None,
            )
        }
        .context("creating positioned panel staging buffer")?;
        let requirements = unsafe { device.get_buffer_memory_requirements(buffer) };
        let properties = unsafe { instance.get_physical_device_memory_properties(physical_device) };
        let memory_type_index = find_memory_type(
            requirements.memory_type_bits,
            &properties,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        )
        .ok_or_else(|| anyhow!("Vulkan exposes no host-visible coherent memory for panel upload"))?;
        let memory = unsafe {
            device.allocate_memory(
                &vk::MemoryAllocateInfo::default()
                    .allocation_size(requirements.size)
                    .memory_type_index(memory_type_index),
                None,
            )
        }
        .context("allocating positioned panel staging memory")?;

        let upload_result = (|| -> Result<()> {
            unsafe {
                device
                    .bind_buffer_memory(buffer, memory, 0)
                    .context("binding positioned panel staging memory")?;
                let mapped = device
                    .map_memory(memory, 0, upload.len() as u64, vk::MemoryMapFlags::empty())
                    .context("mapping positioned panel staging memory")?;
                std::ptr::copy_nonoverlapping(upload.as_ptr(), mapped.cast::<u8>(), upload.len());
                device.unmap_memory(memory);

                prepare_command_buffer(device, command_buffer, fence)?;
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

                let copy = [vk::BufferImageCopy::default()
                    .buffer_offset(0)
                    .buffer_row_length(0)
                    .buffer_image_height(0)
                    .image_subresource(
                        vk::ImageSubresourceLayers::default()
                            .aspect_mask(vk::ImageAspectFlags::COLOR)
                            .mip_level(0)
                            .base_array_layer(0)
                            .layer_count(1),
                    )
                    .image_offset(vk::Offset3D { x: 0, y: 0, z: 0 })
                    .image_extent(vk::Extent3D {
                        width: PANEL_TEXTURE_WIDTH,
                        height: PANEL_TEXTURE_HEIGHT,
                        depth: 1,
                    })];
                device.cmd_copy_buffer_to_image(
                    command_buffer,
                    buffer,
                    image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &copy,
                );

                transition_to_compositor(device, command_buffer, image, self.subresource);
                submit_and_wait(device, queue, command_buffer, fence)?;
            }
            Ok(())
        })();

        unsafe {
            device.destroy_buffer(buffer, None);
            device.free_memory(memory, None);
        }

        if upload_result.is_ok() {
            self.swapchain
                .release_image()
                .context("releasing uploaded positioned panel image")?;
            self.initialized = true;
        }
        upload_result
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

    fn image(&self, image_index: u32) -> Result<vk::Image> {
        self.images
            .get(image_index as usize)
            .copied()
            .ok_or_else(|| anyhow!("runtime returned invalid panel swapchain image index {image_index}"))
    }
}

unsafe fn prepare_command_buffer(
    device: &ash::Device,
    command_buffer: vk::CommandBuffer,
    fence: vk::Fence,
) -> Result<()> {
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
    }
    Ok(())
}

unsafe fn submit_and_wait(
    device: &ash::Device,
    queue: vk::Queue,
    command_buffer: vk::CommandBuffer,
    fence: vk::Fence,
) -> Result<()> {
    unsafe {
        device
            .end_command_buffer(command_buffer)
            .context("ending panel command buffer")?;
        let command_buffers = [command_buffer];
        let submits = [vk::SubmitInfo::default().command_buffers(&command_buffers)];
        device
            .queue_submit(queue, &submits, fence)
            .context("submitting positioned panel upload")?;
        device
            .wait_for_fences(&[fence], true, u64::MAX)
            .context("waiting for positioned panel upload")?;
    }
    Ok(())
}

unsafe fn transition_to_compositor(
    device: &ash::Device,
    command_buffer: vk::CommandBuffer,
    image: vk::Image,
    subresource: vk::ImageSubresourceRange,
) {
    let to_compositor = [vk::ImageMemoryBarrier::default()
        .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
        .dst_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_READ)
        .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
        .new_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(image)
        .subresource_range(subresource)];
    unsafe {
        device.cmd_pipeline_barrier(
            command_buffer,
            vk::PipelineStageFlags::TRANSFER,
            vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &to_compositor,
        );
    }
}

fn prepare_upload_bytes(rgba: &[u8], format: vk::Format) -> Result<Vec<u8>> {
    let expected = PANEL_TEXTURE_WIDTH as usize * PANEL_TEXTURE_HEIGHT as usize * 4;
    if rgba.len() != expected {
        bail!("panel bitmap has {} bytes; expected {expected}", rgba.len());
    }

    match format {
        vk::Format::R8G8B8A8_SRGB | vk::Format::R8G8B8A8_UNORM => Ok(rgba.to_vec()),
        vk::Format::B8G8R8A8_SRGB | vk::Format::B8G8R8A8_UNORM => {
            let mut bgra = rgba.to_vec();
            for pixel in bgra.chunks_exact_mut(4) {
                pixel.swap(0, 2);
            }
            Ok(bgra)
        }
        _ => bail!("unsupported positioned panel upload format {format:?}"),
    }
}

fn find_memory_type(
    memory_type_bits: u32,
    properties: &vk::PhysicalDeviceMemoryProperties,
    required: vk::MemoryPropertyFlags,
) -> Option<u32> {
    properties.memory_types[..properties.memory_type_count as usize]
        .iter()
        .enumerate()
        .find_map(|(index, memory_type)| {
            let supported = memory_type_bits & (1 << index) != 0;
            (supported && memory_type.property_flags.contains(required)).then_some(index as u32)
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel_buffer(pixel: [u8; 4]) -> Vec<u8> {
        let mut bytes = vec![0; PANEL_TEXTURE_WIDTH as usize * PANEL_TEXTURE_HEIGHT as usize * 4];
        for target in bytes.chunks_exact_mut(4) {
            target.copy_from_slice(&pixel);
        }
        bytes
    }

    #[test]
    fn rgba_formats_preserve_channel_order() {
        let rgba = pixel_buffer([1, 2, 3, 4]);
        let prepared = prepare_upload_bytes(&rgba, vk::Format::R8G8B8A8_UNORM).unwrap();
        assert_eq!(&prepared[..4], &[1, 2, 3, 4]);
    }

    #[test]
    fn bgra_formats_swap_red_and_blue() {
        let rgba = pixel_buffer([1, 2, 3, 4]);
        let prepared = prepare_upload_bytes(&rgba, vk::Format::B8G8R8A8_SRGB).unwrap();
        assert_eq!(&prepared[..4], &[3, 2, 1, 4]);
    }

    #[test]
    fn upload_rejects_wrong_bitmap_size() {
        assert!(prepare_upload_bytes(&[0; 4], vk::Format::R8G8B8A8_UNORM).is_err());
    }
}
