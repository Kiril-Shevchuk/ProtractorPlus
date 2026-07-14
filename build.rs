use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is not set"));
    let font_path = out_dir.join("comfortaa-variable.ttf");

    if font_path.exists() {
        return;
    }

    let font_data = google_fonts::comfortaa_variable()
        .expect("failed to download Comfortaa from Google Fonts during build");
    fs::write(&font_path, font_data).expect("failed to write Comfortaa to OUT_DIR");
}
