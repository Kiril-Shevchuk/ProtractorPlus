use std::sync::OnceLock;

use fontdue::Font;
use tiny_skia::{Pixmap, PremultipliedColorU8};

static FONT: OnceLock<Font> = OnceLock::new();

fn ui_font() -> &'static Font {
    FONT.get_or_init(|| {
        let bytes = include_bytes!(concat!(env!("OUT_DIR"), "/comfortaa-variable.ttf"));
        Font::from_bytes(&bytes[..], fontdue::FontSettings {
            // The degree label is rendered near this size, so optimizing the
            // outlines around 18 px keeps the small text crisp.
            scale: 18.0,
            ..fontdue::FontSettings::default()
        })
        .expect("ProtractorPlus could not load the embedded Comfortaa font")
    })
}

pub struct TextLayout {
    pub width: f32,
    pub height: f32,
    pub xmin: f32,
    pub xmax: f32,
    pub ymin: f32,
    pub ymax: f32,
}

pub fn layout_text(text: &str, size: f32) -> TextLayout {
    let font = ui_font();
    let mut pen_x = 0.0f32;
    let mut xmin = f32::MAX;
    let mut xmax = f32::MIN;
    let mut ymin = f32::MAX;
    let mut ymax = f32::MIN;

    for ch in text.chars() {
        let (metrics, _) = font.rasterize(ch, size);
        xmin = xmin.min(pen_x + metrics.xmin as f32);
        xmax = xmax.max(pen_x + metrics.xmin as f32 + metrics.width as f32);
        ymin = ymin.min(metrics.ymin as f32);
        ymax = ymax.max(metrics.ymin as f32 + metrics.height as f32);
        pen_x += metrics.advance_width;
    }

    if !xmin.is_finite() || !xmax.is_finite() || !ymin.is_finite() || !ymax.is_finite() {
        return TextLayout {
            width: 0.0,
            height: 0.0,
            xmin: 0.0,
            xmax: 0.0,
            ymin: 0.0,
            ymax: 0.0,
        };
    }

    TextLayout {
        width: xmax - xmin,
        height: ymax - ymin,
        xmin,
        xmax,
        ymin,
        ymax,
    }
}

pub fn draw_text(
    pixmap: &mut Pixmap,
    text: &str,
    mut x: f32,
    baseline_y: f32,
    size: f32,
    color: tiny_skia::Color,
) {
    let font = ui_font();
    let rgba = color.to_color_u8();
    let pixmap_width = pixmap.width() as usize;
    let pixmap_height = pixmap.height() as i32;

    for ch in text.chars() {
        let (metrics, bitmap) = font.rasterize(ch, size);
        let glyph_x = x.round() as i32 + metrics.xmin;
        let glyph_y = baseline_y.round() as i32 - metrics.ymin - metrics.height as i32;

        for row in 0..metrics.height {
            for col in 0..metrics.width {
                let alpha = bitmap[row * metrics.width + col];
                if alpha == 0 {
                    continue;
                }

                let px = glyph_x + col as i32;
                let py = glyph_y + row as i32;
                if px < 0 || py < 0 || px >= pixmap_width as i32 || py >= pixmap_height {
                    continue;
                }

                let a = ((alpha as u16 * rgba.alpha() as u16) / 255) as u8;
                let src = PremultipliedColorU8::from_rgba(
                    ((rgba.red() as u16 * a as u16) / 255) as u8,
                    ((rgba.green() as u16 * a as u16) / 255) as u8,
                    ((rgba.blue() as u16 * a as u16) / 255) as u8,
                    a,
                )
                .unwrap();
                pixmap.pixels_mut()[py as usize * pixmap_width + px as usize] = src;
            }
        }

        x += metrics.advance_width;
    }
}
