use crate::text::{draw_text, layout_text};
use tiny_skia::{Color, Pixmap, PremultipliedColorU8};

const SOURCE_WIDTH: usize = 512;
const SOURCE_HEIGHT: usize = 512;
const SOURCE_RGBA: &[u8] = include_bytes!("../assets/splash_icon.rgba");
const SIGNATURE: &str = "Cb prod.";

pub fn render_splash(width: u32, height: u32) -> Pixmap {
    let mut pixmap = Pixmap::new(width.max(1), height.max(1)).expect("splash pixmap allocation");
    pixmap.fill(Color::TRANSPARENT);

    draw_scaled_icon(&mut pixmap);
    draw_signature(&mut pixmap);
    pixmap
}

fn draw_scaled_icon(pixmap: &mut Pixmap) {
    debug_assert_eq!(SOURCE_RGBA.len(), SOURCE_WIDTH * SOURCE_HEIGHT * 4);

    let width = pixmap.width() as usize;
    let height = pixmap.height() as usize;
    if width == 0 || height == 0 {
        return;
    }

    for y in 0..height {
        let source_y = ((y as f32 + 0.5) * SOURCE_HEIGHT as f32 / height as f32 - 0.5)
            .clamp(0.0, (SOURCE_HEIGHT - 1) as f32);
        let y0 = source_y.floor() as usize;
        let y1 = (y0 + 1).min(SOURCE_HEIGHT - 1);
        let fy = source_y - y0 as f32;

        for x in 0..width {
            let source_x = ((x as f32 + 0.5) * SOURCE_WIDTH as f32 / width as f32 - 0.5)
                .clamp(0.0, (SOURCE_WIDTH - 1) as f32);
            let x0 = source_x.floor() as usize;
            let x1 = (x0 + 1).min(SOURCE_WIDTH - 1);
            let fx = source_x - x0 as f32;

            let c00 = source_pixel(x0, y0);
            let c10 = source_pixel(x1, y0);
            let c01 = source_pixel(x0, y1);
            let c11 = source_pixel(x1, y1);

            let mut rgba = [0u8; 4];
            for channel in 0..4 {
                let top = c00[channel] as f32 * (1.0 - fx) + c10[channel] as f32 * fx;
                let bottom = c01[channel] as f32 * (1.0 - fx) + c11[channel] as f32 * fx;
                rgba[channel] = (top * (1.0 - fy) + bottom * fy).round() as u8;
            }

            let alpha = rgba[3];
            let premultiplied = PremultipliedColorU8::from_rgba(
                ((rgba[0] as u16 * alpha as u16 + 127) / 255) as u8,
                ((rgba[1] as u16 * alpha as u16 + 127) / 255) as u8,
                ((rgba[2] as u16 * alpha as u16 + 127) / 255) as u8,
                alpha,
            )
            .expect("valid splash pixel");

            pixmap.pixels_mut()[y * width + x] = premultiplied;
        }
    }
}

fn source_pixel(x: usize, y: usize) -> [u8; 4] {
    let index = (y * SOURCE_WIDTH + x) * 4;
    [
        SOURCE_RGBA[index],
        SOURCE_RGBA[index + 1],
        SOURCE_RGBA[index + 2],
        SOURCE_RGBA[index + 3],
    ]
}

fn draw_signature(pixmap: &mut Pixmap) {
    let side = pixmap.width().min(pixmap.height()) as f32;
    let font_size = (side * 0.040).clamp(9.0, 18.0);
    let margin_x = side * 0.075;
    let margin_y = side * 0.070;
    let layout = layout_text(SIGNATURE, font_size);

    let visual_right = pixmap.width() as f32 - margin_x;
    let visual_bottom = pixmap.height() as f32 - margin_y;
    let text_x = visual_right - layout.xmax;
    let baseline_y = visual_bottom + layout.ymin;

    draw_text(
        pixmap,
        SIGNATURE,
        text_x,
        baseline_y,
        font_size,
        Color::from_rgba8(255, 255, 255, 232),
    );
}
