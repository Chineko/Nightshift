use anyhow::{Context, Result, anyhow};
use ash::vk::{self, Handle};
use openxr as xr;

use crate::{
    interaction::PanelRay,
    placement::normalize_quaternion_xyzw,
    swapchain_wait::{release_image, wait_image_bounded},
};

const LASER_TEXTURE_SIZE: u32 = 1;
const LASER_THICKNESS_M: f32 = 0.002;
const CURSOR_SIZE_M: f32 = 0.008;
const POINTER_SMOOTHING_TAU_SECONDS: f32 = 0.025;
const EPSILON: f32 = 1.0e-5;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LaserSegment {
    pub ray: PanelRay,
    pub distance_m: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PointerSmoother {
    smoothed: Option<LaserSegment>,
}

impl PointerSmoother {
    pub fn update(&mut self, target: LaserSegment, alpha: f32) -> LaserSegment {
        let alpha = if alpha.is_finite() {
            alpha.clamp(0.0, 1.0)
        } else {
            1.0
        };
        let next = self
            .smoothed
            .and_then(|current| smooth_segment(current, target, alpha))
            .unwrap_or(target);
        self.smoothed = Some(next);
        next
    }

    pub fn reset(&mut self) {
        self.smoothed = None;
    }
}

pub fn smoothing_alpha(delta_seconds: f32) -> f32 {
    if !delta_seconds.is_finite() || delta_seconds <= 0.0 {
        return 1.0;
    }
    (1.0 - (-delta_seconds / POINTER_SMOOTHING_TAU_SECONDS).exp()).clamp(0.0, 1.0)
}

pub struct PointerLayers<'a> {
    pub beam: Option<xr::CompositionLayerQuad<'a, xr::Vulkan>>,
    pub cursor: xr::CompositionLayerQuad<'a, xr::Vulkan>,
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
        wait_image_bounded(&self.swapchain, "waiting for laser image")?;
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
        let release = release_image(&self.swapchain, "releasing laser image");
        result?;
        release?;
        self.initialized = true;
        Ok(())
    }

    pub fn layers<'a>(
        &'a self,
        space: &'a xr::Space,
        cursor_segment: LaserSegment,
        beam_segment: Option<LaserSegment>,
        hmd_position: [f32; 3],
        show_laser: bool,
    ) -> Option<PointerLayers<'a>> {
        let cursor_pose = cursor_pose(cursor_segment, hmd_position)?;
        let make_layer = |pose, size: xr::Extent2Df| {
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
                .size(size)
        };
        let beam = show_laser
            .then(|| beam_segment.and_then(|segment| laser_pose(segment, hmd_position)))
            .flatten()
            .map(|(beam_pose, length)| {
                make_layer(
                    beam_pose,
                    xr::Extent2Df {
                        width: length,
                        height: LASER_THICKNESS_M,
                    },
                )
            });
        let cursor = make_layer(
            cursor_pose,
            xr::Extent2Df {
                width: CURSOR_SIZE_M,
                height: CURSOR_SIZE_M,
            },
        );
        Some(PointerLayers { beam, cursor })
    }
}

fn laser_pose(segment: LaserSegment, hmd_position: [f32; 3]) -> Option<(xr::Posef, f32)> {
    if !segment.distance_m.is_finite() || segment.distance_m <= EPSILON {
        return None;
    }
    let direction = normalize(segment.ray.direction)?;
    if !segment.ray.origin.iter().all(|value| value.is_finite())
        || !hmd_position.iter().all(|value| value.is_finite())
    {
        return None;
    }
    let midpoint = add(
        segment.ray.origin,
        scale(direction, segment.distance_m * 0.5),
    );
    let to_hmd = normalize(subtract(hmd_position, midpoint))?;
    let projected_view = subtract(to_hmd, scale(direction, dot(to_hmd, direction)));
    let normal = normalize(projected_view)
        .or_else(|| normalize(cross(direction, [0.0, 1.0, 0.0])))
        .or_else(|| normalize(cross(direction, [1.0, 0.0, 0.0])))?;
    let up = normalize(cross(normal, direction))?;
    let orientation = quaternion_from_axes(direction, up, normal)?;
    Some((xr_pose(midpoint, orientation), segment.distance_m))
}

