# Changelog

## 1.3.1

- rendered the Comfortaa degree label internally at 4x resolution;
- downsampled glyph coverage for smoother edges at the final display size;
- corrected alpha compositing so anti-aliased text blends cleanly with the semi-transparent panel;
- optimized the loaded font outlines for the supersampled raster size.

## 1.3.0

- embedded the Comfortaa variable font for the degree label;
- increased the degree-label rasterization size for improved readability;
- reduced the lock panel and lock icon by 50%;
- added 1-degree mouse-wheel adjustment while hovering over the degree panel;
- preserved the angle bisector and equal ray lengths during wheel adjustment.

## 1.2.0

- centered the degree text horizontally and vertically;
- made the degree panel semi-transparent with rounded corners;
- made the lock panel semi-transparent with rounded corners;
- attached the lock position to the ray opposite the angle bisector;
- synchronized the lengths of both rays;
- upgraded GitHub Actions to Node.js 24-compatible versions.

## 1.1.2

- fixed Windows API type inference for `PostMessageW`.
