use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/ProtractorPlus.ico");

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is not set"));
    let font_path = out_dir.join("comfortaa-variable.ttf");

    if !font_path.exists() {
        let font_data = google_fonts::comfortaa_variable()
            .expect("failed to download Comfortaa from Google Fonts during build");
        fs::write(&font_path, font_data).expect("failed to write Comfortaa to OUT_DIR");
    }

    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let manifest_dir =
            PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is not set"));
        let icon_path = manifest_dir.join("assets").join("ProtractorPlus.ico");
        let icon_path = icon_path
            .to_str()
            .expect("ProtractorPlus icon path is not valid UTF-8");

        let mut resource = winresource::WindowsResource::new();
        resource
            .set_icon(icon_path)
            .set("FileDescription", "ProtractorPlus")
            .set("ProductName", "ProtractorPlus")
            .set("CompanyName", "Kiril Shevchuk")
            .set("LegalCopyright", "Copyright (c) 2026 Kiril Shevchuk")
            .set("FileVersion", "2.7.2.0")
            .set("ProductVersion", "2.7.2")
            .set("InternalName", "ProtractorPlus.exe")
            .set("OriginalFilename", "ProtractorPlus.exe");

        resource
            .compile()
            .expect("failed to embed ProtractorPlus icon into the Windows executable");
    }
}
