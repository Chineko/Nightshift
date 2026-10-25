use super::*;
use crate::home_layout::{HomeBrightnessLayout, HomePrimaryLayout, PanelNavLayout};

impl PanelBitmap {
    pub(super) fn draw_home(&mut self, content: &PanelContent, hover: Option<PanelAction>) {
        let brightness_layout = HomeBrightnessLayout::compute();
        let primary_layout = HomePrimaryLayout::compute();
        let nav_layout = PanelNavLayout::compute();
        let down = pixel_rect(brightness_layout.down);
        let up = pixel_rect(brightness_layout.up);
        let primary = pixel_rect(primary_layout.action);
        let nav_home = pixel_rect(nav_layout.home);

        self.rounded_rect(32, 24, 704, 72, 18, PANEL, BORDER);
        self.rounded_rect(52, 40, 44, 44, 13, PURPLE_DARK, PURPLE);
        self.draw_crescent(74, 62, 13, PURPLE_BRIGHT, PURPLE_DARK);
        self.draw_text(112, 49, "NIGHTSHIFT", 4, TEXT);
        self.rounded_rect(
            nav_home.0,
            nav_home.1 + 1,
            nav_home.2 + 2,
            nav_home.3 - 2,
            14,
            PURPLE_DARK,
            BORDER,
        );
        self.draw_text(nav_home.0 + 19, nav_home.1 + 16, "HOME", 3, TEXT);
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

        self.glass_panel(40, 230, 688, 250, 20, [18, 20, 31, 218]);
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
            self.rounded_rect(down.0, down.1, down.2, down.3, 16, PURPLE_DARK, PURPLE_BRIGHT);
        }
        if up_hover {
            self.rounded_rect(up.0, up.1, up.2, up.3, 16, PURPLE_DARK, PURPLE_BRIGHT);
        }
        self.draw_crescent(down.0 + 22, down.1 + 33, 10, MUTED, PANEL);
        self.draw_sun(up.0 + 34, up.1 + 33, 8, MUTED);
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

        self.glass_panel(40, 700, 688, 112, 18, [28, 22, 47, 224]);
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
            primary.0,
            primary.1,
            primary.2,
            primary.3,
            20,
            if hovered { PURPLE_BRIGHT } else { PURPLE },
            if hovered { TEXT } else { PURPLE_BRIGHT },
        );
        self.draw_crescent(
            primary.0 + 214,
            primary.1 + 37,
            13,
            TEXT,
            if hovered { PURPLE_BRIGHT } else { PURPLE },
        );
        let label = content.primary_action_label();
        let label_width = label.chars().count() as u32 * 18;
        let label_x = 397_u32.saturating_sub(label_width / 2).max(290);
        self.draw_text(label_x, primary.1 + 25, label, 3, TEXT);

        self.draw_text(271, 935, "NIGHTSHIFT OVERLAY  V1.4.2", 1, MUTED);
    }
}

fn pixel_rect(rect: crate::layout::LayoutRect) -> (u32, u32, u32, u32) {
    (
        rect.x.round() as u32,
        rect.y.round() as u32,
        rect.width.round() as u32,
        rect.height.round() as u32,
    )
}
