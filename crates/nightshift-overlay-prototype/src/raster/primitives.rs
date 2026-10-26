use super::{
    PanelBitmap,
    text::{measure_text_width, rasterize_text},
};

const SAMPLE_OFFSETS: [(f32, f32); 4] = [
    (0.25, 0.25),
    (0.75, 0.25),
    (0.25, 0.75),
    (0.75, 0.75),
];

impl PanelBitmap {
    pub(super) fn rounded_rect(
        &mut self,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        radius: u32,
        fill: [u8; 4],
        border: [u8; 4],
    ) {
        self.rounded_rect_fill(x, y, width, height, radius, border);
        if width > 4 && height > 4 {
            self.rounded_rect_fill(
                x + 2,
                y + 2,
                width - 4,
                height - 4,
                radius.saturating_sub(2),
                fill,
            );
        }
    }

    pub(super) fn rounded_rect_fill(
        &mut self,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        radius: u32,
        color: [u8; 4],
    ) {
        if width == 0 || height == 0 {
            return;
        }
        let radius = radius.min(width / 2).min(height / 2) as f32;
        for py in 0..height {
            for px in 0..width {
                let coverage = SAMPLE_OFFSETS
                    .iter()
                    .filter(|(sample_x, sample_y)| {
                        rounded_rect_contains(
                            px as f32 + sample_x,
                            py as f32 + sample_y,
                            width as f32,
                            height as f32,
                            radius,
                        )
                    })
                    .count() as u8;
                if coverage > 0 {
                    self.blend_pixel(x + px, y + py, covered_color(color, coverage));
                }
            }
        }
    }

    pub(super) fn draw_line(&mut self, x0: u32, y0: u32, x1: u32, y1: u32, color: [u8; 4]) {
        if y0 == y1 {
            self.fill_rect(x0.min(x1), y0, x0.max(x1) - x0.min(x1) + 1, 1, color);
        } else if x0 == x1 {
            self.fill_rect(x0, y0.min(y1), 1, y0.max(y1) - y0.min(y1) + 1, color);
        }
    }

    pub(super) fn draw_circle(&mut self, cx: u32, cy: u32, radius: u32, color: [u8; 4]) {
        let r = radius as i32;
        let radius_sq = radius as f32 * radius as f32;
        for dy in -(r + 1)..=(r + 1) {
            for dx in -(r + 1)..=(r + 1) {
                let coverage = SAMPLE_OFFSETS
                    .iter()
                    .filter(|(sample_x, sample_y)| {
                        let x = dx as f32 + sample_x - 0.5;
                        let y = dy as f32 + sample_y - 0.5;
                        x * x + y * y <= radius_sq
                    })
                    .count() as u8;
                if coverage == 0 {
                    continue;
                }
                let x = cx as i32 + dx;
                let y = cy as i32 + dy;
                if x >= 0 && y >= 0 {
                    self.blend_pixel(x as u32, y as u32, covered_color(color, coverage));
                }
            }
        }
    }

    pub(super) fn draw_crescent(
        &mut self,
        cx: u32,
        cy: u32,
        radius: u32,
        color: [u8; 4],
        cutout: [u8; 4],
    ) {
        self.draw_circle(cx, cy, radius, color);
        self.draw_circle(
            cx + radius / 2,
            cy.saturating_sub(radius / 4),
            radius.saturating_sub(2),
            cutout,
        );
    }

    pub(super) fn draw_sun(&mut self, cx: u32, cy: u32, radius: u32, color: [u8; 4]) {
        self.draw_circle(cx, cy, radius / 2, color);
        let r = radius + 6;
        self.fill_rect(cx.saturating_sub(1), cy.saturating_sub(r), 3, 6, color);
        self.fill_rect(cx.saturating_sub(1), cy + r.saturating_sub(5), 3, 6, color);
        self.fill_rect(cx.saturating_sub(r), cy.saturating_sub(1), 6, 3, color);
        self.fill_rect(cx + r.saturating_sub(5), cy.saturating_sub(1), 6, 3, color);
    }

    pub(super) fn draw_lightning(&mut self, cx: u32, cy: u32, color: [u8; 4]) {
        self.fill_rect(cx.saturating_sub(3), cy.saturating_sub(18), 7, 16, color);
        self.fill_rect(cx.saturating_sub(10), cy.saturating_sub(4), 14, 7, color);
        self.fill_rect(cx.saturating_sub(3), cy, 7, 18, color);
    }

    pub(super) fn fill(&mut self, color: [u8; 4]) {
        for pixel in self.rgba.chunks_exact_mut(4) {
            pixel.copy_from_slice(&color);
        }
    }

    pub(super) fn fill_rect(&mut self, x: u32, y: u32, width: u32, height: u32, color: [u8; 4]) {
        let x_end = x.saturating_add(width).min(self.width);
        let y_end = y.saturating_add(height).min(self.height);
        for py in y..y_end {
            for px in x..x_end {
                self.put_pixel(px, py, color);
            }
        }
    }

    pub(super) fn draw_text(&mut self, x: u32, y: u32, text: &str, scale: u32, color: [u8; 4]) {
        let max_width = self.width.saturating_sub(x);
        rasterize_text(text, scale, max_width, color, |px, py, width, height, color| {
            let px = px + x as i32;
            let py = py + y as i32;
            if px < 0 || py < 0 {
                return;
            }
            for offset_y in 0..height {
                for offset_x in 0..width {
                    self.blend_pixel(px as u32 + offset_x, py as u32 + offset_y, color);
                }
            }
        });
    }

    pub(super) fn draw_text_centered(
        &mut self,
        x: u32,
        y: u32,
        width: u32,
        text: &str,
        scale: u32,
        color: [u8; 4],
    ) {
        let measured = measure_text_width(text, scale, width).min(width);
        let text_x = x + width.saturating_sub(measured) / 2;
        self.draw_text(text_x, y, text, scale, color);
    }

    pub(super) fn blend_pixel(&mut self, x: u32, y: u32, source: [u8; 4]) {
        if x >= self.width || y >= self.height || source[3] == 0 {
            return;
        }
        if source[3] == 255 {
            self.put_pixel(x, y, source);
            return;
        }

        let offset = ((y * self.width + x) * 4) as usize;
        let alpha = source[3] as u32;
        let inverse = 255_u32 - alpha;
        for channel in 0..3 {
            let blended = source[channel] as u32 * alpha
                + self.rgba[offset + channel] as u32 * inverse;
            self.rgba[offset + channel] = (blended / 255) as u8;
        }
        self.rgba[offset + 3] = 255;
    }

    pub(super) fn put_pixel(&mut self, x: u32, y: u32, color: [u8; 4]) {
        if x >= self.width || y >= self.height {
            return;
        }
        let offset = ((y * self.width + x) * 4) as usize;
        self.rgba[offset..offset + 4].copy_from_slice(&color);
    }
}

fn rounded_rect_contains(x: f32, y: f32, width: f32, height: f32, radius: f32) -> bool {
    if radius <= 0.0 {
        return x >= 0.0 && x <= width && y >= 0.0 && y <= height;
    }
    let center_x = x.clamp(radius, width - radius);
    let center_y = y.clamp(radius, height - radius);
    let dx = x - center_x;
    let dy = y - center_y;
    dx * dx + dy * dy <= radius * radius
}

fn covered_color(mut color: [u8; 4], coverage: u8) -> [u8; 4] {
    color[3] = ((color[3] as u16 * coverage as u16) / SAMPLE_OFFSETS.len() as u16) as u8;
    color
}
