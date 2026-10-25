use crate::layout::{LayoutEngine, LayoutRect};

const BRIGHTNESS_X: f32 = 60.0;
const BRIGHTNESS_Y: f32 = 340.0;
const BUTTON_WIDTH: f32 = 56.0;
const BUTTON_HEIGHT: f32 = 66.0;
const BUTTON_GAP: f32 = 536.0;
const PRIMARY_X: f32 = 40.0;
const PRIMARY_Y: f32 = 840.0;
const PRIMARY_WIDTH: f32 = 688.0;
const PRIMARY_HEIGHT: f32 = 74.0;
const NAV_X: f32 = 500.0;
const NAV_Y: f32 = 38.0;
const NAV_HOME_WIDTH: f32 = 94.0;
const NAV_SETTINGS_WIDTH: f32 = 118.0;
const NAV_HEIGHT: f32 = 48.0;
const NAV_GAP: f32 = 8.0;
const SOCIAL_TABS_X: f32 = 500.0;
const SOCIAL_TABS_Y: f32 = 302.0;
const SOCIAL_TAB_WIDTH: f32 = 102.0;
const SOCIAL_TAB_HEIGHT: f32 = 46.0;
const SOCIAL_TAB_GAP: f32 = 4.0;
const SETTINGS_TOGGLE_X: f32 = 370.0;
const SETTINGS_TOGGLE_Y: f32 = 261.0;
const SETTINGS_TOGGLE_WIDTH: f32 = 62.0;
const SETTINGS_TOGGLE_HEIGHT: f32 = 36.0;
const SETTINGS_TOGGLE_GAP: f32 = 80.0;
const SETTINGS_TOGGLE_HIT_PAD_X: f32 = 14.0;
const SETTINGS_TOGGLE_HIT_PAD_Y: f32 = 23.0;
const SETTINGS_VALUE_X: f32 = 300.0;
const SETTINGS_VALUE_Y: f32 = 470.0;
const SETTINGS_VALUE_WIDTH: f32 = 146.0;
const SETTINGS_VALUE_HEIGHT: f32 = 82.0;
const SETTINGS_VALUE_GAP: f32 = 34.0;
const SOCIAL_REMOVE_X: f32 = 676.0;
const SOCIAL_REMOVE_Y: f32 = 368.0;
const SOCIAL_REMOVE_WIDTH: f32 = 32.0;
const SOCIAL_REMOVE_HEIGHT: f32 = 44.0;
const SOCIAL_REMOVE_GAP: f32 = 28.0;
const SOCIAL_REMOVE_HIT_TOP: f32 = 2.0;
const SOCIAL_REMOVE_HIT_RIGHT: f32 = 8.0;
const SOCIAL_REMOVE_HIT_BOTTOM: f32 = 8.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HomeBrightnessLayout {
    pub down: LayoutRect,
    pub up: LayoutRect,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HomePrimaryLayout {
    pub action: LayoutRect,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PanelNavLayout {
    pub home: LayoutRect,
    pub settings: LayoutRect,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SocialTabsLayout {
    pub whitelist: LayoutRect,
    pub blacklist: LayoutRect,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SettingsToggleLayout {
    pub heart_rate: LayoutRect,
    pub osc: LayoutRect,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SettingsValueLayout {
    pub invite_rules: LayoutRect,
    pub vrchat_status: LayoutRect,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SocialRemoveLayout {
    pub buttons: [LayoutRect; 5],
}

impl HomeBrightnessLayout {
    pub fn compute() -> Self {
        let mut layout = LayoutEngine::new();
        let down = layout
            .leaf(BUTTON_WIDTH, BUTTON_HEIGHT)
            .expect("static brightness-down layout should be valid");
        let up = layout
            .leaf(BUTTON_WIDTH, BUTTON_HEIGHT)
            .expect("static brightness-up layout should be valid");
        let root = layout
            .row(&[down, up], BUTTON_GAP)
            .expect("static brightness row should be valid");

        layout
            .compute(
                root,
                BUTTON_WIDTH * 2.0 + BUTTON_GAP,
                BUTTON_HEIGHT,
            )
            .expect("static brightness layout should compute");

        Self {
            down: offset(
                layout
                    .rect(down)
                    .expect("brightness-down rectangle should exist"),
                BRIGHTNESS_X,
                BRIGHTNESS_Y,
            ),
            up: offset(
                layout.rect(up).expect("brightness-up rectangle should exist"),
                BRIGHTNESS_X,
                BRIGHTNESS_Y,
            ),
        }
    }
}

impl HomePrimaryLayout {
    pub fn compute() -> Self {
        let mut layout = LayoutEngine::new();
        let action = layout
            .leaf(PRIMARY_WIDTH, PRIMARY_HEIGHT)
            .expect("static primary-action layout should be valid");
        layout
            .compute(action, PRIMARY_WIDTH, PRIMARY_HEIGHT)
            .expect("static primary-action layout should compute");

        Self {
            action: offset(
                layout
                    .rect(action)
                    .expect("primary-action rectangle should exist"),
                PRIMARY_X,
                PRIMARY_Y,
            ),
        }
    }
}

impl PanelNavLayout {
    pub fn compute() -> Self {
        let mut layout = LayoutEngine::new();
        let home = layout
            .leaf(NAV_HOME_WIDTH, NAV_HEIGHT)
            .expect("static Home navigation layout should be valid");
        let settings = layout
            .leaf(NAV_SETTINGS_WIDTH, NAV_HEIGHT)
            .expect("static Settings navigation layout should be valid");
        let root = layout
            .row(&[home, settings], NAV_GAP)
            .expect("static navigation row should be valid");

        layout
            .compute(
                root,
                NAV_HOME_WIDTH + NAV_SETTINGS_WIDTH + NAV_GAP,
                NAV_HEIGHT,
            )
            .expect("static navigation layout should compute");

        Self {
            home: offset(
                layout.rect(home).expect("Home navigation rectangle should exist"),
                NAV_X,
                NAV_Y,
            ),
            settings: offset(
                layout
                    .rect(settings)
                    .expect("Settings navigation rectangle should exist"),
                NAV_X,
                NAV_Y,
            ),
        }
    }
}

impl SocialTabsLayout {
    pub fn compute() -> Self {
        let mut layout = LayoutEngine::new();
        let whitelist = layout
            .leaf(SOCIAL_TAB_WIDTH, SOCIAL_TAB_HEIGHT)
            .expect("static whitelist tab layout should be valid");
        let blacklist = layout
            .leaf(SOCIAL_TAB_WIDTH, SOCIAL_TAB_HEIGHT)
            .expect("static blacklist tab layout should be valid");
        let root = layout
            .row(&[whitelist, blacklist], SOCIAL_TAB_GAP)
            .expect("static social tab row should be valid");

        layout
            .compute(
                root,
                SOCIAL_TAB_WIDTH * 2.0 + SOCIAL_TAB_GAP,
                SOCIAL_TAB_HEIGHT,
            )
            .expect("static social tab layout should compute");

        Self {
            whitelist: offset(
                layout
                    .rect(whitelist)
                    .expect("whitelist tab rectangle should exist"),
                SOCIAL_TABS_X,
                SOCIAL_TABS_Y,
            ),
            blacklist: offset(
                layout
                    .rect(blacklist)
                    .expect("blacklist tab rectangle should exist"),
                SOCIAL_TABS_X,
                SOCIAL_TABS_Y,
            ),
        }
    }
}

impl SettingsToggleLayout {
    pub fn compute() -> Self {
        let mut layout = LayoutEngine::new();
        let heart_rate = layout
            .leaf(SETTINGS_TOGGLE_WIDTH, SETTINGS_TOGGLE_HEIGHT)
            .expect("static heart-rate toggle layout should be valid");
        let osc = layout
            .leaf(SETTINGS_TOGGLE_WIDTH, SETTINGS_TOGGLE_HEIGHT)
            .expect("static OSC toggle layout should be valid");
        let root = layout
            .column(&[heart_rate, osc], SETTINGS_TOGGLE_GAP)
            .expect("static Settings toggle column should be valid");

        layout
            .compute(
                root,
                SETTINGS_TOGGLE_WIDTH,
                SETTINGS_TOGGLE_HEIGHT * 2.0 + SETTINGS_TOGGLE_GAP,
            )
            .expect("static Settings toggle layout should compute");

        Self {
            heart_rate: offset(
                layout
                    .rect(heart_rate)
                    .expect("heart-rate toggle rectangle should exist"),
                SETTINGS_TOGGLE_X,
                SETTINGS_TOGGLE_Y,
            ),
            osc: offset(
                layout.rect(osc).expect("OSC toggle rectangle should exist"),
                SETTINGS_TOGGLE_X,
                SETTINGS_TOGGLE_Y,
            ),
        }
    }

    pub fn heart_rate_hit(self) -> LayoutRect {
        expand(
            self.heart_rate,
            SETTINGS_TOGGLE_HIT_PAD_X,
            SETTINGS_TOGGLE_HIT_PAD_Y,
        )
    }

    pub fn osc_hit(self) -> LayoutRect {
        expand(
            self.osc,
            SETTINGS_TOGGLE_HIT_PAD_X,
            SETTINGS_TOGGLE_HIT_PAD_Y,
        )
    }
}

impl SettingsValueLayout {
    pub fn compute() -> Self {
        let mut layout = LayoutEngine::new();
        let invite_rules = layout
            .leaf(SETTINGS_VALUE_WIDTH, SETTINGS_VALUE_HEIGHT)
            .expect("static invite-rules value layout should be valid");
        let vrchat_status = layout
            .leaf(SETTINGS_VALUE_WIDTH, SETTINGS_VALUE_HEIGHT)
            .expect("static VRChat-status value layout should be valid");
        let root = layout
            .column(&[invite_rules, vrchat_status], SETTINGS_VALUE_GAP)
            .expect("static Settings value column should be valid");

        layout
            .compute(
                root,
                SETTINGS_VALUE_WIDTH,
                SETTINGS_VALUE_HEIGHT * 2.0 + SETTINGS_VALUE_GAP,
            )
            .expect("static Settings value layout should compute");

        Self {
            invite_rules: offset(
                layout
                    .rect(invite_rules)
                    .expect("invite-rules value rectangle should exist"),
                SETTINGS_VALUE_X,
                SETTINGS_VALUE_Y,
            ),
            vrchat_status: offset(
                layout
                    .rect(vrchat_status)
                    .expect("VRChat-status value rectangle should exist"),
                SETTINGS_VALUE_X,
                SETTINGS_VALUE_Y,
            ),
        }
    }
}

impl SocialRemoveLayout {
    pub fn compute() -> Self {
        let mut layout = LayoutEngine::new();
        let button0 = layout
            .leaf(SOCIAL_REMOVE_WIDTH, SOCIAL_REMOVE_HEIGHT)
            .expect("static social-remove layout should be valid");
        let button1 = layout
            .leaf(SOCIAL_REMOVE_WIDTH, SOCIAL_REMOVE_HEIGHT)
            .expect("static social-remove layout should be valid");
        let button2 = layout
            .leaf(SOCIAL_REMOVE_WIDTH, SOCIAL_REMOVE_HEIGHT)
            .expect("static social-remove layout should be valid");
        let button3 = layout
            .leaf(SOCIAL_REMOVE_WIDTH, SOCIAL_REMOVE_HEIGHT)
            .expect("static social-remove layout should be valid");
        let button4 = layout
            .leaf(SOCIAL_REMOVE_WIDTH, SOCIAL_REMOVE_HEIGHT)
            .expect("static social-remove layout should be valid");
        let nodes = [button0, button1, button2, button3, button4];
        let root = layout
            .column(&nodes, SOCIAL_REMOVE_GAP)
            .expect("static social-remove column should be valid");

        layout
            .compute(
                root,
                SOCIAL_REMOVE_WIDTH,
                SOCIAL_REMOVE_HEIGHT * 5.0 + SOCIAL_REMOVE_GAP * 4.0,
            )
            .expect("static social-remove layout should compute");

        Self {
            buttons: nodes.map(|node| {
                offset(
                    layout
                        .rect(node)
                        .expect("social-remove rectangle should exist"),
                    SOCIAL_REMOVE_X,
                    SOCIAL_REMOVE_Y,
                )
            }),
        }
    }

    pub fn hit(self, index: usize) -> Option<LayoutRect> {
        self.buttons.get(index).copied().map(|rect| {
            expand_edges(
                rect,
                0.0,
                SOCIAL_REMOVE_HIT_TOP,
                SOCIAL_REMOVE_HIT_RIGHT,
                SOCIAL_REMOVE_HIT_BOTTOM,
            )
        })
    }
}

pub fn contains(rect: LayoutRect, x: f32, y: f32) -> bool {
    x >= rect.x
        && x <= rect.x + rect.width
        && y >= rect.y
        && y <= rect.y + rect.height
}

fn offset(mut rect: LayoutRect, x: f32, y: f32) -> LayoutRect {
    rect.x += x;
    rect.y += y;
    rect
}

fn expand(rect: LayoutRect, x: f32, y: f32) -> LayoutRect {
    expand_edges(rect, x, y, x, y)
}

fn expand_edges(rect: LayoutRect, left: f32, top: f32, right: f32, bottom: f32) -> LayoutRect {
    LayoutRect {
        x: rect.x - left,
        y: rect.y - top,
        width: rect.width + left + right,
        height: rect.height + top + bottom,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brightness_buttons_keep_the_existing_visual_contract() {
        let layout = HomeBrightnessLayout::compute();

        assert_eq!(
            layout.down,
            LayoutRect {
                x: 60.0,
                y: 340.0,
                width: 56.0,
                height: 66.0,
            }
        );
        assert_eq!(
            layout.up,
            LayoutRect {
                x: 652.0,
                y: 340.0,
                width: 56.0,
                height: 66.0,
            }
        );
    }

    #[test]
    fn primary_action_keeps_the_existing_visual_contract() {
        assert_eq!(
            HomePrimaryLayout::compute().action,
            LayoutRect {
                x: 40.0,
                y: 840.0,
                width: 688.0,
                height: 74.0,
            }
        );
    }

    #[test]
    fn navigation_keeps_the_existing_interaction_contract() {
        let navigation = PanelNavLayout::compute();

        assert_eq!(
            navigation.home,
            LayoutRect {
                x: 500.0,
                y: 38.0,
                width: 94.0,
                height: 48.0,
            }
        );
        assert_eq!(
            navigation.settings,
            LayoutRect {
                x: 602.0,
                y: 38.0,
                width: 118.0,
                height: 48.0,
            }
        );
    }

    #[test]
    fn social_tabs_keep_the_existing_interaction_contract() {
        let tabs = SocialTabsLayout::compute();

        assert_eq!(
            tabs.whitelist,
            LayoutRect {
                x: 500.0,
                y: 302.0,
                width: 102.0,
                height: 46.0,
            }
        );
        assert_eq!(
            tabs.blacklist,
            LayoutRect {
                x: 606.0,
                y: 302.0,
                width: 102.0,
                height: 46.0,
            }
        );
    }

    #[test]
    fn settings_toggles_keep_visual_and_hit_contracts() {
        let toggles = SettingsToggleLayout::compute();

        assert_eq!(
            toggles.heart_rate,
            LayoutRect {
                x: 370.0,
                y: 261.0,
                width: 62.0,
                height: 36.0,
            }
        );
        assert_eq!(
            toggles.osc,
            LayoutRect {
                x: 370.0,
                y: 377.0,
                width: 62.0,
                height: 36.0,
            }
        );
        assert_eq!(
            toggles.heart_rate_hit(),
            LayoutRect {
                x: 356.0,
                y: 238.0,
                width: 90.0,
                height: 82.0,
            }
        );
        assert_eq!(
            toggles.osc_hit(),
            LayoutRect {
                x: 356.0,
                y: 354.0,
                width: 90.0,
                height: 82.0,
            }
        );
    }

    #[test]
    fn settings_values_keep_the_existing_interaction_contract() {
        let values = SettingsValueLayout::compute();
        assert_eq!(
            values.invite_rules,
            LayoutRect {
                x: 300.0,
                y: 470.0,
                width: 146.0,
                height: 82.0,
            }
        );
        assert_eq!(
            values.vrchat_status,
            LayoutRect {
                x: 300.0,
                y: 586.0,
                width: 146.0,
                height: 82.0,
            }
        );
    }

    #[test]
    fn social_remove_buttons_keep_visual_and_hit_contracts() {
        let remove = SocialRemoveLayout::compute();
        assert_eq!(
            remove.buttons[0],
            LayoutRect {
                x: 676.0,
                y: 368.0,
                width: 32.0,
                height: 44.0,
            }
        );
        assert_eq!(
            remove.buttons[4],
            LayoutRect {
                x: 676.0,
                y: 656.0,
                width: 32.0,
                height: 44.0,
            }
        );
        assert_eq!(
            remove.hit(0),
            Some(LayoutRect {
                x: 676.0,
                y: 366.0,
                width: 40.0,
                height: 54.0,
            })
        );
        assert_eq!(
            remove.hit(4),
            Some(LayoutRect {
                x: 676.0,
                y: 654.0,
                width: 40.0,
                height: 54.0,
            })
        );
    }

    #[test]
    fn hit_testing_uses_the_computed_rectangles() {
        let brightness = HomeBrightnessLayout::compute();
        let primary = HomePrimaryLayout::compute();
        let navigation = PanelNavLayout::compute();
        let tabs = SocialTabsLayout::compute();
        let toggles = SettingsToggleLayout::compute();
        let values = SettingsValueLayout::compute();
        let remove = SocialRemoveLayout::compute();

        assert!(contains(brightness.down, 60.0, 340.0));
        assert!(contains(brightness.up, 708.0, 406.0));
        assert!(!contains(brightness.down, 120.0, 340.0));
        assert!(contains(primary.action, 40.0, 840.0));
        assert!(contains(primary.action, 728.0, 914.0));
        assert!(!contains(primary.action, 729.0, 914.0));
        assert!(contains(navigation.home, 500.0, 38.0));
        assert!(contains(navigation.settings, 720.0, 86.0));
        assert!(!contains(navigation.home, 595.0, 86.0));
        assert!(contains(tabs.whitelist, 500.0, 302.0));
        assert!(contains(tabs.blacklist, 708.0, 348.0));
        assert!(!contains(tabs.whitelist, 603.0, 348.0));
        assert!(contains(toggles.heart_rate_hit(), 356.0, 238.0));
        assert!(contains(toggles.osc_hit(), 446.0, 436.0));
        assert!(!contains(toggles.heart_rate_hit(), 447.0, 320.0));
        assert!(contains(values.invite_rules, 300.0, 470.0));
        assert!(contains(values.vrchat_status, 446.0, 668.0));
        assert!(contains(remove.hit(0).unwrap(), 716.0, 420.0));
        assert!(!contains(remove.hit(0).unwrap(), 717.0, 420.0));
    }
}
