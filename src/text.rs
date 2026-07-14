use std::sync::OnceLock;

use fontdue::Font;
use tiny_skia::{Color, Pixmap, PremultipliedColorU8};

static FONT: OnceLock<Font> = OnceLock::new();

// Glyphs are rasterized at four times the display resolution and then reduced
// to the target size. This gives the small Comfortaa label much smoother edges.
const TEXT_SUPERSAMPLE: i32 = 4;
const FONT_OPTIMIZATION_SCALE: f32 = 72.0;

fn ui_font() -> &'static Font {
    FONT.get_or_init(|| {
        let bytes = include_bytes!(concat!(env!("OUT_DIR"), "/comfortaa-variable.ttf"));
        Font::from_bytes(
            &bytes[..],
            fontdue::FontSettings {
                // The displayed label is about 16.5 px. At 4x supersampling it
                // is rasterized near 66 px, so the outlines are optimized near
                // that internal resolution rather than the final screen size.
                scale: FONT_OPTIMIZATION_SCALE,
                ..fontdue::FontSettings::default()
            },
        )
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
    let supersample = TEXT_SUPERSAMPLE as f32;
    let raster_size = size * supersample;
    let mut pen_x = 0.0f32;
    let mut xmin = f32::MAX;
    let mut xmax = f32::MIN;
    let mut ymin = f32::MAX;
    let mut ymax = f32::MIN;

    for ch in text.chars() {
        let (metrics, _) = font.rasterize(ch, raster_size);
        let glyph_xmin = metrics.xmin as f32 / supersample;
        let glyph_ymin = metrics.ymin as f32 / supersample;
        let glyph_width = metrics.width as f32 / supersample;
        let glyph_height = metrics.height as f32 / supersample;

        xmin = xmin.min(pen_x + glyph_xmin);
        xmax = xmax.max(pen_x + glyph_xmin + glyph_width);
        ymin = ymin.min(glyph_ymin);
        ymax = ymax.max(glyph_ymin + glyph_height);
        pen_x += metrics.advance_width / supersample;
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

fn floor_div(value: i32, divisor: i32) -> i32 {
    value.div_euclid(divisor)
}

fn ceil_div(value: i32, divisor: i32) -> i32 {
    -(-value).div_euclid(divisor)
}

fn blend_coverage(
    destination: PremultipliedColorU8,
    color: Color,
    coverage: u8,
) -> PremultipliedColorU8 {
    let rgba = color.to_color_u8();
    let source_alpha =
        (coverage as u32 * rgba.alpha() as u32 + 127) / 255;
    if source_alpha == 0 {
        return destination;
    }

    let inverse_alpha = 255 - source_alpha;
    let source_red = (rgba.red() as u32 * source_alpha + 127) / 255;
    let source_green = (rgba.green() as u32 * source_alpha + 127) / 255;
    let source_blue = (rgba.blue() as u32 * source_alpha + 127) / 255;

    let output_alpha = source_alpha
        + (destination.alpha() as u32 * inverse_alpha + 127) / 255;
    let output_red = source_red
        + (destination.red() as u32 * inverse_alpha + 127) / 255;
    let output_green = source_green
        + (destination.green() as u32 * inverse_alpha + 127) / 255;
    let output_blue = source_blue
        + (destination.blue() as u32 * inverse_alpha + 127) / 255;

    // Premultiplied channels must never exceed their alpha channel.
    let output_alpha = output_alpha.min(255) as u8;
    let output_red = output_red.min(output_alpha as u32).min(255) as u8;
    let output_green = output_green.min(output_alpha as u32).min(255) as u8;
    let output_blue = output_blue.min(output_alpha as u32).min(255) as u8;

    PremultipliedColorU8::from_rgba(
        output_red,
        output_green,
        output_blue,
        output_alpha,
    )
    .expect("valid premultiplied text color")
}

pub fn draw_text(
    pixmap: &mut Pixmap,
    text: &str,
    mut x: f32,
    baseline_y: f32,
    size: f32,
    color: Color,
) {
    let font = ui_font();
    let supersample = TEXT_SUPERSAMPLE;
    let supersample_f = supersample as f32;
    let raster_size = size * supersample_f;
    let samples_per_pixel = (supersample * supersample) as u32;
    let pixmap_width = pixmap.width() as i32;
    let pixmap_height = pixmap.height() as i32;

    for ch in text.chars() {
        let (metrics, bitmap) = font.rasterize(ch, raster_size);
        let glyph_x_high = (x * supersample_f).round() as i32 + metrics.xmin;
        let glyph_y_high = (baseline_y * supersample_f).round() as i32
            - metrics.ymin
            - metrics.height as i32;
        let glyph_right_high = glyph_x_high + metrics.width as i32;
        let glyph_bottom_high = glyph_y_high + metrics.height as i32;

        let pixel_left = floor_div(glyph_x_high, supersample);
        let pixel_top = floor_div(glyph_y_high, supersample);
        let pixel_right = ceil_div(glyph_right_high, supersample);
        let pixel_bottom = ceil_div(glyph_bottom_high, supersample);

        for py in pixel_top..pixel_bottom {
            if py < 0 || py >= pixmap_height {
                continue;
            }
            let sample_top = py * supersample;
            let sample_bottom = sample_top + supersample;

            for px in pixel_left..pixel_right {
                if px < 0 || px >= pixmap_width {
                    continue;
                }
                let sample_left = px * supersample;
                let sample_right = sample_left + supersample;
                let mut alpha_sum = 0u32;

                for high_y in sample_top..sample_bottom {
                    let local_y = high_y - glyph_y_high;
                    if local_y < 0 || local_y >= metrics.height as i32 {
                        continue;
                    }
                    for high_x in sample_left..sample_right {
                        let local_x = high_x - glyph_x_high;
                        if local_x < 0 || local_x >= metrics.width as i32 {
                            continue;
                        }
                        let index = local_y as usize * metrics.width + local_x as usize;
                        alpha_sum += bitmap[index] as u32;
                    }
                }

                let coverage =
                    ((alpha_sum + samples_per_pixel / 2) / samples_per_pixel) as u8;
                if coverage == 0 {
                    continue;
                }

                let pixel_index = py as usize * pixmap_width as usize + px as usize;
                let destination = pixmap.pixels()[pixel_index];
                pixmap.pixels_mut()[pixel_index] =
                    blend_coverage(destination, color, coverage);
            }
        }

        x += metrics.advance_width / supersample_f;
    }
}
