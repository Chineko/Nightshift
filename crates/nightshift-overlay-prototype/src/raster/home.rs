use super::*;
use crate::home_layout::{
    HomeBrightnessLayout, HomeCompositionLayout, PanelChromeLayout, PanelNavLayout,
};

impl PanelBitmap {
    pub(super) fn draw_home(&mut self, content: &PanelContent, hover: Option<PanelAction>) {
        let chrome = PanelChromeLayout::compute();
        let home = HomeCompositionLayout::compute();
        let brightness = HomeBrightnessLayout::compute();
        let nav = PanelNavLayout::compute();
        let header = pixel_rect(chrome.header);
        let brightness_panel = pixel_rect(home.brightness_panel);
        let brightness_track = pixel_rect(home.brightness_track);
        let status_cards = home.status_cards.map(pixel_rect);
        let automation = pixel_rect(home.automation_panel);
        let primary = pixel_rect(home.primary_action);
        let connection = pixel_rect(home.connection_badge);
        let down = pixel_rect(brightness.down);
        let up = pixel_rect(brightness.up);
        let nav_home = pixel_rect(nav.home);
        let nav_settings = pixel_rect(nav.settings);

        self.rounded_rect(header.0, header.1, header.2, header.3, 18, PANEL, BORDER);
        self.rounded_rect(header.0 + 20, header.1 + 16, 44, 44, 13, PURPLE_DARK, PURPLE);
        self.draw_crescent(header.0 + 42, header.1 + 38, 13, PURPLE_BRIGHT, PURPLE_DARK);
        self.draw_text(header.0 + 80, header.1 + 25, "NIGHTSHIFT", 4, TEXT);
        self.rounded_rect(
            nav_home.0,
            nav_home.1 + 1,
            nav_home.2 + 2,
            nav_home.3 - 2,
            14,
            PURPLE_DARK,
            BORDER,
        );
        self.draw_text_centered(nav_home.0, nav_home.1 + 16, nav_home.2, "HOME", 3, TEXT);
        self.draw_text_centered(
            nav_settings.0,
            nav_settings.1 + 16,
            nav_settings.2,
            "SETTINGS",
            3,
            MUTED,
        );

        self.draw_text(48, 124, "VR OVERLAY", 2, PURPLE_BRIGHT);
        self.draw_text(48, 151, "GOOD EVENING.", 5, TEXT);
        self.draw_text(48, 194, "EVERYTHING IS CALM AND CONNECTED.", 2, MUTED);
        self.rounded_rect(connection.0, connection.1, connection.2, connection.3, 14, PANEL, BORDER);
        self.draw_circle(
            connection.0 + 20,
            connection.1 + 22,
            5,
            if content.connected { GREEN } else { MUTED },
        );
        self.draw_text(
            connection.0 + 33,
            connection.1 + 14,
            if content.connected { "LIVE" } else { "OFFLINE" },
            2,
            if content.connected { GREEN } else { MUTED },
        );

        self.glass_panel(
            brightness_panel.0,
            brightness_panel.1,
            brightness_panel.2,
            brightness_panel.3,
            20,
            [18, 20, 31, 218],
        );
        self.rounded_rect(
            brightness_panel.0 + 22,
            brightness_panel.1 + 20,
            54,
            54,
            14,
            AMBER_DARK,
            AMBER_DARK,
        );
        self.draw_sun(brightness_panel.0 + 49, brightness_panel.1 + 47, 11, AMBER);
        self.draw_text(brightness_panel.0 + 92, brightness_panel.1 + 21, "BRIGHTNESS", 3, TEXT);
        self.draw_text(
            brightness_panel.0 + 92,
            brightness_panel.1 + 48,
            "OVERLAY DISPLAY LEVEL",
            2,
            MUTED,
        );
        let value = format!("{}%", content.brightness_percent);
        self.draw_text(brightness_panel.0 + 511, brightness_panel.1 + 30, &value, 6, TEXT);

        self.rounded_rect(
            brightness_track.0,
            brightness_track.1,
            brightness_track.2,
            brightness_track.3,
            3,
            [48, 52, 66, 255],
            [48, 52, 66, 255],
        );
        let fill_w = ((brightness_track.2 as f32 * content.brightness_percent as f32 / 100.0)
            .round() as u32)
            .clamp(brightness_track.3, brightness_track.2);
        self.rounded_rect(
            brightness_track.0,
            brightness_track.1,
            fill_w,
            brightness_track.3,
            3,
            PURPLE,
            PURPLE,
        );
        let knob_x = brightness_track.0 + fill_w.saturating_sub(1);
        self.draw_circle(knob_x, brightness_track.1 + 3, 10, PURPLE_BRIGHT);
        self.draw_circle(knob_x, brightness_track.1 + 3, 5, TEXT);

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
        self.draw_line(
            brightness_panel.0 + 24,
            brightness_panel.1 + 191,
            brightness_panel.0 + brightness_panel.2 - 24,
            brightness_panel.1 + 191,
            BORDER,
        );
        self.draw_text(
            brightness_panel.0 + 24,
            brightness_panel.1 + 213,
            "TARGET BRIGHTNESS",
            2,
            MUTED,
        );
        self.draw_text(
            brightness_panel.0 + 575,
            brightness_panel.1 + 213,
            if content.brightness_synced { "SYNCED" } else { "FADING" },
            2,
            if content.brightness_synced { GREEN } else { AMBER },
        );

        self.draw_text(48, 505, "AT A GLANCE", 3, TEXT);
        self.draw_text(659, 507, "LIVE", 2, GREEN);
        self.draw_circle(643, 515, 5, GREEN);

        let [heart_rate, osc, vrchat] = status_cards;
        self.status_card(
            heart_rate.0,
            heart_rate.1,
            heart_rate.2,
            heart_rate.3,
            PINK,
            "HR",
            "HEART RATE",
            &content.heart_rate_value,
            Some((&content.heart_rate_status, GREEN)),
        );
        self.status_card(
            osc.0,
            osc.1,
            osc.2,
            osc.3,
            BLUE,
            "OSC",
            "OSC AUTOMATION",
            &content.osc_status,
            None,
        );
        self.status_card(
            vrchat.0,
            vrchat.1,
            vrchat.2,
            vrchat.3,
            PURPLE_BRIGHT,
            "VR",
            "VRCHAT STATUS",
            &content.vrchat_status,
            None,
        );

        self.glass_panel(
            automation.0,
            automation.1,
            automation.2,
            automation.3,
            18,
            [28, 22, 47, 224],
        );
        self.rounded_rect(
            automation.0 + 22,
            automation.1 + 21,
            54,
            54,
            14,
            PURPLE_DARK,
            PURPLE_DARK,
        );
        self.draw_lightning(automation.0 + 49, automation.1 + 48, PURPLE_BRIGHT);
        self.draw_text(automation.0 + 92, automation.1 + 17, "AUTOMATION READY", 3, TEXT);
        self.draw_text(
            automation.0 + 92,
            automation.1 + 51,
            &format!("SLEEP TARGET {}%", content.target_brightness_percent),
            2,
            MUTED,
        );
        self.draw_text(
            automation.0 + 92,
            automation.1 + 77,
            "BRIGHTNESS + VRCHAT UPDATE",
            2,
            MUTED,
        );

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
        self.draw_text_centered(primary.0 + 238, primary.1 + 25, primary.2 - 278, label, 3, TEXT);

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
