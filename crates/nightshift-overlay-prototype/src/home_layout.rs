use crate::layout::{LayoutEngine, LayoutRect};

const PANEL_HEADER_X: f32 = 32.0;
const PANEL_HEADER_Y: f32 = 24.0;
const PANEL_HEADER_WIDTH: f32 = 704.0;
const PANEL_HEADER_HEIGHT: f32 = 72.0;

const HOME_X: f32 = 40.0;
const HOME_Y: f32 = 230.0;
const HOME_WIDTH: f32 = 688.0;
const HOME_BRIGHTNESS_HEIGHT: f32 = 250.0;
const HOME_STATUS_HEIGHT: f32 = 136.0;
const HOME_AUTOMATION_HEIGHT: f32 = 112.0;
const HOME_PRIMARY_HEIGHT: f32 = 74.0;
const HOME_TO_STATUS_GAP: f32 = 60.0;
const HOME_TO_AUTOMATION_GAP: f32 = 24.0;
const HOME_TO_PRIMARY_GAP: f32 = 28.0;
const HOME_STATUS_CARD_WIDTH: f32 = 216.0;
const HOME_STATUS_CARD_GAP: f32 = 20.0;
const HOME_BRIGHTNESS_BUTTON_WIDTH: f32 = 56.0;
const HOME_BRIGHTNESS_BUTTON_HEIGHT: f32 = 66.0;
const HOME_BRIGHTNESS_BUTTON_GAP: f32 = 536.0;
const HOME_BRIGHTNESS_BUTTON_X: f32 = 20.0;
const HOME_BRIGHTNESS_BUTTON_Y: f32 = 110.0;
const HOME_BRIGHTNESS_TRACK_X: f32 = 52.0;
const HOME_BRIGHTNESS_TRACK_Y: f32 = 140.0;
const HOME_BRIGHTNESS_TRACK_WIDTH: f32 = 584.0;
const HOME_BRIGHTNESS_TRACK_HEIGHT: f32 = 7.0;
const HOME_CONNECTION_X: f32 = 621.0;
const HOME_CONNECTION_Y: f32 = 135.0;
const HOME_CONNECTION_WIDTH: f32 = 99.0;
const HOME_CONNECTION_HEIGHT: f32 = 44.0;

const NAV_X: f32 = 500.0;
const NAV_Y: f32 = 38.0;
const NAV_HOME_WIDTH: f32 = 94.0;
const NAV_SETTINGS_WIDTH: f32 = 118.0;
const NAV_HEIGHT: f32 = 48.0;
const NAV_GAP: f32 = 8.0;

