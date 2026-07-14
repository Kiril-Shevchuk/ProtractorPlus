use winit::window::Icon;

pub fn window_icon() -> Icon {
    const WIDTH: u32 = 64;
    const HEIGHT: u32 = 64;
    let rgba = include_bytes!("../assets/window_icon.rgba").to_vec();

    Icon::from_rgba(rgba, WIDTH, HEIGHT).expect("valid embedded ProtractorPlus icon")
}
