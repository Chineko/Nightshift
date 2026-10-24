#[path = "../src/content.rs"]
mod content;
#[path = "../src/interaction.rs"]
mod interaction;
#[path = "../src/panel_cache.rs"]
mod panel_cache;
#[path = "../src/panel_state.rs"]
mod panel_state;
#[path = "../src/placement.rs"]
mod placement;
#[path = "../src/raster.rs"]
mod raster;

use content::PanelContent;
use nightshift_protocol::{AppState, SleepPhase};
use panel_cache::PanelBitmapCache;
use raster::{PANEL_BITMAP_HEIGHT, PANEL_BITMAP_WIDTH, PanelBitmap};

#[test]
fn daemon_state_changes_produce_a_new_panel_bitmap() {
    let awake = AppState::default();
    let awake_content = PanelContent::from_state(&awake);
    let awake_bitmap = PanelBitmap::from_content(&awake_content);

    let mut sleeping = awake;
    sleeping.sleep.phase = SleepPhase::Sleeping;
    sleeping.brightness.current = 0.42;
    let sleeping_content = PanelContent::from_state(&sleeping);
    let sleeping_bitmap = PanelBitmap::from_content(&sleeping_content);

    assert_ne!(awake_content, sleeping_content);
    assert_ne!(awake_bitmap.rgba, sleeping_bitmap.rgba);
    assert_eq!(sleeping_bitmap.width, PANEL_BITMAP_WIDTH);
    assert_eq!(sleeping_bitmap.height, PANEL_BITMAP_HEIGHT);
}

#[test]
fn runtime_cache_skips_an_unchanged_panel() {
    let content = PanelContent::from_state(&AppState::default());
    let mut cache = PanelBitmapCache::default();
    assert!(cache.render_if_changed(&content, None).is_some());
    assert!(cache.render_if_changed(&content, None).is_none());
}
