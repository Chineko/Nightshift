use super::{BORDER, PanelBitmap, SHADOW};

impl PanelBitmap {
    pub(super) fn glass_panel(
        &mut self,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        radius: u32,
        tint: [u8; 4],
    ) {
        // Fake the expensive part of glass until profiling gives us permission to buy an airbrush robot.
        self.rounded_rect_fill(
            x + 3,
            y + 5,
            width,
            height,
            radius,
            [SHADOW[0], SHADOW[1], SHADOW[2], 185],
        );
        self.rounded_rect_fill(x, y, width, height, radius, BORDER);
        if width <= 4 || height <= 4 {
            return;
        }

        let inner_x = x + 2;
        let inner_y = y + 2;
        let inner_width = width - 4;
        let inner_height = height - 4;
        let inner_radius = radius.saturating_sub(2);
        let top = brighten(tint, 14);
        let bottom = darken(tint, 8);
        self.rounded_vertical_gradient(
            inner_x,
            inner_y,
            inner_width,
            inner_height,
            inner_radius,
            top,
            bottom,
        );

        if width > 12 {
            self.fill_rect(
                x + radius.max(6),
                y + 2,
                width.saturating_sub(radius.max(6) * 2),
                1,
                [255, 255, 255, 34],
            );
        }
    }

    fn rounded_vertical_gradient(
        &mut self,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        radius: u32,
        top: [u8; 4],
        bottom: [u8; 4],
    ) {
        let radius = radius.min(width / 2).min(height / 2) as i32;
        let denominator = height.saturating_sub(1).max(1);
        for py in 0..height {
            let color = interpolate(top, bottom, py, denominator);
            for px in 0..width {
                let px_i = px as i32;
                let py_i = py as i32;
                let dx = if px_i < radius {
                    radius - px_i
                } else if px_i >= width as i32 - radius {
                    px_i - (width as i32 - radius - 1)
                } else {
                    0
                };
                let dy = if py_i < radius {
                    radius - py_i
                } else if py_i >= height as i32 - radius {
                    py_i - (height as i32 - radius - 1)
                } else {
                    0
                };
                if dx == 0 || dy == 0 || dx * dx + dy * dy <= radius * radius {
                    self.blend_pixel(x + px, y + py, color);
                }
            }
        }
    }
}

fn interpolate(start: [u8; 4], end: [u8; 4], position: u32, length: u32) -> [u8; 4] {
    let mut color = [0; 4];
    for channel in 0..4 {
        let start = start[channel] as i32;
        let delta = end[channel] as i32 - start;
        color[channel] = (start + delta * position as i32 / length as i32).clamp(0, 255) as u8;
    }
    color
}

fn brighten(mut color: [u8; 4], amount: u8) -> [u8; 4] {
    for channel in &mut color[..3] {
        *channel = channel.saturating_add(amount);
    }
    color
}

fn darken(mut color: [u8; 4], amount: u8) -> [u8; 4] {
    for channel in &mut color[..3] {
        *channel = channel.saturating_sub(amount);
    }
    color
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gradient_reaches_both_ends() {
        let top = [20, 30, 40, 120];
        let bottom = [5, 10, 15, 200];
        assert_eq!(interpolate(top, bottom, 0, 10), top);
        assert_eq!(interpolate(top, bottom, 10, 10), bottom);
    }

    #[test]
    fn tint_helpers_saturate_instead_of_summoning_color_overflow_goblins() {
        assert_eq!(brighten([250, 1, 2, 255], 20), [255, 21, 22, 255]);
        assert_eq!(darken([3, 4, 5, 255], 10), [0, 0, 0, 255]);
    }
}
