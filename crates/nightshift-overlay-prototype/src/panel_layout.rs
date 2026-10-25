use crate::{
    interaction::{PanelAction, PanelPage},
    layout::{LayoutEngine, LayoutRect},
    widget::{Widget, WidgetArena, WidgetId, WidgetKind},
};

pub const PANEL_WIDTH: f32 = 768.0;
pub const PANEL_HEIGHT: f32 = 960.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActionRect {
    pub action: PanelAction,
    pub rect: LayoutRect,
}

#[derive(Clone, Debug, Default)]
pub struct PanelLayout {
    regions: Vec<ActionRect>,
}

impl PanelLayout {
    pub fn for_page(page: PanelPage, wake_action: bool) -> Self {
        let mut builder = Builder::default();

        builder.row(
            500.0,
            38.0,
            8.0,
            &[
                ("nav-home", PanelAction::NavigateHome, 94.0, 48.0),
                ("nav-settings", PanelAction::NavigateSettings, 118.0, 48.0),
            ],
        );

        match page {
            PanelPage::Home => {
                builder.row(
                    60.0,
                    340.0,
                    536.0,
                    &[
                        ("brightness-down", PanelAction::BrightnessDown, 56.0, 66.0),
                        ("brightness-up", PanelAction::BrightnessUp, 56.0, 66.0),
                    ],
                );
                builder.single(
                    40.0,
                    840.0,
                    "primary-sleep-action",
                    if wake_action {
                        PanelAction::Wake
                    } else {
                        PanelAction::EnterSleep
                    },
                    688.0,
                    74.0,
                );
            }
            PanelPage::Settings => {
                for (name, action, x, y, width, height) in [
                    ("settings-hr", PanelAction::ToggleHeartRate, 356.0, 238.0, 90.0, 82.0),
                    ("settings-osc", PanelAction::ToggleOsc, 356.0, 354.0, 90.0, 82.0),
                    ("settings-invites", PanelAction::CycleInviteProfile, 300.0, 470.0, 146.0, 82.0),
                    ("settings-vrchat", PanelAction::CycleVrchatStatus, 300.0, 586.0, 146.0, 82.0),
                ] {
                    builder.single(x, y, name, action, width, height);
                }

                builder.row(
                    500.0,
                    302.0,
                    4.0,
                    &[
                        ("social-whitelist", PanelAction::SelectWhitelist, 102.0, 46.0),
                        ("social-blacklist", PanelAction::SelectBlacklist, 102.0, 46.0),
                    ],
                );

                builder.column(
                    676.0,
                    366.0,
                    18.0,
                    &[
                        ("social-remove-0", PanelAction::RemoveSocial(0), 40.0, 54.0),
                        ("social-remove-1", PanelAction::RemoveSocial(1), 40.0, 54.0),
                        ("social-remove-2", PanelAction::RemoveSocial(2), 40.0, 54.0),
                        ("social-remove-3", PanelAction::RemoveSocial(3), 40.0, 54.0),
                        ("social-remove-4", PanelAction::RemoveSocial(4), 40.0, 54.0),
                    ],
                );
            }
        }

        builder.finish()
    }

    pub fn rect(&self, action: PanelAction) -> Option<LayoutRect> {
        self.regions
            .iter()
            .find(|region| region.action == action)
            .map(|region| region.rect)
    }

    pub fn action_at_pixels(&self, x: f32, y: f32) -> Option<PanelAction> {
        self.regions
            .iter()
            .find(|region| contains(region.rect, x, y))
            .map(|region| region.action)
    }

    pub fn regions(&self) -> &[ActionRect] {
        &self.regions
    }
}

#[derive(Default)]
struct Builder {
    regions: Vec<ActionRect>,
}

impl Builder {
    fn single(
        &mut self,
        x: f32,
        y: f32,
        name: &'static str,
        action: PanelAction,
        width: f32,
        height: f32,
    ) {
        self.group(x, y, Axis::Row, 0.0, &[(name, action, width, height)]);
    }

    fn row(
        &mut self,
        x: f32,
        y: f32,
        gap: f32,
        items: &[(&'static str, PanelAction, f32, f32)],
    ) {
        self.group(x, y, Axis::Row, gap, items);
    }

    fn column(
        &mut self,
        x: f32,
        y: f32,
        gap: f32,
        items: &[(&'static str, PanelAction, f32, f32)],
    ) {
        self.group(x, y, Axis::Column, gap, items);
    }

    fn group(
        &mut self,
        origin_x: f32,
        origin_y: f32,
        axis: Axis,
        gap: f32,
        items: &[(&'static str, PanelAction, f32, f32)],
    ) {
        let mut widgets = WidgetArena::new();
        let mut layout = LayoutEngine::new();
        let mut nodes = Vec::with_capacity(items.len());
        let mut ids = Vec::<(PanelAction, WidgetId)>::with_capacity(items.len());

        for &(name, action, width, height) in items {
            let id = widgets.insert(Widget::new(name, WidgetKind::Button));
            let node = layout.leaf(width, height).expect("static panel layout leaf");
            layout.bind(id, node);
            nodes.push(node);
            ids.push((action, id));
        }

        let root = match axis {
            Axis::Row => layout.row(&nodes, gap),
            Axis::Column => layout.column(&nodes, gap),
        }
        .expect("static panel layout group");

        layout
            .compute(root, PANEL_WIDTH, PANEL_HEIGHT)
            .expect("computing static panel layout");

        for (action, id) in ids {
            let mut rect = layout
                .rect_for_widget(id)
                .expect("bound static panel widget geometry");
            rect.x += origin_x;
            rect.y += origin_y;
            self.regions.push(ActionRect { action, rect });
        }
    }

    fn finish(self) -> PanelLayout {
        PanelLayout {
            regions: self.regions,
        }
    }
}

#[derive(Clone, Copy)]
enum Axis {
    Row,
    Column,
}

fn contains(rect: LayoutRect, x: f32, y: f32) -> bool {
    x >= rect.x && x <= rect.x + rect.width && y >= rect.y && y <= rect.y + rect.height
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_geometry_preserves_existing_interaction_contract() {
        let layout = PanelLayout::for_page(PanelPage::Home, false);
        assert_eq!(
            layout.rect(PanelAction::BrightnessDown),
            Some(LayoutRect {
                x: 60.0,
                y: 340.0,
                width: 56.0,
                height: 66.0,
            })
        );
        assert_eq!(
            layout.action_at_pixels(384.0, 875.0),
            Some(PanelAction::EnterSleep)
        );
    }

    #[test]
    fn wake_state_changes_primary_action_without_moving_it() {
        let sleep = PanelLayout::for_page(PanelPage::Home, false);
        let wake = PanelLayout::for_page(PanelPage::Home, true);
        assert_eq!(
            sleep.rect(PanelAction::EnterSleep),
            wake.rect(PanelAction::Wake)
        );
    }

    #[test]
    fn social_remove_buttons_are_taffy_spaced() {
        let layout = PanelLayout::for_page(PanelPage::Settings, false);
        let first = layout.rect(PanelAction::RemoveSocial(0)).unwrap();
        let second = layout.rect(PanelAction::RemoveSocial(1)).unwrap();
        assert_eq!(second.y - first.y, 72.0);
    }
}
