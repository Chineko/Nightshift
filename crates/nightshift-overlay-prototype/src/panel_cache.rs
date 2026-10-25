#[path = "social.rs"]
mod social;

use crate::{content::PanelContent, interaction::PanelAction, raster::PanelBitmap};
use social::SocialAvatarRuntime;

#[derive(Debug)]
pub struct PanelBitmapCache {
    last_content: Option<PanelContent>,
    last_hover: Option<PanelAction>,
    last_social_revision: u64,
    social: SocialAvatarRuntime,
}

impl Default for PanelBitmapCache {
    fn default() -> Self {
        Self {
            last_content: None,
            last_hover: None,
            last_social_revision: 0,
            social: SocialAvatarRuntime::spawn(),
        }
    }
}

impl PanelBitmapCache {
    pub fn render_if_changed(
        &mut self,
        content: &PanelContent,
        hover: Option<PanelAction>,
    ) -> Option<PanelBitmap> {
        self.social.poll();
        self.social.request_visible(content);
        let social_revision = self.social.revision();

        if self.last_content.as_ref() == Some(content)
            && self.last_hover == hover
            && self.last_social_revision == social_revision
        {
            return None;
        }

        let mut bitmap = PanelBitmap::from_content_with_hover(content, hover);
        self.social.decorate(&mut bitmap, content, hover);
        self.last_content = Some(content.clone());
        self.last_hover = hover;
        self.last_social_revision = social_revision;
        Some(bitmap)
    }

    pub fn invalidate(&mut self) {
        // Sometimes the compositor eats our homework. This lets the caller make us redraw it.
        self.last_content = None;
    }
}

#[cfg(test)]
mod tests {
    use nightshift_protocol::{AppState, SleepPhase};

    use super::*;

    #[test]
    fn identical_content_does_not_raster_twice() {
        let content = PanelContent::from_state(&AppState::default());
        let mut cache = PanelBitmapCache::default();
        assert!(cache.render_if_changed(&content, None).is_some());
        assert!(cache.render_if_changed(&content, None).is_none());
    }

    #[test]
    fn changed_content_produces_a_new_bitmap() {
        let mut state = AppState::default();
        let first = PanelContent::from_state(&state);
        state.sleep.phase = SleepPhase::Sleeping;
        let second = PanelContent::from_state(&state);

        let mut cache = PanelBitmapCache::default();
        assert!(cache.render_if_changed(&first, None).is_some());
        assert!(cache.render_if_changed(&second, None).is_some());
    }

    #[test]
    fn invalidation_forces_exact_content_to_render_again() {
        let content = PanelContent::from_state(&AppState::default());
        let mut cache = PanelBitmapCache::default();
        assert!(cache.render_if_changed(&content, None).is_some());
        assert!(cache.render_if_changed(&content, None).is_none());
        cache.invalidate();
        assert!(cache.render_if_changed(&content, None).is_some());
    }

    #[test]
    fn hover_change_produces_a_new_bitmap() {
        let content = PanelContent::from_state(&AppState::default());
        let mut cache = PanelBitmapCache::default();
        assert!(cache.render_if_changed(&content, None).is_some());
        assert!(
            cache
                .render_if_changed(&content, Some(PanelAction::Wake))
                .is_some()
        );
        assert!(
            cache
                .render_if_changed(&content, Some(PanelAction::Wake))
                .is_none()
        );
    }
}
