use super::{PanelBitmap, font::glyph5x7};

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
        let r = radius.min(width / 2).min(height / 2) as i32;
        for py in 0..height {
            for px in 0..width {
                let px_i = px as i32;
                let py_i = py as i32;
                let dx = if px_i < r {
                    r - px_i
                } else if px_i >= width as i32 - r {
                    px_i - (width as i32 - r - 1)
                } else {
                    0
                };
                let dy = if py_i < r {
                    r - py_i
                } else if py_i >= height as i32 - r {
                    py_i - (height as i32 - r - 1)
                } else {
                    0
                };
                if dx == 0 || dy == 0 || dx * dx + dy * dy <= r * r {
                    self.put_pixel(x + px, y + py, color);
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
        for dy in -r..=r {
            for dx in -r..=r {
                if dx * dx + dy * dy <= r * r {
                    let x = cx as i32 + dx;
                    let y = cy as i32 + dy;
                    if x >= 0 && y >= 0 {
                        self.put_pixel(x as u32, y as u32, color);
                    }
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
        let mut cursor_x = x;
        let advance = 6 * scale;
        for character in text.chars() {
            if cursor_x.saturating_add(5 * scale) > self.width {
                break;
            }
            self.draw_glyph(cursor_x, y, glyph5x7(character), scale, color);
            cursor_x = cursor_x.saturating_add(advance);
        }
    }

    pub(super) fn draw_glyph(
        &mut self,
        x: u32,
        y: u32,
        glyph: [u8; 7],
        scale: u32,
        color: [u8; 4],
    ) {
        for (row, bits) in glyph.into_iter().enumerate() {
            for column in 0..5_u32 {
                let mask = 1 << (4 - column);
                if bits & mask != 0 {
                    self.fill_rect(
                        x + column * scale,
                        y + row as u32 * scale,
                        scale,
                        scale,
                        color,
                    );
                }
            }
        }
    }

    pub(super) fn put_pixel(&mut self, x: u32, y: u32, color: [u8; 4]) {
        if x >= self.width || y >= self.height {
            return;
        }
        let offset = ((y * self.width + x) * 4) as usize;
        self.rgba[offset..offset + 4].copy_from_slice(&color);
    }
}
