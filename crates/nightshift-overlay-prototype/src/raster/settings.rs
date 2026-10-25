use super::*;
use crate::home_layout::{
    PanelNavLayout, SettingsToggleLayout, SettingsValueLayout, SocialRemoveLayout, SocialTabsLayout,
};

impl PanelBitmap {
    pub(super) fn draw_settings(&mut self, content: &PanelContent, hover: Option<PanelAction>) {
        let nav_layout = PanelNavLayout::compute();
        let social_tabs = SocialTabsLayout::compute();
        let toggles = SettingsToggleLayout::compute();
        let values = SettingsValueLayout::compute();
        let social_remove = SocialRemoveLayout::compute();
        let nav_home = pixel_rect(nav_layout.home);
        let nav_settings = pixel_rect(nav_layout.settings);
        let whitelist = pixel_rect(social_tabs.whitelist);
        let blacklist = pixel_rect(social_tabs.blacklist);
        let heart_rate_toggle = pixel_rect(toggles.heart_rate);
        let osc_toggle = pixel_rect(toggles.osc);
        let invite_rules = pixel_rect(values.invite_rules);
        let vrchat_status = pixel_rect(values.vrchat_status);

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
            PANEL_ALT,
            BORDER,
        );
        self.draw_text(nav_home.0 + 19, nav_home.1 + 16, "HOME", 3, MUTED);
        self.rounded_rect(
            nav_settings.0,
            nav_settings.1 + 1,
            nav_settings.2,
            nav_settings.3 - 2,
            14,
            PURPLE_DARK,
            PURPLE,
        );
        self.draw_text(nav_settings.0 + 14, nav_settings.1 + 16, "SETTINGS", 3, TEXT);

        self.draw_text(40, 124, "PREFERENCES", 2, PURPLE_BRIGHT);
        self.draw_text(40, 151, "SETTINGS", 5, TEXT);
        self.draw_text(
            40,
            194,
            "FINE-TUNE NIGHTSHIFT WITHOUT LEAVING VR.",
            2,
            MUTED,
        );

        self.quick_toggle_card(
            40,
            230,
            "HR",
            PINK,
            "HR MONITORING",
            "LIVE HEART RATE FROM YOUR DEVICE",
            heart_rate_toggle,
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
            osc_toggle,
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
            invite_rules,
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
            vrchat_status,
            content.sleep_vrchat_status_label(),
            hover == Some(PanelAction::CycleVrchatStatus),
        );

        self.rounded_rect(43, 717, 420, 128, 18, SHADOW, SHADOW);
        self.rounded_rect(40, 712, 420, 128, 18, [20, 18, 35, 255], [57, 44, 91, 255]);
        self.rounded_rect(60, 736, 52, 52, 14, PURPLE_DARK, PURPLE_DARK);
        self.draw_lightning(86, 762, PURPLE_BRIGHT);
        self.draw_text(128, 730, "AUTOMATION READY", 3, TEXT);
        self.draw_text(128, 765, "ADVANCED TUNING LIVES IN THE", 2, MUTED);
        self.draw_text(128, 790, "DESKTOP CONTROL CENTER.", 2, MUTED);

