use super::{
    content::{PanelContent, SocialTab},
    interaction::{PanelAction, PanelPage},
};

pub const PANEL_BITMAP_WIDTH: u32 = 768;
pub const PANEL_BITMAP_HEIGHT: u32 = 960;

const BACKGROUND: [u8; 4] = [7, 9, 14, 255];
const PANEL: [u8; 4] = [15, 18, 27, 255];
const PANEL_ALT: [u8; 4] = [18, 21, 31, 255];
const BORDER: [u8; 4] = [42, 47, 62, 255];
const TEXT: [u8; 4] = [242, 243, 248, 255];
const MUTED: [u8; 4] = [139, 146, 176, 255];
const PURPLE: [u8; 4] = [139, 92, 246, 255];
const PURPLE_BRIGHT: [u8; 4] = [167, 139, 250, 255];
const PURPLE_DARK: [u8; 4] = [48, 37, 82, 255];
const AMBER: [u8; 4] = [245, 176, 65, 255];
const AMBER_DARK: [u8; 4] = [48, 37, 25, 255];
const GREEN: [u8; 4] = [69, 231, 171, 255];
const PINK: [u8; 4] = [244, 96, 139, 255];
const BLUE: [u8; 4] = [85, 162, 255, 255];

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
        match content.page {
            PanelPage::Home => bitmap.draw_home(content, hover),
            PanelPage::Settings => bitmap.draw_settings(content, hover),
        }
        bitmap
    }

    fn draw_home(&mut self, content: &PanelContent, hover: Option<PanelAction>) {
        self.rounded_rect(32, 24, 704, 72, 18, PANEL, BORDER);
        self.rounded_rect(52, 40, 44, 44, 13, PURPLE_DARK, PURPLE);
        self.draw_crescent(74, 62, 13, PURPLE_BRIGHT, PURPLE_DARK);
        self.draw_text(112, 49, "NIGHTSHIFT", 4, TEXT);
        self.rounded_rect(500, 39, 96, 46, 14, PURPLE_DARK, BORDER);
        self.draw_text(519, 54, "HOME", 3, TEXT);
        self.draw_text(620, 54, "SETTINGS", 3, MUTED);

        self.draw_text(48, 124, "VR OVERLAY", 2, PURPLE_BRIGHT);
        self.draw_text(48, 151, "GOOD EVENING.", 5, TEXT);
        self.draw_text(48, 194, "EVERYTHING IS CALM AND CONNECTED.", 2, MUTED);
        self.rounded_rect(621, 135, 99, 44, 14, PANEL, BORDER);
        self.draw_circle(641, 157, 5, if content.connected { GREEN } else { MUTED });
        self.draw_text(
            654,
            149,
            if content.connected { "LIVE" } else { "OFFLINE" },
            2,
            if content.connected { GREEN } else { MUTED },
        );

        self.rounded_rect(40, 230, 688, 250, 20, PANEL, BORDER);
        self.rounded_rect(62, 250, 54, 54, 14, AMBER_DARK, AMBER_DARK);
        self.draw_sun(89, 277, 11, AMBER);
        self.draw_text(132, 251, "BRIGHTNESS", 3, TEXT);
        self.draw_text(132, 278, "OVERLAY DISPLAY LEVEL", 2, MUTED);
        let value = format!("{}%", content.brightness_percent);
        self.draw_text(551, 260, &value, 6, TEXT);

        let track_x = 92;
        let track_y = 370;
        let track_w = 584;
        self.rounded_rect(
            track_x,
            track_y,
            track_w,
            7,
            3,
            [48, 52, 66, 255],
            [48, 52, 66, 255],
        );
        let fill_w = ((track_w as f32 * content.brightness_percent as f32 / 100.0).round() as u32)
            .clamp(7, track_w);
        self.rounded_rect(track_x, track_y, fill_w, 7, 3, PURPLE, PURPLE);
        let knob_x = track_x + fill_w.saturating_sub(1);
        self.draw_circle(knob_x, track_y + 3, 10, PURPLE_BRIGHT);
        self.draw_circle(knob_x, track_y + 3, 5, TEXT);

        let down_hover = hover == Some(PanelAction::BrightnessDown);
        let up_hover = hover == Some(PanelAction::BrightnessUp);
        if down_hover {
            self.rounded_rect(60, 340, 56, 66, 16, PURPLE_DARK, PURPLE_BRIGHT);
        }
        if up_hover {
            self.rounded_rect(652, 340, 56, 66, 16, PURPLE_DARK, PURPLE_BRIGHT);
        }
        self.draw_crescent(82, 373, 10, MUTED, PANEL);
        self.draw_sun(686, 373, 8, MUTED);
        self.draw_line(64, 421, 704, 421, BORDER);
        self.draw_text(64, 443, "TARGET BRIGHTNESS", 2, MUTED);
        self.draw_text(
            615,
            443,
            if content.brightness_synced {
                "SYNCED"
            } else {
                "FADING"
            },
            2,
            if content.brightness_synced {
                GREEN
            } else {
                AMBER
            },
        );

        self.draw_text(48, 505, "AT A GLANCE", 3, TEXT);
        self.draw_text(659, 507, "LIVE", 2, GREEN);
        self.draw_circle(643, 515, 5, GREEN);

        self.status_card(
            40,
            540,
            216,
            136,
            PINK,
            "HR",
            "HEART RATE",
            &content.heart_rate_value,
            Some((&content.heart_rate_status, GREEN)),
        );
        self.status_card(
            276,
            540,
            216,
            136,
            BLUE,
            "OSC",
            "OSC AUTOMATION",
            &content.osc_status,
            None,
        );
        self.status_card(
            512,
            540,
            216,
            136,
            PURPLE_BRIGHT,
            "VR",
            "VRCHAT STATUS",
            &content.vrchat_status,
            None,
        );

        self.rounded_rect(40, 700, 688, 112, 18, [20, 18, 35, 255], [57, 44, 91, 255]);
        self.rounded_rect(62, 721, 54, 54, 14, PURPLE_DARK, PURPLE_DARK);
        self.draw_lightning(89, 748, PURPLE_BRIGHT);
        self.draw_text(132, 717, "AUTOMATION READY", 3, TEXT);
        self.draw_text(
            132,
            751,
            &format!("SLEEP TARGET {}%", content.target_brightness_percent),
            2,
            MUTED,
        );
        self.draw_text(132, 777, "BRIGHTNESS + VRCHAT UPDATE", 2, MUTED);

        let primary_action = if content.sleep_action_is_wake() {
            PanelAction::Wake
        } else {
            PanelAction::EnterSleep
        };
        let hovered = hover == Some(primary_action);
        self.rounded_rect(
            40,
            840,
            688,
            74,
            20,
            if hovered { PURPLE_BRIGHT } else { PURPLE },
            if hovered { TEXT } else { PURPLE_BRIGHT },
        );
        self.draw_crescent(
            254,
            877,
            13,
            TEXT,
            if hovered { PURPLE_BRIGHT } else { PURPLE },
        );
        let label = content.primary_action_label();
        let label_width = label.chars().count() as u32 * 18;
        let label_x = 397_u32.saturating_sub(label_width / 2).max(290);
        self.draw_text(label_x, 865, label, 3, TEXT);

        self.draw_text(271, 935, "NIGHTSHIFT OVERLAY  V1.4.2", 1, MUTED);
    }


    fn draw_settings(&mut self, content: &PanelContent, hover: Option<PanelAction>) {
        self.rounded_rect(32, 24, 704, 72, 18, PANEL, BORDER);
        self.rounded_rect(52, 40, 44, 44, 13, PURPLE_DARK, PURPLE);
        self.draw_crescent(74, 62, 13, PURPLE_BRIGHT, PURPLE_DARK);
        self.draw_text(112, 49, "NIGHTSHIFT", 4, TEXT);
        self.rounded_rect(500, 39, 96, 46, 14, PANEL_ALT, BORDER);
        self.draw_text(519, 54, "HOME", 3, MUTED);
        self.rounded_rect(602, 39, 118, 46, 14, PURPLE_DARK, PURPLE);
        self.draw_text(616, 54, "SETTINGS", 3, TEXT);

        self.draw_text(40, 124, "PREFERENCES", 2, PURPLE_BRIGHT);
        self.draw_text(40, 151, "SETTINGS", 5, TEXT);
        self.draw_text(40, 194, "QUICK CONTROLS FOR WHILE YOU ARE IN VR.", 2, MUTED);

        self.quick_toggle_card(
            40,
            230,
            "HR",
            PINK,
            "HR MONITORING",
            "LIVE HEART RATE FROM YOUR DEVICE",
            content.heart_rate_enabled,
            hover == Some(PanelAction::ToggleHeartRate),
        );
        self.quick_toggle_card(
            40,
            346,
            "OSC",
            BLUE,
            "OSC AUTOMATION",
            "SLEEP MODE TRIGGERS OSC PARAMETERS",
            content.osc_enabled,
            hover == Some(PanelAction::ToggleOsc),
        );
        self.quick_value_card(
            40,
            462,
            "IN",
            PURPLE_BRIGHT,
            "INVITE RULES",
            "WHO CAN SEND REQUESTS DURING SLEEP",
            content.invite_profile_label(),
            hover == Some(PanelAction::CycleInviteProfile),
        );
        self.quick_value_card(
            40,
            578,
            "VR",
            [94, 220, 210, 255],
            "VRCHAT STATUS",
            "STATUS USED BY SLEEP AUTOMATION",
            content.sleep_vrchat_status_label(),
            hover == Some(PanelAction::CycleVrchatStatus),
        );

        self.rounded_rect(40, 712, 420, 128, 18, [20, 18, 35, 255], [57, 44, 91, 255]);
        self.rounded_rect(60, 736, 52, 52, 14, PURPLE_DARK, PURPLE_DARK);
        self.draw_lightning(86, 762, PURPLE_BRIGHT);
        self.draw_text(128, 730, "AUTOMATION READY", 3, TEXT);
        self.draw_text(128, 765, "ADVANCED TUNING LIVES IN THE", 2, MUTED);
        self.draw_text(128, 790, "DESKTOP CONTROL CENTER.", 2, MUTED);

        self.rounded_rect(480, 230, 248, 610, 18, PANEL, BORDER);
        self.rounded_rect(500, 250, 48, 48, 13, PURPLE_DARK, PURPLE_DARK);
        self.draw_text(511, 267, "PPL", 2, PURPLE_BRIGHT);
        self.draw_text(560, 252, "SOCIAL ACCESS", 3, TEXT);
        self.draw_text(560, 280, "QUICK REMOVE", 2, MUTED);

        let whitelist_selected = content.social_tab == SocialTab::Whitelist;
        let blacklist_selected = content.social_tab == SocialTab::Blacklist;
        let whitelist_hover = hover == Some(PanelAction::SelectWhitelist);
        let blacklist_hover = hover == Some(PanelAction::SelectBlacklist);
        self.rounded_rect(
            500,
            302,
            102,
            46,
            12,
            if whitelist_selected || whitelist_hover { PURPLE_DARK } else { PANEL_ALT },
            if whitelist_selected { PURPLE } else { BORDER },
        );
        self.draw_text(512, 318, "WHITELIST", 2, if whitelist_selected { TEXT } else { MUTED });
        self.rounded_rect(
            606,
            302,
            102,
            46,
            12,
            if blacklist_selected || blacklist_hover { PURPLE_DARK } else { PANEL_ALT },
            if blacklist_selected { PURPLE } else { BORDER },
        );
        self.draw_text(617, 318, "BLACKLIST", 2, if blacklist_selected { TEXT } else { MUTED });

        let entries = content.social_entries();
        if entries.is_empty() {
            self.draw_text(514, 386, "NO PEOPLE HERE", 2, MUTED);
            self.draw_text(514, 416, "ADD THEM ON DESKTOP", 2, MUTED);
        } else {
            for (index, entry) in entries.iter().take(5).enumerate() {
                let y = 360 + index as u32 * 72;
                let action = PanelAction::RemoveSocial(index as u8);
                let row_hover = hover == Some(action);
                self.rounded_rect(
                    500,
                    y,
                    208,
                    60,
                    14,
                    if row_hover { [24, 26, 39, 255] } else { PANEL_ALT },
                    if row_hover { PURPLE_DARK } else { BORDER },
                );
                let avatar = social_initial(entry);
                self.draw_circle(526, y + 30, 17, PURPLE_DARK);
                self.draw_text(520, y + 22, &avatar.to_string(), 2, PURPLE_BRIGHT);
                let label = compact_social_label(entry, 12);
                self.draw_text(552, y + 21, &label, 2, TEXT);
                self.rounded_rect(676, y + 8, 32, 44, 10, if row_hover { PURPLE_DARK } else { PANEL }, BORDER);
                self.draw_text(686, y + 20, "X", 2, MUTED);
            }
            if entries.len() > 5 {
                self.draw_text(512, 735, &format!("+{} MORE ON DESKTOP", entries.len() - 5), 2, MUTED);
            }
        }
        self.draw_text(512, 792, "ADD + EDIT PEOPLE", 2, MUTED);
        self.draw_text(512, 817, "IN DESKTOP SETTINGS", 2, MUTED);

        self.draw_text(271, 935, "NIGHTSHIFT OVERLAY  V1.4.2", 1, MUTED);
    }

    fn quick_toggle_card(
        &mut self,
        x: u32,
        y: u32,
        icon: &str,
        accent: [u8; 4],
        title: &str,
        description: &str,
        enabled: bool,
        hovered: bool,
    ) {
        self.rounded_rect(x, y, 420, 100, 18, if hovered { [19, 22, 33, 255] } else { PANEL_ALT }, if hovered { PURPLE_DARK } else { BORDER });
        self.rounded_rect(x + 18, y + 24, 52, 52, 14, [28, 25, 40, 255], [28, 25, 40, 255]);
        self.draw_text(x + 29, y + 42, icon, 2, accent);
        self.draw_text(x + 84, y + 23, title, 3, TEXT);
        self.draw_text(x + 84, y + 56, description, 1, MUTED);
        let toggle_x = x + 330;
        self.rounded_rect(toggle_x, y + 31, 62, 36, 18, if enabled { PURPLE } else { [48, 52, 66, 255] }, if hovered { PURPLE_BRIGHT } else { BORDER });
        self.draw_circle(if enabled { toggle_x + 44 } else { toggle_x + 18 }, y + 49, 12, TEXT);
    }

    fn quick_value_card(
        &mut self,
        x: u32,
        y: u32,
        icon: &str,
        accent: [u8; 4],
        title: &str,
        description: &str,
        value: &str,
        hovered: bool,
    ) {
        self.rounded_rect(x, y, 420, 100, 18, if hovered { [19, 22, 33, 255] } else { PANEL_ALT }, if hovered { PURPLE_DARK } else { BORDER });
        self.rounded_rect(x + 18, y + 24, 52, 52, 14, [28, 25, 40, 255], [28, 25, 40, 255]);
        self.draw_text(x + 29, y + 42, icon, 2, accent);
        self.draw_text(x + 84, y + 23, title, 3, TEXT);
        self.draw_text(x + 84, y + 56, description, 1, MUTED);
        let width = (value.chars().count() as u32 * 12 + 28).clamp(82, 136);
        self.rounded_rect(x + 398 - width, y + 29, width, 42, 12, PANEL, if hovered { PURPLE } else { BORDER });
        self.draw_text(x + 410 - width, y + 43, value, 2, TEXT);
    }

    fn status_card(
        &mut self,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        accent: [u8; 4],
        icon: &str,
        label: &str,
        value: &str,
        footer: Option<(&str, [u8; 4])>,
    ) {
        self.rounded_rect(x, y, width, height, 18, PANEL_ALT, BORDER);
        self.rounded_rect(
            x + 18,
            y + 22,
            52,
            52,
            14,
            [28, 25, 40, 255],
            [28, 25, 40, 255],
        );
        self.draw_text(x + 27, y + 40, icon, 2, accent);
        self.draw_text(x + 82, y + 24, label, 2, MUTED);
        self.draw_text(x + 82, y + 55, value, 3, TEXT);
        if let Some((footer, color)) = footer {
            self.draw_text(x + 82, y + 91, footer, 2, color);
        }
    }

    fn rounded_rect(
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

    fn rounded_rect_fill(
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

    fn draw_line(&mut self, x0: u32, y0: u32, x1: u32, y1: u32, color: [u8; 4]) {
        if y0 == y1 {
            self.fill_rect(x0.min(x1), y0, x0.max(x1) - x0.min(x1) + 1, 1, color);
        } else if x0 == x1 {
            self.fill_rect(x0, y0.min(y1), 1, y0.max(y1) - y0.min(y1) + 1, color);
        }
    }

    fn draw_circle(&mut self, cx: u32, cy: u32, radius: u32, color: [u8; 4]) {
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

    fn draw_crescent(&mut self, cx: u32, cy: u32, radius: u32, color: [u8; 4], cutout: [u8; 4]) {
        self.draw_circle(cx, cy, radius, color);
        self.draw_circle(
            cx + radius / 2,
            cy.saturating_sub(radius / 4),
            radius.saturating_sub(2),
            cutout,
        );
    }

    fn draw_sun(&mut self, cx: u32, cy: u32, radius: u32, color: [u8; 4]) {
        self.draw_circle(cx, cy, radius / 2, color);
        let r = radius + 6;
        self.fill_rect(cx.saturating_sub(1), cy.saturating_sub(r), 3, 6, color);
        self.fill_rect(cx.saturating_sub(1), cy + r.saturating_sub(5), 3, 6, color);
        self.fill_rect(cx.saturating_sub(r), cy.saturating_sub(1), 6, 3, color);
        self.fill_rect(cx + r.saturating_sub(5), cy.saturating_sub(1), 6, 3, color);
    }

    fn draw_lightning(&mut self, cx: u32, cy: u32, color: [u8; 4]) {
        self.fill_rect(cx.saturating_sub(3), cy.saturating_sub(18), 7, 16, color);
        self.fill_rect(cx.saturating_sub(10), cy.saturating_sub(4), 14, 7, color);
        self.fill_rect(cx.saturating_sub(3), cy, 7, 18, color);
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

    fn put_pixel(&mut self, x: u32, y: u32, color: [u8; 4]) {
        if x >= self.width || y >= self.height {
            return;
        }
        let offset = ((y * self.width + x) * 4) as usize;
        self.rgba[offset..offset + 4].copy_from_slice(&color);
    }
}

fn social_initial(value: &str) -> char {
    value.chars().find(|ch| ch.is_ascii_alphanumeric()).unwrap_or('?').to_ascii_uppercase()
}

fn compact_social_label(value: &str, max_chars: usize) -> String {
    let mut label = value.trim().to_string();
    if let Some(stripped) = label.strip_prefix("usr_") {
        label = stripped.to_string();
    }
    let count = label.chars().count();
    if count > max_chars {
        label = label.chars().take(max_chars.saturating_sub(2)).collect::<String>();
        label.push_str("..");
    }
    label.to_ascii_uppercase()
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
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
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
        '+' => [
            0b00000, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0b00000,
        ],
        '.' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00110, 0b00110,
        ],
        ' ' => [0; 7],
        _ => [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b00000, 0b00100,
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nightshift_protocol::{AppState, SleepPhase};
    use crate::content::SocialTab;
    use crate::interaction::PanelPage;

    #[test]
    fn raster_has_vertical_rgba_dimensions() {
        let bitmap = PanelBitmap::from_content(&PanelContent::from_state(&AppState::default()));
        assert_eq!(bitmap.width, 768);
        assert_eq!(bitmap.height, 960);
        assert_eq!(bitmap.rgba.len(), 768 * 960 * 4);
    }

    #[test]
    fn settings_page_renders_distinct_dashboard() {
        let home = PanelBitmap::from_content(&PanelContent::from_state(&AppState::default()));
        let settings_content = PanelContent::from_state(&AppState::default())
            .with_view(PanelPage::Settings, SocialTab::Whitelist);
        let settings = PanelBitmap::from_content(&settings_content);
        assert_ne!(home.rgba, settings.rgba);
    }

    #[test]
    fn sleeping_state_changes_primary_button_pixels() {
        let awake = PanelBitmap::from_content(&PanelContent::from_state(&AppState::default()));
        let mut state = AppState::default();
        state.sleep.phase = SleepPhase::Sleeping;
        let sleeping = PanelBitmap::from_content(&PanelContent::from_state(&state));
        assert_ne!(awake.rgba, sleeping.rgba);
    }
}
