use super::{content::PanelContent, interaction::PanelAction};

pub const PANEL_BITMAP_WIDTH: u32 = 768;
pub const PANEL_BITMAP_HEIGHT: u32 = 448;

const BACKGROUND: [u8; 4] = [9, 10, 14, 255];
const TITLE: [u8; 4] = [190, 160, 255, 255];
const TEXT: [u8; 4] = [236, 238, 245, 255];
const DIVIDER: [u8; 4] = [58, 61, 75, 255];
const BUTTON_FILL: [u8; 4] = [34, 36, 48, 255];
const BUTTON_HOVER_FILL: [u8; 4] = [62, 52, 86, 255];
const BUTTON_HOVER_BORDER: [u8; 4] = [190, 160, 255, 255];
const BUTTON_BORDER: [u8; 4] = [92, 82, 122, 255];
const BUTTON_TEXT: [u8; 4] = [228, 220, 248, 255];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanelBitmap {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl PanelBitmap {
    pub fn from_content(content: &PanelContent) -> Self {
        Self::from_content_with_hover(content, None)
    }

    pub fn from_content_with_hover(content: &PanelContent, hover: Option<PanelAction>) -> Self {
        let mut bitmap = Self {
            width: PANEL_BITMAP_WIDTH,
            height: PANEL_BITMAP_HEIGHT,
            rgba: vec![0; PANEL_BITMAP_WIDTH as usize * PANEL_BITMAP_HEIGHT as usize * 4],
        };
        bitmap.fill(BACKGROUND);
        bitmap.draw_text(36, 28, &content.lines[0], 5, TITLE);
        bitmap.fill_rect(36, 72, PANEL_BITMAP_WIDTH - 72, 2, DIVIDER);

        for (index, line) in content.lines.iter().skip(1).enumerate() {
            bitmap.draw_text(36, 100 + index as u32 * 60, line, 3, TEXT);
        }
        bitmap.draw_button(
            36,
            388,
            165,
            44,
            "SLEEP",
            hover == Some(PanelAction::EnterSleep),
        );
        bitmap.draw_button(213, 388, 165, 44, "WAKE", hover == Some(PanelAction::Wake));
        bitmap.draw_button(
            390,
            388,
            165,
            44,
            "DARKER",
            hover == Some(PanelAction::BrightnessDown),
        );
        bitmap.draw_button(
            567,
            388,
            165,
            44,
            "BRIGHT",
            hover == Some(PanelAction::BrightnessUp),
        );
        bitmap
    }

    fn draw_button(&mut self, x: u32, y: u32, width: u32, height: u32, label: &str, hovered: bool) {
        let border = if hovered {
            BUTTON_HOVER_BORDER
        } else {
            BUTTON_BORDER
        };
        let fill = if hovered {
            BUTTON_HOVER_FILL
        } else {
            BUTTON_FILL
        };
        self.fill_rect(x, y, width, height, border);
        self.fill_rect(
            x + 2,
            y + 2,
            width.saturating_sub(4),
            height.saturating_sub(4),
            fill,
        );
        self.draw_text(x + 18, y + 11, label, 3, BUTTON_TEXT);
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
        let advance = 6 * scale;
        for character in text.chars() {
            if cursor_x.saturating_add(5 * scale) > self.width {
                break;
            }
            self.draw_glyph(cursor_x, y, glyph5x7(character), scale, color);
            cursor_x = cursor_x.saturating_add(advance);
        }
    }

    fn draw_glyph(&mut self, x: u32, y: u32, glyph: [u8; 7], scale: u32, color: [u8; 4]) {
        for (row, bits) in glyph.into_iter().enumerate() {
            for column in 0..5_u32 {
                let mask = 1 << (4 - column);
                if bits & mask == 0 {
                    continue;
                }
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

    fn put_pixel(&mut self, x: u32, y: u32, color: [u8; 4]) {
        if x >= self.width || y >= self.height {
            return;
        }
        let offset = ((y * self.width + x) * 4) as usize;
        self.rgba[offset..offset + 4].copy_from_slice(&color);
    }
}

fn glyph5x7(character: char) -> [u8; 7] {
    match character.to_ascii_uppercase() {
        'A' => [
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'B' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ],
        'C' => [
            0b01111, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b01111,
        ],
        'D' => [
            0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
        ],
        'E' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
        'F' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'G' => [
            0b01111, 0b10000, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111,
        ],
        'H' => [
            0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'I' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b11111,
        ],
        'J' => [
            0b00111, 0b00010, 0b00010, 0b00010, 0b10010, 0b10010, 0b01100,
        ],
        'K' => [
            0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
        ],
        'L' => [
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ],
        'M' => [
            0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
        ],
        'N' => [
            0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001,
        ],
        'O' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'P' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'Q' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101,
        ],
        'R' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ],
        'S' => [
            0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        'T' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b11111,
        ],
        'U' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'V' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
        ],
        'W' => [
            0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b10101, 0b01010,
        ],
        'X' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
        ],
        'Y' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'Z' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
        ],
        '0' => [
            0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
        '1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        '2' => [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
        ],
        '3' => [
            0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        '4' => [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
        '5' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b00001, 0b00001, 0b11110,
        ],
        '6' => [
            0b01110, 0b10000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
        '7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        '8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        '9' => [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b01110,
        ],
        ':' => [
            0b00000, 0b00100, 0b00100, 0b00000, 0b00100, 0b00100, 0b00000,
        ],
        '%' => [
            0b11001, 0b11010, 0b00100, 0b01000, 0b10110, 0b10011, 0b00000,
        ],
        '-' => [
            0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000,
        ],
        '>' => [
            0b10000, 0b01000, 0b00100, 0b00010, 0b00100, 0b01000, 0b10000,
        ],
        '|' => [
            0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        '(' => [
            0b00010, 0b00100, 0b01000, 0b01000, 0b01000, 0b00100, 0b00010,
        ],
        ')' => [
            0b01000, 0b00100, 0b00010, 0b00010, 0b00010, 0b00100, 0b01000,
        ],
        ' ' => [0; 7],
        _ => [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b00000, 0b00100,
        ],
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
        assert_eq!(
            bitmap.rgba.len(),
            (PANEL_BITMAP_WIDTH * PANEL_BITMAP_HEIGHT * 4) as usize
        );
    }

    #[test]
    fn raster_contains_visible_text_pixels() {
        let bitmap = PanelBitmap::from_content(&PanelContent::from_state(&AppState::default()));
        assert!(bitmap.rgba.chunks_exact(4).any(|pixel| pixel == TITLE));
        assert!(bitmap.rgba.chunks_exact(4).any(|pixel| pixel == TEXT));
        assert!(bitmap.rgba.chunks_exact(4).any(|pixel| pixel == DIVIDER));
    }

    #[test]
    fn raster_contains_visible_interaction_controls() {
        let bitmap = PanelBitmap::from_content(&PanelContent::from_state(&AppState::default()));
        assert!(
            bitmap
                .rgba
                .chunks_exact(4)
                .any(|pixel| pixel == BUTTON_FILL)
        );
        assert!(
            bitmap
                .rgba
                .chunks_exact(4)
                .any(|pixel| pixel == BUTTON_BORDER)
        );
        assert!(
            bitmap
                .rgba
                .chunks_exact(4)
                .any(|pixel| pixel == BUTTON_TEXT)
        );
    }

    #[test]
    fn long_lines_clip_without_resizing_or_panicking() {
        let mut content = PanelContent::from_state(&AppState::default());
        content.lines[5] = "X".repeat(500);
        let bitmap = PanelBitmap::from_content(&content);
        assert_eq!(
            bitmap.rgba.len(),
            (PANEL_BITMAP_WIDTH * PANEL_BITMAP_HEIGHT * 4) as usize
        );
    }

    #[test]
    fn lowercase_text_uses_readable_uppercase_glyphs() {
        assert_eq!(glyph5x7('a'), glyph5x7('A'));
        assert_ne!(glyph5x7('A'), [0; 7]);
    }

    #[test]
    fn hovered_button_has_distinct_pixels() {
        let content = PanelContent::from_state(&AppState::default());
        let normal = PanelBitmap::from_content(&content);
        let hovered = PanelBitmap::from_content_with_hover(&content, Some(PanelAction::Wake));
        assert_ne!(normal, hovered);
        assert!(
            hovered
                .rgba
                .chunks_exact(4)
                .any(|pixel| pixel == BUTTON_HOVER_FILL)
        );
        assert!(
            hovered
                .rgba
                .chunks_exact(4)
                .any(|pixel| pixel == BUTTON_HOVER_BORDER)
        );
    }
}
