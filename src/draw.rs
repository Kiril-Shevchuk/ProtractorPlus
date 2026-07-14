use crate::text::{draw_text, layout_text};
use tiny_skia::{Color, FillRule, Paint, Path, PathBuilder, Pixmap, Stroke, Transform};

#[derive(Clone, Copy, Debug)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

pub const HANDLE_RADIUS: f32 = 10.5;
const LABEL_FONT_SIZE: f32 = 14.0;
const LABEL_PAD_X: f32 = 8.0;
const LABEL_PAD_Y: f32 = 6.0;
const LABEL_MIN_HEIGHT: f32 = 24.0;
const LABEL_RADIUS: f32 = 6.0;

#[derive(Clone, Copy, Debug)]
pub struct ContentBounds {
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
}

pub fn angle_between(a: Point, vertex: Point, b: Point) -> f32 {
    let ax = a.x - vertex.x;
    let ay = a.y - vertex.y;
    let bx = b.x - vertex.x;
    let by = b.y - vertex.y;
    (ax * by - ay * bx)
        .atan2(ax * bx + ay * by)
        .abs()
        .to_degrees()
}

fn label_center(a: Point, vertex: Point, b: Point) -> (f32, f32) {
    let ax = a.x - vertex.x;
    let ay = a.y - vertex.y;
    let bx = b.x - vertex.x;
    let by = b.y - vertex.y;
    let la = (ax * ax + ay * ay).sqrt().max(1.0);
    let lb = (bx * bx + by * by).sqrt().max(1.0);
    let mut dx = ax / la + bx / lb;
    let mut dy = ay / la + by / lb;
    let length = (dx * dx + dy * dy).sqrt();
    if length < 0.001 {
        dx = -ay / la;
        dy = ax / la;
    } else {
        dx /= length;
        dy /= length;
    }
    (vertex.x + dx * 34.0, vertex.y + dy * 34.0)
}

fn label_metrics(a: Point, vertex: Point, b: Point) -> (f32, f32, f32, f32) {
    let label = format!("{}°", angle_between(a, vertex, b).round() as i32);
    let layout = layout_text(&label, LABEL_FONT_SIZE);
    let (cx, cy) = label_center(a, vertex, b);
    let width = layout.width + 2.0 * LABEL_PAD_X;
    let height = (layout.height + 2.0 * LABEL_PAD_Y).max(LABEL_MIN_HEIGHT);
    (width, height, cx, cy)
}

pub fn content_bounds(points: [Point; 3]) -> ContentBounds {
    let a = points[0];
    let vertex = points[1];
    let b = points[2];
    let mut min_x = a.x.min(vertex.x).min(b.x) - HANDLE_RADIUS;
    let mut min_y = a.y.min(vertex.y).min(b.y) - HANDLE_RADIUS;
    let mut max_x = a.x.max(vertex.x).max(b.x) + HANDLE_RADIUS;
    let mut max_y = a.y.max(vertex.y).max(b.y) + HANDLE_RADIUS;
    let (width, height, cx, cy) = label_metrics(a, vertex, b);
    min_x = min_x.min(cx - width / 2.0);
    min_y = min_y.min(cy - height / 2.0);
    max_x = max_x.max(cx + width / 2.0);
    max_y = max_y.max(cy + height / 2.0);
    ContentBounds {
        min_x,
        min_y,
        max_x,
        max_y,
    }
}

pub fn rounded_rect_path(x: f32, y: f32, width: f32, height: f32, radius: f32) -> Option<Path> {
    if width <= 0.0 || height <= 0.0 {
        return None;
    }

    let radius = radius.max(0.0).min(width * 0.5).min(height * 0.5);
    if radius <= 0.0 {
        return tiny_skia::Rect::from_xywh(x, y, width, height).map(PathBuilder::from_rect);
    }

    // Standard cubic-Bezier approximation of a quarter circle.
    const KAPPA: f32 = 0.552_284_8;
    let control = radius * KAPPA;
    let right = x + width;
    let bottom = y + height;

    let mut builder = PathBuilder::new();
    builder.move_to(x + radius, y);
    builder.line_to(right - radius, y);
    builder.cubic_to(
        right - radius + control,
        y,
        right,
        y + radius - control,
        right,
        y + radius,
    );
    builder.line_to(right, bottom - radius);
    builder.cubic_to(
        right,
        bottom - radius + control,
        right - radius + control,
        bottom,
        right - radius,
        bottom,
    );
    builder.line_to(x + radius, bottom);
    builder.cubic_to(
        x + radius - control,
        bottom,
        x,
        bottom - radius + control,
        x,
        bottom - radius,
    );
    builder.line_to(x, y + radius);
    builder.cubic_to(
        x,
        y + radius - control,
        x + radius - control,
        y,
        x + radius,
        y,
    );
    builder.close();
    builder.finish()
}

