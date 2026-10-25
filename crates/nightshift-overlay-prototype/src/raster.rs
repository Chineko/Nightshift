#[path = "raster/effects.rs"]
mod effects;
#[path = "raster/home.rs"]
mod home;
#[path = "raster/primitives.rs"]
mod primitives;
#[path = "raster/settings.rs"]
mod settings;
#[path = "raster/text.rs"]
mod text;

use super::{
    content::{PanelContent, SocialTab},
    interaction::{PanelAction, PanelPage},
};

pub const PANEL_BITMAP_WIDTH: u32 = 768;
pub const PANEL_BITMAP_HEIGHT: u32 = 960;

const BACKGROUND: [u8; 4] = [8, 9, 15, 255];
const PANEL: [u8; 4] = [14, 16, 25, 255];
const PANEL_ALT: [u8; 4] = [18, 20, 31, 255];
const BORDER: [u8; 4] = [38, 42, 58, 255];
const TEXT: [u8; 4] = [244, 244, 249, 255];
const MUTED: [u8; 4] = [139, 142, 166, 255];
const SHADOW: [u8; 4] = [4, 5, 9, 255];
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::SocialTab;
    use crate::interaction::PanelPage;
    use nightshift_protocol::{AppState, SleepPhase};

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
