use std::sync::OnceLock;

use fontdue::Font;
use tiny_skia::{Pixmap, PremultipliedColorU8};

static FONT: OnceLock<Font> = OnceLock::new();

fn ui_font() -> &'static Font {
    FONT.get_or_init(|| {
        let paths = [
            r"C:\Windows\Fonts\segoeui.ttf",
            r"C:\Windows\Fonts\arial.ttf",
            r"C:\Windows\Fonts\tahoma.ttf",
        ];
        for path in paths {
            if let Ok(data) = std::fs::read(path) {
                if let Ok(font) = Font::from_bytes(data, fontdue::FontSettings::default()) {
                    return font;
                }
            }
        }
        panic!("ProtractorPlus could not load a Windows UI font");
    })
}

pub struct TextLayout {
    pub width: f32,
    pub height: f32,
    pub ymin: f32,
    pub ymax: f32,
}

pub fn layout_text(text: &str, size: f32) -> TextLayout {
    let font = ui_font();
    let mut width = 0.0f32;
    let mut ymin = f32::MAX;
    let mut ymax = f32::MIN;
    for ch in text.chars() {
        let (metrics, _) = font.rasterize(ch, size);
        ymin = ymin.min(metrics.ymin as f32);
        ymax = ymax.max(metrics.ymin as f32 + metrics.height as f32);
        width += metrics.advance_width;
    }
    if !ymin.is_finite() || !ymax.is_finite() {
        return TextLayout { width: 0.0, height: 0.0, ymin: 0.0, ymax: 0.0 };
    }
    TextLayout { width, height: ymax - ymin, ymin, ymax }
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
    for ch in text.chars() {
        let (metrics, bitmap) = font.rasterize(ch, size);
        let gx = x.round() as i32 + metrics.xmin;
        let gy = baseline_y.round() as i32 - metrics.ymin - metrics.height as i32;
        for row in 0..metrics.height {
            for col in 0..metrics.width {
                let alpha = bitmap[row * metrics.width + col];
                if alpha == 0 { continue; }
                let px = gx + col as i32;
                let py = gy + row as i32;
                if px < 0 || py < 0 || px >= pixmap.width() as i32 || py >= pixmap.height() as i32 {
                    continue;
                }
                let a = ((alpha as u16 * rgba.alpha() as u16) / 255) as u8;
                let src = PremultipliedColorU8::from_rgba(
                    ((rgba.red() as u16 * a as u16) / 255) as u8,
                    ((rgba.green() as u16 * a as u16) / 255) as u8,
                    ((rgba.blue() as u16 * a as u16) / 255) as u8,
                    a,
                ).unwrap();
                pixmap.pixels_mut()[py as usize * pixmap_width + px as usize] = src;
            }
        }
        x += metrics.advance_width;
    }
}
