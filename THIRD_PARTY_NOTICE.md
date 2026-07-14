# Third-party notice

ProtractorPlus is based on the GPL-3.0 project `drfailov/MyProtractor`.
The modified source code is distributed under GPL-3.0-only.

Rust dependencies are resolved by Cargo from `Cargo.toml` during the build.

## Comfortaa

The degree label uses the Comfortaa typeface by Johan Aakerlund. Font data is obtained during the build through the `google-fonts` Rust crate and embedded into the executable.

## Windows resource embedding

The Windows executable icon and version metadata are embedded at build time with the `winresource` Rust crate.
