use std::cell::RefCell;

use cosmic_text::{Attrs, Buffer, Color, FontSystem, Metrics, Shaping, SwashCache, Wrap};

struct TextEngine {
    fonts: FontSystem,
    glyphs: SwashCache,
}

impl TextEngine {
    fn new() -> Self {
        Self {
            fonts: FontSystem::new(),
            glyphs: SwashCache::new(),
        }
    }

    fn draw(
        &mut self,
        text: &str,
        font_size: f32,
        line_height: f32,
        max_width: f32,
        color: [u8; 4],
        mut pixel: impl FnMut(i32, i32, u32, u32, [u8; 4]),
    ) {
        let mut buffer = Buffer::new(&mut self.fonts, Metrics::new(font_size, line_height));
        buffer.set_wrap(Wrap::None);
        buffer.set_size(Some(max_width.max(1.0)), Some(line_height.max(1.0)));
        buffer.set_text(text, &Attrs::new(), Shaping::Advanced, None);
        buffer.draw(
            &mut self.fonts,
            &mut self.glyphs,
            Color::rgba(color[0], color[1], color[2], color[3]),
            |x, y, width, height, color| pixel(x, y, width, height, color.as_rgba()),
        );
    }
}

thread_local! {
    // Font discovery is expensive enough that even Hydra refuses to do it for every label.
    static TEXT_ENGINE: RefCell<TextEngine> = RefCell::new(TextEngine::new());
}

pub(super) fn rasterize_text(
    text: &str,
    scale: u32,
    max_width: u32,
    color: [u8; 4],
    pixel: impl FnMut(i32, i32, u32, u32, [u8; 4]),
) {
    let scale = scale.max(1) as f32;
    TEXT_ENGINE.with(|engine| {
        engine.borrow_mut().draw(
            text,
            7.0 * scale,
            8.0 * scale,
            max_width as f32,
            color,
            pixel,
        );
    });
}
