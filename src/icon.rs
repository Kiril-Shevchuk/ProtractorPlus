use winit::window::Icon;

pub fn window_icon() -> Icon {
    const W: u32 = 32;
    const H: u32 = 32;
    let mut rgba = vec![0u8; (W * H * 4) as usize];
    for y in 0..H {
        for x in 0..W {
            let i = ((y * W + x) * 4) as usize;
            let dx = x as f32 - 16.0;
            let dy = y as f32 - 16.0;
            if dx * dx + dy * dy <= 13.0 * 13.0 {
                rgba[i] = 70; rgba[i+1] = 120; rgba[i+2] = 255; rgba[i+3] = 255;
            }
            if (y >= 14 && y <= 17 && x >= 5 && x <= 27) ||
               ((x as i32 - y as i32).abs() <= 1 && x >= 15 && x <= 27 && y >= 5 && y <= 17) {
                rgba[i] = 255; rgba[i+1] = 255; rgba[i+2] = 255; rgba[i+3] = 255;
            }
        }
    }
    Icon::from_rgba(rgba, W, H).expect("valid icon")
}
