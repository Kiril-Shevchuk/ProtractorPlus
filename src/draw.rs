use crate::text::{draw_text, layout_text};
use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Stroke, Transform};

#[derive(Clone, Copy, Debug)]
pub struct Point { pub x: f32, pub y: f32 }

pub const HANDLE_RADIUS: f32 = 10.5;
const LABEL_FONT_SIZE: f32 = 14.0;
const LABEL_PAD_X: f32 = 7.0;
const LABEL_PAD_Y: f32 = 5.0;

#[derive(Clone, Copy, Debug)]
pub struct ContentBounds { pub min_x: f32, pub min_y: f32, pub max_x: f32, pub max_y: f32 }

pub fn angle_between(a: Point, vertex: Point, b: Point) -> f32 {
    let ax = a.x - vertex.x;
    let ay = a.y - vertex.y;
    let bx = b.x - vertex.x;
    let by = b.y - vertex.y;
    (ax * by - ay * bx).atan2(ax * bx + ay * by).abs().to_degrees()
}

fn label_center(a: Point, vertex: Point, b: Point) -> (f32, f32) {
    let ax = a.x - vertex.x; let ay = a.y - vertex.y;
    let bx = b.x - vertex.x; let by = b.y - vertex.y;
    let la = (ax * ax + ay * ay).sqrt().max(1.0);
    let lb = (bx * bx + by * by).sqrt().max(1.0);
    let mut dx = ax / la + bx / lb;
    let mut dy = ay / la + by / lb;
    let l = (dx * dx + dy * dy).sqrt();
    if l < 0.001 { dx = -ay / la; dy = ax / la; } else { dx /= l; dy /= l; }
    (vertex.x + dx * 34.0, vertex.y + dy * 34.0)
}

fn label_metrics(a: Point, vertex: Point, b: Point) -> (f32, f32, f32, f32) {
    let label = format!("{}°", angle_between(a, vertex, b).round() as i32);
    let layout = layout_text(&label, LABEL_FONT_SIZE);
    let (cx, cy) = label_center(a, vertex, b);
    (layout.width + 2.0 * LABEL_PAD_X, layout.height + 2.0 * LABEL_PAD_Y, cx, cy)
}

pub fn content_bounds(points: [Point; 3]) -> ContentBounds {
    let a = points[0]; let v = points[1]; let b = points[2];
    let mut min_x = a.x.min(v.x).min(b.x) - HANDLE_RADIUS;
    let mut min_y = a.y.min(v.y).min(b.y) - HANDLE_RADIUS;
    let mut max_x = a.x.max(v.x).max(b.x) + HANDLE_RADIUS;
    let mut max_y = a.y.max(v.y).max(b.y) + HANDLE_RADIUS;
    let (w, h, cx, cy) = label_metrics(a, v, b);
    min_x = min_x.min(cx - w / 2.0); min_y = min_y.min(cy - h / 2.0);
    max_x = max_x.max(cx + w / 2.0); max_y = max_y.max(cy + h / 2.0);
    ContentBounds { min_x, min_y, max_x, max_y }
}

fn stroke_line(pixmap: &mut Pixmap, from: Point, to: Point, width: f32, color: Color) {
    let mut pb = PathBuilder::new(); pb.move_to(from.x, from.y); pb.line_to(to.x, to.y);
    if let Some(path) = pb.finish() {
        let mut paint = Paint::default(); paint.set_color(color); paint.anti_alias = true;
        pixmap.stroke_path(&path, &paint, &Stroke { width, ..Default::default() }, Transform::identity(), None);
    }
}

fn draw_handle(pixmap: &mut Pixmap, center: Point, color: Color) {
    let mut outer = PathBuilder::new(); outer.push_circle(center.x, center.y, HANDLE_RADIUS);
    let mut p = Paint::default(); p.set_color(Color::from_rgba8(255,255,255,215)); p.anti_alias = true;
    pixmap.fill_path(&outer.finish().unwrap(), &p, FillRule::Winding, Transform::identity(), None);
    let mut inner = PathBuilder::new(); inner.push_circle(center.x, center.y, HANDLE_RADIUS - 3.0);
    p.set_color(color); pixmap.fill_path(&inner.finish().unwrap(), &p, FillRule::Winding, Transform::identity(), None);
    let mut ring = PathBuilder::new(); ring.push_circle(center.x, center.y, HANDLE_RADIUS);
    stroke_line_path(pixmap, &ring.finish().unwrap(), 1.5, Color::from_rgba8(20,20,20,210));
}

fn stroke_line_path(pixmap: &mut Pixmap, path: &tiny_skia::Path, width: f32, color: Color) {
    let mut paint = Paint::default(); paint.set_color(color); paint.anti_alias = true;
    pixmap.stroke_path(path, &paint, &Stroke { width, ..Default::default() }, Transform::identity(), None);
}

pub fn render_angle_measure(width: u32, height: u32, points: [Point; 3]) -> Pixmap {
    let mut pixmap = Pixmap::new(width, height).expect("pixmap allocation");
    pixmap.fill(Color::TRANSPARENT);
    let a = points[0]; let v = points[1]; let b = points[2];
    let line = Color::from_rgba8(25,25,25,220);
    stroke_line(&mut pixmap, v, a, 1.5, line);
    stroke_line(&mut pixmap, v, b, 1.5, line);
    draw_handle(&mut pixmap, a, Color::from_rgba8(255,70,70,235));
    draw_handle(&mut pixmap, b, Color::from_rgba8(255,70,70,235));
    draw_handle(&mut pixmap, v, Color::from_rgba8(70,120,255,240));

    let label = format!("{}°", angle_between(a, v, b).round() as i32);
    let layout = layout_text(&label, LABEL_FONT_SIZE);
    let (w, h, cx, cy) = label_metrics(a, v, b);
    if let Some(rect) = tiny_skia::Rect::from_xywh(cx - w/2.0, cy - h/2.0, w, h) {
        let mut paint = Paint::default(); paint.set_color(Color::from_rgba8(255,255,255,205));
        pixmap.fill_rect(rect, &paint, Transform::identity(), None);
    }
    let baseline = cy - (layout.ymin + layout.ymax) / 2.0;
    draw_text(&mut pixmap, &label, cx - layout.width/2.0, baseline, LABEL_FONT_SIZE, Color::from_rgba8(20,20,20,245));
    pixmap
}
