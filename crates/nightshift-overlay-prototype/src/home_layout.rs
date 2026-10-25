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
    fn hit_testing_uses_the_computed_rectangles() {
        let brightness = HomeBrightnessLayout::compute();
        let primary = HomePrimaryLayout::compute();
        let navigation = PanelNavLayout::compute();

        assert!(contains(brightness.down, 60.0, 340.0));
        assert!(contains(brightness.up, 708.0, 406.0));
        assert!(!contains(brightness.down, 120.0, 340.0));
        assert!(contains(primary.action, 40.0, 840.0));
        assert!(contains(primary.action, 728.0, 914.0));
        assert!(!contains(primary.action, 729.0, 914.0));
        assert!(contains(navigation.home, 500.0, 38.0));
        assert!(contains(navigation.settings, 720.0, 86.0));
        assert!(!contains(navigation.home, 595.0, 86.0));
    }
}
