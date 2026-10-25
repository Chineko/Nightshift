use crate::layout::{LayoutEngine, LayoutRect};

const BRIGHTNESS_X: f32 = 60.0;
const BRIGHTNESS_Y: f32 = 340.0;
const BUTTON_WIDTH: f32 = 56.0;
const BUTTON_HEIGHT: f32 = 66.0;
const BUTTON_GAP: f32 = 536.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HomeBrightnessLayout {
    pub down: LayoutRect,
    pub up: LayoutRect,
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
            ),
            up: offset(
                layout.rect(up).expect("brightness-up rectangle should exist"),
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

fn offset(mut rect: LayoutRect) -> LayoutRect {
    rect.x += BRIGHTNESS_X;
    rect.y += BRIGHTNESS_Y;
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
    fn hit_testing_uses_the_computed_rectangles() {
        let layout = HomeBrightnessLayout::compute();

        assert!(contains(layout.down, 60.0, 340.0));
        assert!(contains(layout.up, 708.0, 406.0));
        assert!(!contains(layout.down, 120.0, 340.0));
    }
}
