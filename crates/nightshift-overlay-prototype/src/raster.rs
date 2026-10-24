use font8x8::{BASIC_FONTS, UnicodeFonts};

use crate::content::PanelContent;

pub const PANEL_BITMAP_WIDTH: u32 = 768;
pub const PANEL_BITMAP_HEIGHT: u32 = 448;

const BACKGROUND: [u8; 4] = [9, 10, 14, 255];
const TITLE: [u8; 4] = [190, 160, 255, 255];
const TEXT: [u8; 4] = [236, 238, 245, 255];
const DIVIDER: [u8; 4] = [58, 61, 75, 255];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanelBitmap {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl PanelBitmap {
    pub fn from_content(content: &PanelContent) -> Self {
        let mut bitmap = Self {
            width: PANEL_BITMAP_WIDTH,
            height: PANEL_BITMAP_HEIGHT,
            rgba: vec![0; PANEL_BITMAP_WIDTH as usize * PANEL_BITMAP_HEIGHT as usize * 4],
        };
        bitmap.fill(BACKGROUND);
        bitmap.draw_text(36, 28, &content.lines[0], 3, TITLE);
        bitmap.fill_rect(36, 66, PANEL_BITMAP_WIDTH - 72, 2, DIVIDER);

        for (index, line) in content.lines.iter().skip(1).enumerate() {
            bitmap.draw_text(36, 92 + index as u32 * 58, line, 2, TEXT);
        }
        bitmap
    }

    fn fill(&mut self, color: [u8; 4]) {
        for pixel in self.rgba.chunks_exact_mut(4) {
            pixel.copy_from_slice(&color);
        }
    }

    fn fill_rect(&mut self, x: u32, y: u32, width: u32, height: u32, color: [u8; 4]) {
        let x_end = x.saturating_add(width).min(self.width);
        let y_end = y.saturating_add(height).min(self.height);
        for py in y..y_end {
            for px in x..x_end {
                self.put_pixel(px, py, color);
            }
        }
    }

    fn draw_text(&mut self, x: u32, y: u32, text: &str, scale: u32, color: [u8; 4]) {
        let mut cursor_x = x;
        let advance = 9 * scale;
        for character in text.chars() {
            if cursor_x.saturating_add(8 * scale) > self.width {
                break;
            }
            if let Some(glyph) = BASIC_FONTS.get(character) {
                self.draw_glyph(cursor_x, y, glyph, scale, color);
            }
            cursor_x = cursor_x.saturating_add(advance);
        }
    }

    fn draw_glyph(&mut self, x: u32, y: u32, glyph: [u8; 8], scale: u32, color: [u8; 4]) {
        for (row, bits) in glyph.into_iter().enumerate() {
            for column in 0..8_u32 {
                if bits & (1 << column) == 0 {
                    continue;
                }
                let px = x + column * scale;
                let py = y + row as u32 * scale;
                self.fill_rect(px, py, scale, scale, color);
            }
        }
    }

    fn put_pixel(&mut self, x: u32, y: u32, color: [u8; 4]) {
        if x >= self.width || y >= self.height {
            return;
        }
        let offset = ((y * self.width + x) * 4) as usize;
        self.rgba[offset..offset + 4].copy_from_slice(&color);
    }
}

#[cfg(test)]
mod tests {
    use nightshift_protocol::AppState;

    use super::*;

    #[test]
    fn raster_has_expected_rgba_dimensions() {
        let bitmap = PanelBitmap::from_content(&PanelContent::from_state(&AppState::default()));
        assert_eq!(bitmap.width, PANEL_BITMAP_WIDTH);
        assert_eq!(bitmap.height, PANEL_BITMAP_HEIGHT);
        assert_eq!(bitmap.rgba.len(), (PANEL_BITMAP_WIDTH * PANEL_BITMAP_HEIGHT * 4) as usize);
    }

    #[test]
    fn raster_contains_visible_text_pixels() {
        let bitmap = PanelBitmap::from_content(&PanelContent::from_state(&AppState::default()));
        assert!(bitmap.rgba.chunks_exact(4).any(|pixel| pixel == TITLE));
        assert!(bitmap.rgba.chunks_exact(4).any(|pixel| pixel == TEXT));
        assert!(bitmap.rgba.chunks_exact(4).any(|pixel| pixel == DIVIDER));
    }

    #[test]
    fn long_lines_clip_without_resizing_or_panicking() {
        let mut content = PanelContent::from_state(&AppState::default());
        content.lines[5] = "X".repeat(500);
        let bitmap = PanelBitmap::from_content(&content);
        assert_eq!(bitmap.rgba.len(), (PANEL_BITMAP_WIDTH * PANEL_BITMAP_HEIGHT * 4) as usize);
    }
}