fn cursor_pose(segment: LaserSegment, hmd_position: [f32; 3]) -> Option<xr::Posef> {
    if !segment.distance_m.is_finite() || segment.distance_m < 0.0 {
        return None;
    }
    let direction = normalize(segment.ray.direction)?;
    let hit = add(segment.ray.origin, scale(direction, segment.distance_m));
    let to_hmd =
        normalize(subtract(hmd_position, hit)).or_else(|| normalize(scale(direction, -1.0)))?;
    let orientation = quaternion_from_z_axis(to_hmd)?;
    Some(xr_pose(hit, orientation))
}

fn smooth_segment(current: LaserSegment, target: LaserSegment, alpha: f32) -> Option<LaserSegment> {
    let current_end = segment_end(current)?;
    let target_end = segment_end(target)?;
    segment_from_points(
        lerp(current.ray.origin, target.ray.origin, alpha),
        lerp(current_end, target_end, alpha),
    )
}

fn segment_end(segment: LaserSegment) -> Option<[f32; 3]> {
    let direction = normalize(segment.ray.direction)?;
    Some(add(
        segment.ray.origin,
        scale(direction, segment.distance_m),
    ))
}

fn segment_from_points(origin: [f32; 3], end: [f32; 3]) -> Option<LaserSegment> {
    let delta = subtract(end, origin);
    let distance_m = length(delta)?;
    Some(LaserSegment {
        ray: PanelRay {
            origin,
            direction: scale(delta, distance_m.recip()),
        },
        distance_m,
    })
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

fn quaternion_from_z_axis(direction: [f32; 3]) -> Option<[f32; 4]> {
    let dot = direction[2];
    if dot < -0.9999 {
        return Some([0.0, 1.0, 0.0, 0.0]);
    }
    let cross = [-direction[1], direction[0], 0.0];
    normalize_quaternion_xyzw([cross[0], cross[1], cross[2], 1.0 + dot])
}

fn quaternion_rotate_vector(q: [f32; 4], vector: [f32; 3]) -> [f32; 3] {
    let qv = [vector[0], vector[1], vector[2], 0.0];
    let conjugate = [-q[0], -q[1], -q[2], q[3]];
    let rotated = quaternion_multiply(quaternion_multiply(q, qv), conjugate);
    [rotated[0], rotated[1], rotated[2]]
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

fn add(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
    [left[0] + right[0], left[1] + right[1], left[2] + right[2]]
}

fn subtract(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
    [left[0] - right[0], left[1] - right[1], left[2] - right[2]]
}

fn scale(vector: [f32; 3], scalar: f32) -> [f32; 3] {
    [vector[0] * scalar, vector[1] * scalar, vector[2] * scalar]
}

fn dot(left: [f32; 3], right: [f32; 3]) -> f32 {
    left[0] * right[0] + left[1] * right[1] + left[2] * right[2]
}

fn cross(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
    [
        left[1] * right[2] - left[2] * right[1],
        left[2] * right[0] - left[0] * right[2],
        left[0] * right[1] - left[1] * right[0],
    ]
}

fn quaternion_from_axes(x: [f32; 3], y: [f32; 3], z: [f32; 3]) -> Option<[f32; 4]> {
    let trace = x[0] + y[1] + z[2];
    let quaternion = if trace > 0.0 {
        let s = (trace + 1.0).sqrt() * 2.0;
        [
            (y[2] - z[1]) / s,
            (z[0] - x[2]) / s,
            (x[1] - y[0]) / s,
            0.25 * s,
        ]
    } else if x[0] > y[1] && x[0] > z[2] {
        let s = (1.0 + x[0] - y[1] - z[2]).sqrt() * 2.0;
        [
            0.25 * s,
            (y[0] + x[1]) / s,
            (z[0] + x[2]) / s,
            (y[2] - z[1]) / s,
        ]
    } else if y[1] > z[2] {
        let s = (1.0 + y[1] - x[0] - z[2]).sqrt() * 2.0;
        [
            (y[0] + x[1]) / s,
            0.25 * s,
            (z[1] + y[2]) / s,
            (z[0] - x[2]) / s,
        ]
    } else {
        let s = (1.0 + z[2] - x[0] - y[1]).sqrt() * 2.0;
        [
            (z[0] + x[2]) / s,
            (z[1] + y[2]) / s,
            0.25 * s,
            (x[1] - y[0]) / s,
        ]
    };
    normalize_quaternion_xyzw(quaternion)
}

fn lerp(from: [f32; 3], to: [f32; 3], alpha: f32) -> [f32; 3] {
    add(from, scale(subtract(to, from), alpha))
}

fn length(vector: [f32; 3]) -> Option<f32> {
    let length_sq = dot(vector, vector);
    if !length_sq.is_finite() || length_sq <= EPSILON * EPSILON {
        return None;
    }
    Some(length_sq.sqrt())
}

fn normalize(vector: [f32; 3]) -> Option<[f32; 3]> {
    if !vector.iter().all(|value| value.is_finite()) {
        return None;
    }
    let length_sq = dot(vector, vector);
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
        let (first, length) = laser_pose(segment, [0.0, 1.6, 0.5]).unwrap();
        assert!((length - 2.0).abs() < 1.0e-5);
        assert!((first.position.y - 1.0).abs() < 1.0e-5);
        assert!((first.position.z + 1.0).abs() < 1.0e-5);
    }

    #[test]
    fn invalid_beam_geometry_fails_closed() {
        assert!(
            laser_pose(
                LaserSegment {
                    ray: PanelRay {
                        origin: [0.0; 3],
                        direction: [0.0; 3]
                    },
                    distance_m: 1.0,
                },
                [0.0, 1.6, 0.0]
            )
            .is_none()
        );
    }

    #[test]
    fn smoothing_is_frame_rate_aware_and_stays_bounded() {
        let at_90_hz = smoothing_alpha(1.0 / 90.0);
        let at_120_hz = smoothing_alpha(1.0 / 120.0);
        assert!(at_90_hz > at_120_hz);
        assert!(at_90_hz > 0.0 && at_90_hz < 1.0);
    }

    #[test]
    fn cursor_sits_slightly_in_front_of_hit_point() {
        let segment = LaserSegment {
            ray: PanelRay {
                origin: [0.0, 1.6, 0.0],
                direction: [0.0, 0.0, -1.0],
            },
            distance_m: 1.0,
        };
        let pose = cursor_pose(segment, [0.0, 1.6, 0.0]).unwrap();
        assert!((pose.position.z + 1.0).abs() < 1.0e-5);
    }
}

#[cfg(test)]
mod audited_pointer_geometry_tests {
    use super::*;

    #[test]
    fn beam_normal_faces_hmd_continuously() {
        let segment = LaserSegment {
            ray: PanelRay {
                origin: [0.0, 0.0, 0.0],
                direction: [1.0, 0.0, 0.0],
            },
            distance_m: 1.0,
        };
        let pose = laser_pose(segment, [0.5, 1.0, 0.7]).unwrap().0;
        let q = [
            pose.orientation.x,
            pose.orientation.y,
            pose.orientation.z,
            pose.orientation.w,
        ];
        let normal = quaternion_rotate_vector(q, [0.0, 0.0, 1.0]);
        let midpoint = [0.5, 0.0, 0.0];
        let to_hmd = normalize(subtract([0.5, 1.0, 0.7], midpoint)).unwrap();
        assert!(dot(normal, to_hmd) > 0.99);
    }
}