pub fn fill_rounded_rect(
    pixmap: &mut Pixmap,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    radius: f32,
    color: Color,
) {
    let Some(path) = rounded_rect_path(x, y, width, height, radius) else {
        return;
    };
    let mut paint = Paint::default();
    paint.set_color(color);
    paint.anti_alias = true;
    pixmap.fill_path(
        &path,
        &paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );
}

fn stroke_line(pixmap: &mut Pixmap, from: Point, to: Point, width: f32, color: Color) {
    let mut builder = PathBuilder::new();
    builder.move_to(from.x, from.y);
    builder.line_to(to.x, to.y);
    if let Some(path) = builder.finish() {
        let mut paint = Paint::default();
        paint.set_color(color);
        paint.anti_alias = true;
        pixmap.stroke_path(
            &path,
            &paint,
            &Stroke {
                width,
                ..Default::default()
            },
            Transform::identity(),
            None,
        );
    }
}

fn draw_handle(pixmap: &mut Pixmap, center: Point, color: Color) {
    let mut outer = PathBuilder::new();
    outer.push_circle(center.x, center.y, HANDLE_RADIUS);
    let mut paint = Paint::default();
    paint.set_color(Color::from_rgba8(255, 255, 255, 215));
    paint.anti_alias = true;
    pixmap.fill_path(
        &outer.finish().unwrap(),
        &paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );

    let mut inner = PathBuilder::new();
    inner.push_circle(center.x, center.y, HANDLE_RADIUS - 3.0);
    paint.set_color(color);
    pixmap.fill_path(
        &inner.finish().unwrap(),
        &paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );

    let mut ring = PathBuilder::new();
    ring.push_circle(center.x, center.y, HANDLE_RADIUS);
    stroke_line_path(
        pixmap,
        &ring.finish().unwrap(),
        1.5,
        Color::from_rgba8(20, 20, 20, 210),
    );
}

fn stroke_line_path(pixmap: &mut Pixmap, path: &Path, width: f32, color: Color) {
    let mut paint = Paint::default();
    paint.set_color(color);
    paint.anti_alias = true;
    pixmap.stroke_path(
        path,
        &paint,
        &Stroke {
            width,
            ..Default::default()
        },
        Transform::identity(),
        None,
    );
}

pub fn render_angle_measure(width: u32, height: u32, points: [Point; 3]) -> Pixmap {
    let mut pixmap = Pixmap::new(width, height).expect("pixmap allocation");
    pixmap.fill(Color::TRANSPARENT);

    let a = points[0];
    let vertex = points[1];
    let b = points[2];
    let line = Color::from_rgba8(25, 25, 25, 220);
    stroke_line(&mut pixmap, vertex, a, 1.5, line);
    stroke_line(&mut pixmap, vertex, b, 1.5, line);
    draw_handle(&mut pixmap, a, Color::from_rgba8(255, 70, 70, 235));
    draw_handle(&mut pixmap, b, Color::from_rgba8(255, 70, 70, 235));
    draw_handle(
        &mut pixmap,
        vertex,
        Color::from_rgba8(70, 120, 255, 240),
    );

    let label = format!("{}°", angle_between(a, vertex, b).round() as i32);
    let layout = layout_text(&label, LABEL_FONT_SIZE);
    let (panel_width, panel_height, cx, cy) = label_metrics(a, vertex, b);
    fill_rounded_rect(
        &mut pixmap,
        cx - panel_width / 2.0,
        cy - panel_height / 2.0,
        panel_width,
        panel_height,
        LABEL_RADIUS,
        Color::from_rgba8(255, 255, 255, 155),
    );

    // Center the actual glyph bounds, not only the font advance box.
    let text_x = cx - (layout.xmin + layout.xmax) * 0.5;
    let baseline = cy + (layout.ymin + layout.ymax) * 0.5;
    draw_text(
        &mut pixmap,
        &label,
        text_x,
        baseline,
        LABEL_FONT_SIZE,
        Color::from_rgba8(20, 20, 20, 245),
    );

    pixmap
}