const SETTINGS_X: f32 = 40.0;
const SETTINGS_Y: f32 = 230.0;
const SETTINGS_WIDTH: f32 = 688.0;
const SETTINGS_HEIGHT: f32 = 610.0;
const SETTINGS_QUICK_WIDTH: f32 = 420.0;
const SETTINGS_QUICK_HEIGHT: f32 = 100.0;
const SETTINGS_QUICK_GAP: f32 = 16.0;
const SETTINGS_AUTOMATION_HEIGHT: f32 = 128.0;
const SETTINGS_QUICK_TO_AUTOMATION_GAP: f32 = 34.0;
const SETTINGS_SOCIAL_WIDTH: f32 = 248.0;
const SETTINGS_COLUMN_GAP: f32 = 20.0;
const SETTINGS_TOGGLE_WIDTH: f32 = 62.0;
const SETTINGS_TOGGLE_HEIGHT: f32 = 36.0;
const SETTINGS_TOGGLE_X: f32 = 330.0;
const SETTINGS_TOGGLE_Y: f32 = 31.0;
const SETTINGS_TOGGLE_HIT_PAD_X: f32 = 14.0;
const SETTINGS_TOGGLE_HIT_PAD_Y: f32 = 23.0;
const SETTINGS_VALUE_X: f32 = 260.0;
const SETTINGS_VALUE_Y: f32 = 8.0;
const SETTINGS_VALUE_WIDTH: f32 = 146.0;
const SETTINGS_VALUE_HEIGHT: f32 = 82.0;
const SOCIAL_TAB_X: f32 = 20.0;
const SOCIAL_TAB_Y: f32 = 72.0;
const SOCIAL_TAB_WIDTH: f32 = 102.0;
const SOCIAL_TAB_HEIGHT: f32 = 46.0;
const SOCIAL_TAB_GAP: f32 = 4.0;
const SOCIAL_ROW_X: f32 = 20.0;
const SOCIAL_ROW_Y: f32 = 130.0;
const SOCIAL_ROW_WIDTH: f32 = 208.0;
const SOCIAL_ROW_HEIGHT: f32 = 60.0;
const SOCIAL_ROW_GAP: f32 = 12.0;
const SOCIAL_REMOVE_X: f32 = 176.0;
const SOCIAL_REMOVE_Y: f32 = 8.0;
const SOCIAL_REMOVE_WIDTH: f32 = 32.0;
const SOCIAL_REMOVE_HEIGHT: f32 = 44.0;
const SOCIAL_REMOVE_HIT_TOP: f32 = 2.0;
const SOCIAL_REMOVE_HIT_RIGHT: f32 = 8.0;
const SOCIAL_REMOVE_HIT_BOTTOM: f32 = 8.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PanelChromeLayout {
    pub header: LayoutRect,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HomeCompositionLayout {
    pub brightness_panel: LayoutRect,
    pub brightness_track: LayoutRect,
    pub status_cards: [LayoutRect; 3],
    pub automation_panel: LayoutRect,
    pub primary_action: LayoutRect,
    pub connection_badge: LayoutRect,
}

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
pub struct SettingsCompositionLayout {
    pub quick_cards: [LayoutRect; 4],
    pub automation_panel: LayoutRect,
    pub social_panel: LayoutRect,
    pub social_rows: [LayoutRect; 5],
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

impl PanelChromeLayout {
    pub fn compute() -> Self {
        let mut layout = LayoutEngine::new();
        let header = layout
            .leaf(PANEL_HEADER_WIDTH, PANEL_HEADER_HEIGHT)
            .expect("static panel header layout should be valid");
        layout
            .compute(header, PANEL_HEADER_WIDTH, PANEL_HEADER_HEIGHT)
            .expect("static panel header layout should compute");
        Self {
            header: offset(
                layout.rect(header).expect("panel header rectangle should exist"),
                PANEL_HEADER_X,
                PANEL_HEADER_Y,
            ),
        }
    }
}

impl HomeCompositionLayout {
    pub fn compute() -> Self {
        let mut layout = LayoutEngine::new();
        let brightness_panel = layout
            .leaf(HOME_WIDTH, HOME_BRIGHTNESS_HEIGHT)
            .expect("static Home brightness panel should be valid");
        let gap_to_status = layout
            .leaf(HOME_WIDTH, HOME_TO_STATUS_GAP)
            .expect("static Home status spacer should be valid");
        let status0 = layout
            .leaf(HOME_STATUS_CARD_WIDTH, HOME_STATUS_HEIGHT)
            .expect("static Home status card should be valid");
        let status1 = layout
            .leaf(HOME_STATUS_CARD_WIDTH, HOME_STATUS_HEIGHT)
            .expect("static Home status card should be valid");
        let status2 = layout
            .leaf(HOME_STATUS_CARD_WIDTH, HOME_STATUS_HEIGHT)
            .expect("static Home status card should be valid");
        let status_nodes = [status0, status1, status2];
        let status_row = layout
            .row(&status_nodes, HOME_STATUS_CARD_GAP)
            .expect("static Home status row should be valid");
        let gap_to_automation = layout
            .leaf(HOME_WIDTH, HOME_TO_AUTOMATION_GAP)
            .expect("static Home automation spacer should be valid");
        let automation_panel = layout
            .leaf(HOME_WIDTH, HOME_AUTOMATION_HEIGHT)
            .expect("static Home automation panel should be valid");
        let gap_to_primary = layout
            .leaf(HOME_WIDTH, HOME_TO_PRIMARY_GAP)
            .expect("static Home primary spacer should be valid");
        let primary_action = layout
            .leaf(HOME_WIDTH, HOME_PRIMARY_HEIGHT)
            .expect("static Home primary action should be valid");
        let root = layout
            .column(
                &[
                    brightness_panel,
                    gap_to_status,
                    status_row,
                    gap_to_automation,
                    automation_panel,
                    gap_to_primary,
                    primary_action,
                ],
                0.0,
            )
            .expect("static Home composition should be valid");
        let height = HOME_BRIGHTNESS_HEIGHT
            + HOME_TO_STATUS_GAP
            + HOME_STATUS_HEIGHT
            + HOME_TO_AUTOMATION_GAP
            + HOME_AUTOMATION_HEIGHT
            + HOME_TO_PRIMARY_GAP
            + HOME_PRIMARY_HEIGHT;
        layout
            .compute(root, HOME_WIDTH, height)
            .expect("static Home composition should compute");

        let brightness_panel = offset(
            layout
                .rect(brightness_panel)
                .expect("Home brightness panel rectangle should exist"),
            HOME_X,
            HOME_Y,
        );
        let status_cards = status_nodes.map(|node| {
            offset(
                layout.rect(node).expect("Home status card rectangle should exist"),
                HOME_X,
                HOME_Y,
            )
        });
        let automation_panel = offset(
            layout
                .rect(automation_panel)
                .expect("Home automation panel rectangle should exist"),
            HOME_X,
            HOME_Y,
        );
        let primary_action = offset(
            layout
                .rect(primary_action)
                .expect("Home primary action rectangle should exist"),
            HOME_X,
            HOME_Y,
        );

        Self {
            brightness_panel,
            brightness_track: LayoutRect {
                x: brightness_panel.x + HOME_BRIGHTNESS_TRACK_X,
                y: brightness_panel.y + HOME_BRIGHTNESS_TRACK_Y,
                width: HOME_BRIGHTNESS_TRACK_WIDTH,
                height: HOME_BRIGHTNESS_TRACK_HEIGHT,
            },
            status_cards,
            automation_panel,
            primary_action,
            connection_badge: LayoutRect {
                x: HOME_CONNECTION_X,
                y: HOME_CONNECTION_Y,
                width: HOME_CONNECTION_WIDTH,
                height: HOME_CONNECTION_HEIGHT,
            },
        }
    }
}

impl HomeBrightnessLayout {
    pub fn compute() -> Self {
        let composition = HomeCompositionLayout::compute();
        let mut layout = LayoutEngine::new();
        let down = layout
            .leaf(HOME_BRIGHTNESS_BUTTON_WIDTH, HOME_BRIGHTNESS_BUTTON_HEIGHT)
            .expect("static brightness-down layout should be valid");
        let up = layout
            .leaf(HOME_BRIGHTNESS_BUTTON_WIDTH, HOME_BRIGHTNESS_BUTTON_HEIGHT)
            .expect("static brightness-up layout should be valid");
        let root = layout
            .row(&[down, up], HOME_BRIGHTNESS_BUTTON_GAP)
            .expect("static brightness row should be valid");
        layout
            .compute(
                root,
                HOME_BRIGHTNESS_BUTTON_WIDTH * 2.0 + HOME_BRIGHTNESS_BUTTON_GAP,
                HOME_BRIGHTNESS_BUTTON_HEIGHT,
            )
            .expect("static brightness layout should compute");

        let x = composition.brightness_panel.x + HOME_BRIGHTNESS_BUTTON_X;
        let y = composition.brightness_panel.y + HOME_BRIGHTNESS_BUTTON_Y;
        Self {
            down: offset(
                layout
                    .rect(down)
                    .expect("brightness-down rectangle should exist"),
                x,
                y,
            ),
            up: offset(
                layout.rect(up).expect("brightness-up rectangle should exist"),
                x,
                y,
            ),
        }
    }
}

impl HomePrimaryLayout {
    pub fn compute() -> Self {
        Self {
            action: HomeCompositionLayout::compute().primary_action,
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

impl SettingsCompositionLayout {
    pub fn compute() -> Self {
        let mut layout = LayoutEngine::new();
        let card0 = layout
            .leaf(SETTINGS_QUICK_WIDTH, SETTINGS_QUICK_HEIGHT)
            .expect("static Settings quick card should be valid");
        let card1 = layout
            .leaf(SETTINGS_QUICK_WIDTH, SETTINGS_QUICK_HEIGHT)
            .expect("static Settings quick card should be valid");
        let card2 = layout
            .leaf(SETTINGS_QUICK_WIDTH, SETTINGS_QUICK_HEIGHT)
            .expect("static Settings quick card should be valid");
        let card3 = layout
            .leaf(SETTINGS_QUICK_WIDTH, SETTINGS_QUICK_HEIGHT)
            .expect("static Settings quick card should be valid");
        let card_nodes = [card0, card1, card2, card3];
        let quick_column = layout
            .column(&card_nodes, SETTINGS_QUICK_GAP)
            .expect("static Settings quick-card column should be valid");
        let spacer = layout
            .leaf(SETTINGS_QUICK_WIDTH, SETTINGS_QUICK_TO_AUTOMATION_GAP)
            .expect("static Settings automation spacer should be valid");
        let automation_panel = layout
            .leaf(SETTINGS_QUICK_WIDTH, SETTINGS_AUTOMATION_HEIGHT)
            .expect("static Settings automation panel should be valid");
        let left = layout
            .column(&[quick_column, spacer, automation_panel], 0.0)
            .expect("static Settings left column should be valid");
        let social_panel = layout
            .leaf(SETTINGS_SOCIAL_WIDTH, SETTINGS_HEIGHT)
            .expect("static Settings social panel should be valid");
        let root = layout
            .row(&[left, social_panel], SETTINGS_COLUMN_GAP)
            .expect("static Settings composition should be valid");
        layout
            .compute(root, SETTINGS_WIDTH, SETTINGS_HEIGHT)
            .expect("static Settings composition should compute");

        let quick_cards = card_nodes.map(|node| {
            offset(
                layout.rect(node).expect("Settings quick-card rectangle should exist"),
                SETTINGS_X,
                SETTINGS_Y,
            )
        });
        let automation_panel = offset(
            layout
                .rect(automation_panel)
                .expect("Settings automation rectangle should exist"),
            SETTINGS_X,
            SETTINGS_Y,
        );
        let social_panel = offset(
            layout
                .rect(social_panel)
                .expect("Settings social-panel rectangle should exist"),
            SETTINGS_X,
            SETTINGS_Y,
        );

        let mut rows_layout = LayoutEngine::new();
        let row0 = rows_layout
            .leaf(SOCIAL_ROW_WIDTH, SOCIAL_ROW_HEIGHT)
            .expect("static social row should be valid");
        let row1 = rows_layout
            .leaf(SOCIAL_ROW_WIDTH, SOCIAL_ROW_HEIGHT)
            .expect("static social row should be valid");
        let row2 = rows_layout
            .leaf(SOCIAL_ROW_WIDTH, SOCIAL_ROW_HEIGHT)
            .expect("static social row should be valid");
        let row3 = rows_layout
            .leaf(SOCIAL_ROW_WIDTH, SOCIAL_ROW_HEIGHT)
            .expect("static social row should be valid");
        let row4 = rows_layout
            .leaf(SOCIAL_ROW_WIDTH, SOCIAL_ROW_HEIGHT)
            .expect("static social row should be valid");
        let row_nodes = [row0, row1, row2, row3, row4];
        let rows_root = rows_layout
            .column(&row_nodes, SOCIAL_ROW_GAP)
            .expect("static social row column should be valid");
        rows_layout
            .compute(
                rows_root,
                SOCIAL_ROW_WIDTH,
                SOCIAL_ROW_HEIGHT * 5.0 + SOCIAL_ROW_GAP * 4.0,
            )
            .expect("static social row column should compute");
        let row_x = social_panel.x + SOCIAL_ROW_X;
        let row_y = social_panel.y + SOCIAL_ROW_Y;
        let social_rows = row_nodes.map(|node| {
            offset(
                rows_layout.rect(node).expect("social row rectangle should exist"),
                row_x,
                row_y,
            )
        });

        Self {
            quick_cards,
            automation_panel,
            social_panel,
            social_rows,
        }
    }
}

impl SocialTabsLayout {
    pub fn compute() -> Self {
        let social = SettingsCompositionLayout::compute().social_panel;
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

        let x = social.x + SOCIAL_TAB_X;
        let y = social.y + SOCIAL_TAB_Y;
        Self {
            whitelist: offset(
                layout
                    .rect(whitelist)
                    .expect("whitelist tab rectangle should exist"),
                x,
                y,
            ),
            blacklist: offset(
                layout
                    .rect(blacklist)
                    .expect("blacklist tab rectangle should exist"),
                x,
                y,
            ),
        }
    }
}

impl SettingsToggleLayout {
    pub fn compute() -> Self {
        let cards = SettingsCompositionLayout::compute().quick_cards;
        Self {
            heart_rate: child_rect(
                cards[0],
                SETTINGS_TOGGLE_X,
                SETTINGS_TOGGLE_Y,
                SETTINGS_TOGGLE_WIDTH,
                SETTINGS_TOGGLE_HEIGHT,
            ),
            osc: child_rect(
                cards[1],
                SETTINGS_TOGGLE_X,
                SETTINGS_TOGGLE_Y,
                SETTINGS_TOGGLE_WIDTH,
                SETTINGS_TOGGLE_HEIGHT,
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
        let cards = SettingsCompositionLayout::compute().quick_cards;
        Self {
            invite_rules: child_rect(
                cards[2],
                SETTINGS_VALUE_X,
                SETTINGS_VALUE_Y,
                SETTINGS_VALUE_WIDTH,
                SETTINGS_VALUE_HEIGHT,
            ),
            vrchat_status: child_rect(
                cards[3],
                SETTINGS_VALUE_X,
                SETTINGS_VALUE_Y,
                SETTINGS_VALUE_WIDTH,
                SETTINGS_VALUE_HEIGHT,
            ),
        }
    }
}

impl SocialRemoveLayout {
    pub fn compute() -> Self {
        let rows = SettingsCompositionLayout::compute().social_rows;
        Self {
            buttons: rows.map(|row| {
                child_rect(
                    row,
                    SOCIAL_REMOVE_X,
                    SOCIAL_REMOVE_Y,
                    SOCIAL_REMOVE_WIDTH,
                    SOCIAL_REMOVE_HEIGHT,
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

fn child_rect(parent: LayoutRect, x: f32, y: f32, width: f32, height: f32) -> LayoutRect {
    LayoutRect {
        x: parent.x + x,
        y: parent.y + y,
        width,
        height,
    }
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
    fn panel_header_keeps_the_existing_visual_contract() {
        assert_eq!(
            PanelChromeLayout::compute().header,
            LayoutRect {
                x: 32.0,
                y: 24.0,
                width: 704.0,
                height: 72.0,
            }
        );
    }

    #[test]
    fn home_composition_keeps_the_existing_visual_contract() {
        let home = HomeCompositionLayout::compute();
        assert_eq!(
            home.brightness_panel,
            LayoutRect {
                x: 40.0,
                y: 230.0,
                width: 688.0,
                height: 250.0,
            }
        );
        assert_eq!(
            home.brightness_track,
            LayoutRect {
                x: 92.0,
                y: 370.0,
                width: 584.0,
                height: 7.0,
            }
        );
        assert_eq!(
            home.status_cards,
            [
                LayoutRect { x: 40.0, y: 540.0, width: 216.0, height: 136.0 },
                LayoutRect { x: 276.0, y: 540.0, width: 216.0, height: 136.0 },
                LayoutRect { x: 512.0, y: 540.0, width: 216.0, height: 136.0 },
            ]
        );
        assert_eq!(
            home.automation_panel,
            LayoutRect {
                x: 40.0,
                y: 700.0,
                width: 688.0,
                height: 112.0,
            }
        );
        assert_eq!(
            home.primary_action,
            LayoutRect {
                x: 40.0,
                y: 840.0,
                width: 688.0,
                height: 74.0,
            }
        );
    }

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
    fn settings_composition_keeps_the_existing_visual_contract() {
        let settings = SettingsCompositionLayout::compute();
        assert_eq!(
            settings.quick_cards,
            [
                LayoutRect { x: 40.0, y: 230.0, width: 420.0, height: 100.0 },
                LayoutRect { x: 40.0, y: 346.0, width: 420.0, height: 100.0 },
                LayoutRect { x: 40.0, y: 462.0, width: 420.0, height: 100.0 },
                LayoutRect { x: 40.0, y: 578.0, width: 420.0, height: 100.0 },
            ]
        );
        assert_eq!(
            settings.automation_panel,
            LayoutRect {
                x: 40.0,
                y: 712.0,
                width: 420.0,
                height: 128.0,
            }
        );
        assert_eq!(
            settings.social_panel,
            LayoutRect {
                x: 480.0,
                y: 230.0,
                width: 248.0,
                height: 610.0,
            }
        );
        assert_eq!(
            settings.social_rows[0],
            LayoutRect {
                x: 500.0,
                y: 360.0,
                width: 208.0,
                height: 60.0,
            }
        );
        assert_eq!(
            settings.social_rows[4],
            LayoutRect {
                x: 500.0,
                y: 648.0,
                width: 208.0,
                height: 60.0,
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