        self.rounded_rect(483, 235, 248, 610, 18, SHADOW, SHADOW);
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
            whitelist.0,
            whitelist.1,
            whitelist.2,
            whitelist.3,
            12,
            if whitelist_selected || whitelist_hover {
                PURPLE_DARK
            } else {
                PANEL_ALT
            },
            if whitelist_selected { PURPLE } else { BORDER },
        );
        self.draw_text(
            whitelist.0 + 12,
            whitelist.1 + 16,
            "WHITELIST",
            2,
            if whitelist_selected { TEXT } else { MUTED },
        );
        self.rounded_rect(
            blacklist.0,
            blacklist.1,
            blacklist.2,
            blacklist.3,
            12,
            if blacklist_selected || blacklist_hover {
                PURPLE_DARK
            } else {
                PANEL_ALT
            },
            if blacklist_selected { PURPLE } else { BORDER },
        );
        self.draw_text(
            blacklist.0 + 11,
            blacklist.1 + 16,
            "BLACKLIST",
            2,
            if blacklist_selected { TEXT } else { MUTED },
        );

        let entries = content.social_entries();
        if entries.is_empty() {
            self.draw_text(514, 386, "NO PEOPLE HERE", 2, MUTED);
            self.draw_text(514, 416, "ADD THEM ON DESKTOP", 2, MUTED);
        } else {
            for (index, entry) in entries.iter().take(5).enumerate() {
                let remove = pixel_rect(social_remove.buttons[index]);
                let y = remove.1 - 8;
                let action = PanelAction::RemoveSocial(index as u8);
                let row_hover = hover == Some(action);
                self.rounded_rect(
                    500,
                    y,
                    208,
                    60,
                    14,
                    if row_hover {
                        [24, 26, 39, 255]
                    } else {
                        PANEL_ALT
                    },
                    if row_hover { PURPLE_DARK } else { BORDER },
                );
                let avatar = social_initial(entry);
                self.draw_circle(526, y + 30, 19, BORDER);
                self.draw_circle(526, y + 30, 16, PURPLE_DARK);
                self.draw_text(520, y + 22, &avatar.to_string(), 2, PURPLE_BRIGHT);
                let label = compact_social_label(entry, 12);
                self.draw_text(552, y + 21, &label, 2, TEXT);
                self.rounded_rect(
                    remove.0,
                    remove.1,
                    remove.2,
                    remove.3,
                    10,
                    if row_hover { PURPLE_DARK } else { PANEL },
                    BORDER,
                );
                self.draw_text(remove.0 + 10, remove.1 + 12, "X", 2, MUTED);
            }
            if entries.len() > 5 {
                self.draw_text(
                    512,
                    735,
                    &format!("+{} MORE ON DESKTOP", entries.len() - 5),
                    2,
                    MUTED,
                );
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
        toggle: (u32, u32, u32, u32),
        enabled: bool,
        hovered: bool,
    ) {
        self.rounded_rect(x + 3, y + 5, 420, 100, 18, SHADOW, SHADOW);
        self.rounded_rect(
            x,
            y,
            420,
            100,
            18,
            if hovered {
                [19, 22, 33, 255]
            } else {
                PANEL_ALT
            },
            if hovered { PURPLE_DARK } else { BORDER },
        );
        self.rounded_rect(x + 4, y + 16, 4, 68, 2, accent, accent);
        self.rounded_rect(
            x + 18,
            y + 24,
            52,
            52,
            14,
            [28, 25, 40, 255],
            [28, 25, 40, 255],
        );
        self.draw_text(x + 29, y + 42, icon, 2, accent);
        self.draw_text(x + 84, y + 23, title, 3, TEXT);
        self.draw_text(x + 84, y + 56, description, 1, MUTED);
        self.rounded_rect(
            toggle.0,
            toggle.1,
            toggle.2,
            toggle.3,
            18,
            if enabled { PURPLE } else { [48, 52, 66, 255] },
            if hovered { PURPLE_BRIGHT } else { BORDER },
        );
        self.draw_circle(
            if enabled {
                toggle.0 + 44
            } else {
                toggle.0 + 18
            },
            toggle.1 + 18,
            12,
            TEXT,
        );
    }

    fn quick_value_card(
        &mut self,
        x: u32,
        y: u32,
        icon: &str,
        accent: [u8; 4],
        title: &str,
        description: &str,
        slot: (u32, u32, u32, u32),
        value: &str,
        hovered: bool,
    ) {
        self.rounded_rect(x + 3, y + 5, 420, 100, 18, SHADOW, SHADOW);
        self.rounded_rect(
            x,
            y,
            420,
            100,
            18,
            if hovered {
                [19, 22, 33, 255]
            } else {
                PANEL_ALT
            },
            if hovered { PURPLE_DARK } else { BORDER },
        );
        self.rounded_rect(x + 4, y + 16, 4, 68, 2, accent, accent);
        self.rounded_rect(
            x + 18,
            y + 24,
            52,
            52,
            14,
            [28, 25, 40, 255],
            [28, 25, 40, 255],
        );
        self.draw_text(x + 29, y + 42, icon, 2, accent);
        self.draw_text(x + 84, y + 23, title, 3, TEXT);
        self.draw_text(x + 84, y + 56, description, 1, MUTED);
        let width = (value.chars().count() as u32 * 12 + 28).clamp(82, 136);
        let pill_right = slot.0 + slot.2 - 8;
        let pill_x = pill_right.saturating_sub(width);
        let pill_y = slot.1 + 21;
        self.rounded_rect(
            pill_x,
            pill_y,
            width,
            42,
            12,
            PANEL,
            if hovered { PURPLE } else { BORDER },
        );
        self.draw_text(pill_x + 12, pill_y + 14, value, 2, TEXT);
    }

    pub(super) fn status_card(
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
        self.rounded_rect(x + 3, y + 5, width, height, 18, SHADOW, SHADOW);
        self.rounded_rect(x, y, width, height, 18, PANEL_ALT, BORDER);
        self.rounded_rect(x + 18, y + 4, width - 36, 3, 1, accent, accent);
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
}

fn pixel_rect(rect: crate::layout::LayoutRect) -> (u32, u32, u32, u32) {
    (
        rect.x.round() as u32,
        rect.y.round() as u32,
        rect.width.round() as u32,
        rect.height.round() as u32,
    )
}

fn social_initial(value: &str) -> char {
    value
        .chars()
        .find(|ch| ch.is_ascii_alphanumeric())
        .unwrap_or('?')
        .to_ascii_uppercase()
}

fn compact_social_label(value: &str, max_chars: usize) -> String {
    let mut label = value.trim().to_string();
    if let Some(stripped) = label.strip_prefix("usr_") {
        label = stripped.to_string();
    }
    let count = label.chars().count();
    if count > max_chars {
        label = label
            .chars()
            .take(max_chars.saturating_sub(2))
            .collect::<String>();
        label.push_str("..");
    }
    label.to_ascii_uppercase()
}
