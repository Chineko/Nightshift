use anyhow::{Context, Result, anyhow};
use ash::vk::{self, Handle};
use openxr as xr;

use crate::{interaction::PanelRay, placement::normalize_quaternion_xyzw};

const LASER_TEXTURE_SIZE: u32 = 1;
const LASER_THICKNESS_M: f32 = 0.004;
const EPSILON: f32 = 1.0e-5;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LaserSegment {
    pub ray: PanelRay,
    pub distance_m: f32,
}

pub struct LaserRenderer {
    swapchain: xr::Swapchain<xr::Vulkan>,
    images: Vec<vk::Image>,
    rect: xr::Rect2Di,
    subresource: vk::ImageSubresourceRange,
    initialized: bool,
}

impl LaserRenderer {
    pub fn new(session: &xr::Session<xr::Vulkan>, format: vk::Format) -> Result<Self> {
        let swapchain = session
            .create_swapchain(&xr::SwapchainCreateInfo {
                create_flags: xr::SwapchainCreateFlags::EMPTY,
                usage_flags: xr::SwapchainUsageFlags::COLOR_ATTACHMENT
                    | xr::SwapchainUsageFlags::TRANSFER_DST,
                format: format.as_raw() as u32,
                sample_count: 1,
                width: LASER_TEXTURE_SIZE,
                height: LASER_TEXTURE_SIZE,
                face_count: 1,
                array_size: 1,
                mip_count: 1,
            })
            .context("creating laser swapchain")?;
        let images = swapchain
            .enumerate_images()
            .context("enumerating laser swapchain images")?
            .into_iter()
            .map(vk::Image::from_raw)
            .collect::<Vec<_>>();
        Ok(Self {
            swapchain,
            images,
            rect: xr::Rect2Di {
                offset: xr::Offset2Di { x: 0, y: 0 },
                extent: xr::Extent2Di {
                    width: LASER_TEXTURE_SIZE as i32,
                    height: LASER_TEXTURE_SIZE as i32,
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
            .context("acquiring laser image")?;
        self.swapchain
            .wait_image(xr::Duration::INFINITE)
            .context("waiting for laser image")?;
        let result = unsafe {
            (|| -> Result<()> {
                let image = *self.images.get(image_index as usize).ok_or_else(|| {
                    anyhow!("runtime returned invalid laser image index {image_index}")
                })?;
                device.wait_for_fences(&[fence], true, u64::MAX)?;
                device.reset_fences(&[fence])?;
                device
                    .reset_command_buffer(command_buffer, vk::CommandBufferResetFlags::empty())?;
                device.begin_command_buffer(
                    command_buffer,
                    &vk::CommandBufferBeginInfo::default()
                        .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
                )?;
                let to_transfer = [vk::ImageMemoryBarrier::default()
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
                        float32: [0.72, 0.42, 1.0, 0.92],
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
                device.end_command_buffer(command_buffer)?;
                let command_buffers = [command_buffer];
                let submits = [vk::SubmitInfo::default().command_buffers(&command_buffers)];
                device.queue_submit(queue, &submits, fence)?;
                device.wait_for_fences(&[fence], true, u64::MAX)?;
                Ok(())
            })()
        };
        let release = self
            .swapchain
            .release_image()
            .context("releasing laser image");
        result?;
        release?;
        self.initialized = true;
        Ok(())
    }

    pub fn layers<'a>(
        &'a self,
        space: &'a xr::Space,
        segment: LaserSegment,
    ) -> Option<[xr::CompositionLayerQuad<'a, xr::Vulkan>; 2]> {
        let (first_pose, second_pose, length) = laser_poses(segment)?;
        let make_layer = |pose| {
            xr::CompositionLayerQuad::new()
                .layer_flags(xr::CompositionLayerFlags::BLEND_TEXTURE_SOURCE_ALPHA)
                .pose(pose)
                .sub_image(
                    xr::SwapchainSubImage::new()
                        .swapchain(&self.swapchain)
                        .image_array_index(0)
                        .image_rect(self.rect),
                )
                .eye_visibility(xr::EyeVisibility::BOTH)
                .space(space)
                .size(xr::Extent2Df {
                    width: length,
                    height: LASER_THICKNESS_M,
                })
        };
        Some([make_layer(first_pose), make_layer(second_pose)])
    }
}

fn laser_poses(segment: LaserSegment) -> Option<(xr::Posef, xr::Posef, f32)> {
    if !segment.distance_m.is_finite() || segment.distance_m <= EPSILON {
        return None;
    }
    let direction = normalize(segment.ray.direction)?;
    if !segment.ray.origin.iter().all(|value| value.is_finite()) {
        return None;
    }
    let midpoint = [
        segment.ray.origin[0] + direction[0] * segment.distance_m * 0.5,
        segment.ray.origin[1] + direction[1] * segment.distance_m * 0.5,
        segment.ray.origin[2] + direction[2] * segment.distance_m * 0.5,
    ];
    let base = quaternion_from_x_axis(direction)?;
    let quarter_turn_x = [
        std::f32::consts::FRAC_1_SQRT_2,
        0.0,
        0.0,
        std::f32::consts::FRAC_1_SQRT_2,
    ];
    let crossed = normalize_quaternion_xyzw(quaternion_multiply(base, quarter_turn_x))?;
    Some((
        xr_pose(midpoint, base),
        xr_pose(midpoint, crossed),
        segment.distance_m,
    ))
}

fn xr_pose(position: [f32; 3], orientation: [f32; 4]) -> xr::Posef {
    xr::Posef {
        orientation: xr::Quaternionf {
            x: orientation[0],
            y: orientation[1],
            z: orientation[2],
            w: orientation[3],
        },
        position: xr::Vector3f {
            x: position[0],
            y: position[1],
            z: position[2],
        },
    }
}

fn quaternion_from_x_axis(direction: [f32; 3]) -> Option<[f32; 4]> {
    let dot = direction[0];
    if dot < -0.9999 {
        return Some([0.0, 1.0, 0.0, 0.0]);
    }
    let cross = [0.0, -direction[2], direction[1]];
    normalize_quaternion_xyzw([cross[0], cross[1], cross[2], 1.0 + dot])
}

fn quaternion_multiply(left: [f32; 4], right: [f32; 4]) -> [f32; 4] {
    let [ax, ay, az, aw] = left;
    let [bx, by, bz, bw] = right;
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

fn normalize(vector: [f32; 3]) -> Option<[f32; 3]> {
    if !vector.iter().all(|value| value.is_finite()) {
        return None;
    }
    let length_sq = vector[0] * vector[0] + vector[1] * vector[1] + vector[2] * vector[2];
    if !length_sq.is_finite() || length_sq <= EPSILON * EPSILON {
        return None;
    }
    let inverse = length_sq.sqrt().recip();
    Some([
        vector[0] * inverse,
        vector[1] * inverse,
        vector[2] * inverse,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beam_midpoint_and_length_follow_ray() {
        let segment = LaserSegment {
            ray: PanelRay {
                origin: [0.0, 1.0, 0.0],
                direction: [0.0, 0.0, -1.0],
            },
            distance_m: 2.0,
        };
        let (first, _, length) = laser_poses(segment).unwrap();
        assert!((length - 2.0).abs() < 1.0e-5);
        assert!((first.position.y - 1.0).abs() < 1.0e-5);
        assert!((first.position.z + 1.0).abs() < 1.0e-5);
    }

    #[test]
    fn invalid_beam_geometry_fails_closed() {
        assert!(
            laser_poses(LaserSegment {
                ray: PanelRay {
                    origin: [0.0; 3],
                    direction: [0.0; 3]
                },
                distance_m: 1.0,
            })
            .is_none()
        );
    }
}
