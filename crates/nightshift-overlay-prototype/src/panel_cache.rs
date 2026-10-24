use crate::{content::PanelContent, raster::PanelBitmap};

#[derive(Clone, Debug, Default)]
pub struct PanelBitmapCache {
    last_content: Option<PanelContent>,
}

impl PanelBitmapCache {
    pub fn render_if_changed(&mut self, content: &PanelContent) -> Option<PanelBitmap> {
        if self.last_content.as_ref() == Some(content) {
            return None;
        }

        let bitmap = PanelBitmap::from_content(content);
        self.last_content = Some(content.clone());
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
        assert!(cache.render_if_changed(&content).is_some());
        assert!(cache.render_if_changed(&content).is_none());
    }

    #[test]
    fn changed_content_produces_a_new_bitmap() {
        let mut state = AppState::default();
        let first = PanelContent::from_state(&state);
        state.sleep.phase = SleepPhase::Sleeping;
        let second = PanelContent::from_state(&state);

        let mut cache = PanelBitmapCache::default();
        assert!(cache.render_if_changed(&first).is_some());
        assert!(cache.render_if_changed(&second).is_some());
    }

    #[test]
    fn invalidation_forces_exact_content_to_render_again() {
        let content = PanelContent::from_state(&AppState::default());
        let mut cache = PanelBitmapCache::default();
        assert!(cache.render_if_changed(&content).is_some());
        assert!(cache.render_if_changed(&content).is_none());
        cache.invalidate();
        assert!(cache.render_if_changed(&content).is_some());
    }
}
