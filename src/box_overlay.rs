use std::fs;
use std::path::Path;

use crate::distance::format_distance;
use crate::draw::{draw_text_panel, ContentBounds, PanelRect, Point};
use crate::text::{draw_text, layout_text};
use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Stroke, Transform};

pub const MAX_BOX_POINTS: usize = 10;
const MARKER_RADIUS: f32 = 7.0;
const MARKER_HIT_RADIUS: f32 = 12.0;
const NUMBER_FONT_SIZE: f32 = 10.5;
const SEGMENT_WIDTH: f32 = 1.3;
const DASH_LENGTH: f32 = 6.0;
const DASH_GAP: f32 = 4.5;
const PANEL_OFFSET: f32 = 18.0;
const PANEL_GAP: f32 = 5.0;
const PANEL_SHIFT_STEP: f32 = 7.0;
const EPSILON: f32 = 0.0001;

fn magenta() -> Color {
    Color::from_rgba8(214, 32, 168, 235)
}

fn magenta_dark() -> Color {
    Color::from_rgba8(126, 15, 94, 252)
}

fn magenta_panel() -> Color {
    Color::from_rgba8(255, 220, 247, 190)
}

#[derive(Clone, Debug, Default)]
pub struct BoxSavedState {
    pub visible: bool,
    pub closed: bool,
    pub points: Vec<Point>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoxPanelKind {
    Distance(usize),
    Bearing(usize),
}

#[derive(Clone, Debug)]
pub struct BoxPanel {
    pub kind: BoxPanelKind,
    pub rect: PanelRect,
    pub text: String,
}

#[derive(Clone, Debug, Default)]
pub struct BoxPanelLayout {
    pub panels: Vec<BoxPanel>,
}

impl BoxPanelLayout {
    pub fn rects(&self) -> Vec<PanelRect> {
        self.panels.iter().map(|panel| panel.rect).collect()
    }

    pub fn hit_test(&self, point: Point) -> Option<BoxPanelKind> {
        self.panels
            .iter()
            .find(|panel| point_in_rect(point, panel.rect))
            .map(|panel| panel.kind)
    }
}

pub fn load_box_state(path: &Path) -> BoxSavedState {
    let Ok(text) = fs::read_to_string(path) else {
        return BoxSavedState::default();
    };
    let mut values = text.split_whitespace();
    let version = values.next().and_then(|value| value.parse::<u32>().ok());
    if version != Some(1) {
        return BoxSavedState::default();
    }
    let visible = values
        .next()
        .and_then(|value| value.parse::<u8>().ok())
        .map(|value| value != 0)
        .unwrap_or(false);
    let closed = values
        .next()
        .and_then(|value| value.parse::<u8>().ok())
        .map(|value| value != 0)
        .unwrap_or(false);
    let count = values
        .next()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0)
        .min(MAX_BOX_POINTS);
    let mut points = Vec::with_capacity(count);
    for _ in 0..count {
        let Some(x) = values.next().and_then(|value| value.parse::<f32>().ok()) else {
            return BoxSavedState::default();
        };
        let Some(y) = values.next().and_then(|value| value.parse::<f32>().ok()) else {
            return BoxSavedState::default();
        };
        if x.is_finite() && y.is_finite() {
            points.push(Point { x, y });
        }
    }
    BoxSavedState {
        visible,
        closed: closed && points.len() >= 3,
        points,
    }
}

pub fn save_box_state(path: &Path, visible: bool, closed: bool, points: &[Point]) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let mut lines = vec![
        "1".to_string(),
        u8::from(visible).to_string(),
        u8::from(closed && points.len() >= 3).to_string(),
        points.len().min(MAX_BOX_POINTS).to_string(),
    ];
    for point in points.iter().take(MAX_BOX_POINTS) {
        lines.push(format!("{} {}", point.x, point.y));
    }
    let _ = fs::write(path, format!("{}\n", lines.join("\n")));
}

pub fn box_point_at(point: Point, points: &[Point]) -> Option<usize> {
    points.iter().enumerate().find_map(|(index, candidate)| {
        let dx = candidate.x - point.x;
        let dy = candidate.y - point.y;
        (dx * dx + dy * dy <= MARKER_HIT_RADIUS * MARKER_HIT_RADIUS).then_some(index)
    })
}

pub fn is_close_target(point: Point, points: &[Point]) -> bool {
    points.len() >= 3 && box_point_at(point, points) == Some(0)
}

pub fn segment_count(points: &[Point], closed: bool) -> usize {
    if points.len() < 2 {
        0
    } else if closed && points.len() >= 3 {
        points.len()
    } else {
        points.len() - 1
    }
}

fn segment(points: &[Point], closed: bool, index: usize) -> Option<(Point, Point)> {
    let count = segment_count(points, closed);
    if index >= count {
        return None;
    }
    let from = points[index];
    let to = if index + 1 < points.len() {
        points[index + 1]
    } else {
        points[0]
    };
    Some((from, to))
}

fn length(from: Point, to: Point) -> f32 {
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    (dx * dx + dy * dy).sqrt()
}

fn midpoint(from: Point, to: Point) -> Point {
    Point {
        x: (from.x + to.x) * 0.5,
        y: (from.y + to.y) * 0.5,
    }
}

fn normalize_bearing(angle: f32) -> f32 {
    let mut value = angle;
    while value < 0.0 {
        value += std::f32::consts::TAU;
    }
    while value >= std::f32::consts::TAU {
        value -= std::f32::consts::TAU;
    }
    value
}

fn bearing_degrees(from: Point, to: Point, north_angle: f32) -> i32 {
    let segment_angle = (to.y - from.y).atan2(to.x - from.x);
    let degrees = normalize_bearing(segment_angle - north_angle).to_degrees().round() as i32;
    degrees.rem_euclid(360)
}

fn stroke_dashed_segment(pixmap: &mut Pixmap, from: Point, to: Point) {
    let total = length(from, to);
    if total < EPSILON {
        return;
    }
    let angle = (to.y - from.y).atan2(to.x - from.x);
    let ux = angle.cos();
    let uy = angle.sin();
    let mut distance = MARKER_RADIUS * 0.8;
    let end_limit = (total - MARKER_RADIUS * 0.8).max(distance);
    while distance < end_limit {
        let dash_end = (distance + DASH_LENGTH).min(end_limit);
        let mut builder = PathBuilder::new();
        builder.move_to(from.x + ux * distance, from.y + uy * distance);
        builder.line_to(from.x + ux * dash_end, from.y + uy * dash_end);
        if let Some(path) = builder.finish() {
            let mut paint = Paint::default();
            paint.set_color(magenta());
            paint.anti_alias = true;
            pixmap.stroke_path(
                &path,
                &paint,
                &Stroke {
                    width: SEGMENT_WIDTH,
                    ..Stroke::default()
                },
                Transform::identity(),
                None,
            );
        }
        distance += DASH_LENGTH + DASH_GAP;
    }
}

fn draw_triangle_marker(pixmap: &mut Pixmap, center: Point, number: usize) {
    let mut builder = PathBuilder::new();
    builder.move_to(center.x, center.y - MARKER_RADIUS);
    builder.line_to(center.x - MARKER_RADIUS * 0.88, center.y + MARKER_RADIUS * 0.62);
    builder.line_to(center.x + MARKER_RADIUS * 0.88, center.y + MARKER_RADIUS * 0.62);
    builder.close();
    if let Some(path) = builder.finish() {
        let mut paint = Paint::default();
        paint.set_color(magenta());
        paint.anti_alias = true;
        pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
    }

    let text = number.to_string();
    let layout = layout_text(&text, NUMBER_FONT_SIZE);
    let text_center = Point {
        x: center.x + MARKER_RADIUS + 5.5,
        y: center.y - MARKER_RADIUS * 0.55,
    };
    let x = text_center.x - (layout.xmin + layout.xmax) * 0.5;
    let baseline = text_center.y + (layout.ymin + layout.ymax) * 0.5;
    draw_text(
        pixmap,
        &text,
        x,
        baseline,
        NUMBER_FONT_SIZE,
        magenta_dark(),
    );
}

pub fn draw_box_geometry(pixmap: &mut Pixmap, points: &[Point], closed: bool) {
    for index in 0..segment_count(points, closed) {
        if let Some((from, to)) = segment(points, closed, index) {
            stroke_dashed_segment(pixmap, from, to);
        }
    }
    for (index, point) in points.iter().enumerate() {
        draw_triangle_marker(pixmap, *point, index + 1);
    }
}

fn point_in_rect(point: Point, rect: PanelRect) -> bool {
    point.x >= rect.x
        && point.x <= rect.x + rect.width
        && point.y >= rect.y
        && point.y <= rect.y + rect.height
}

fn rects_overlap(a: PanelRect, b: PanelRect, gap: f32) -> bool {
    a.x < b.x + b.width + gap
        && a.x + a.width + gap > b.x
        && a.y < b.y + b.height + gap
        && a.y + a.height + gap > b.y
}

fn translate_rect(rect: PanelRect, dx: f32, dy: f32) -> PanelRect {
    PanelRect {
        x: rect.x + dx,
        y: rect.y + dy,
        ..rect
    }
}

fn resolve_panel(preferred: PanelRect, occupied: &[PanelRect]) -> PanelRect {
    if occupied
        .iter()
        .all(|other| !rects_overlap(preferred, *other, PANEL_GAP))
    {
        return preferred;
    }
    for ring in 1..=22 {
        let d = ring as f32 * PANEL_SHIFT_STEP;
        let candidates = [
            (0.0, -d),
            (0.0, d),
            (-d, 0.0),
            (d, 0.0),
            (-d, -d),
            (d, -d),
            (-d, d),
            (d, d),
            (-d * 1.5, 0.0),
            (d * 1.5, 0.0),
        ];
        for (dx, dy) in candidates {
            let candidate = translate_rect(preferred, dx, dy);
            if occupied
                .iter()
                .all(|other| !rects_overlap(candidate, *other, PANEL_GAP))
            {
                return candidate;
            }
        }
    }
    translate_rect(preferred, 0.0, 23.0 * PANEL_SHIFT_STEP)
}

fn preferred_panel_center(from: Point, to: Point, side: f32) -> Point {
    let mid = midpoint(from, to);
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    let len = (dx * dx + dy * dy).sqrt();
    if len < EPSILON {
        return mid;
    }
    Point {
        x: mid.x + (-dy / len) * PANEL_OFFSET * side,
        y: mid.y + (dx / len) * PANEL_OFFSET * side,
    }
}

fn panel_rect(text: &str, center: Point) -> PanelRect {
    crate::draw::text_panel_rect(text, center.x, center.y)
}

pub fn layout_box_panels(
    points: &[Point],
    closed: bool,
    meters_per_pixel: f32,
    north_angle: f32,
    show_distance: bool,
    show_bearing: bool,
    hidden_distance_mask: u16,
    hidden_bearing_mask: u16,
    avoid_rects: &[PanelRect],
) -> BoxPanelLayout {
    let mut occupied = avoid_rects.to_vec();
    let mut panels = Vec::new();
    let count = segment_count(points, closed);

    for index in 0..count {
        let Some((from, to)) = segment(points, closed, index) else {
            continue;
        };
        if show_distance
            && meters_per_pixel > 0.0
            && hidden_distance_mask & (1u16 << index) == 0
        {
            let text = format_distance(length(from, to) * meters_per_pixel);
            let center = preferred_panel_center(from, to, -1.0);
            let rect = resolve_panel(panel_rect(&text, center), &occupied);
            occupied.push(rect);
            panels.push(BoxPanel {
                kind: BoxPanelKind::Distance(index),
                rect,
                text,
            });
        }

        if show_bearing && hidden_bearing_mask & (1u16 << index) == 0 {
            let text = format!("{}°", bearing_degrees(from, to, north_angle));
            let center = preferred_panel_center(from, to, 1.0);
            let rect = resolve_panel(panel_rect(&text, center), &occupied);
            occupied.push(rect);
            panels.push(BoxPanel {
                kind: BoxPanelKind::Bearing(index),
                rect,
                text,
            });
        }
    }

    BoxPanelLayout { panels }
}

pub fn draw_box_panels(pixmap: &mut Pixmap, layout: &BoxPanelLayout) {
    for panel in &layout.panels {
        let center = Point {
            x: panel.rect.x + panel.rect.width * 0.5,
            y: panel.rect.y + panel.rect.height * 0.5,
        };
        draw_text_panel(
            pixmap,
            &panel.text,
            center.x,
            center.y,
            magenta_panel(),
            magenta_dark(),
        );
    }
}

pub fn box_bounds(points: &[Point], layout: &BoxPanelLayout) -> Option<ContentBounds> {
    let first = *points.first()?;
    let mut bounds = ContentBounds {
        min_x: first.x - 20.0,
        min_y: first.y - 20.0,
        max_x: first.x + 28.0,
        max_y: first.y + 20.0,
    };
    for point in points.iter().skip(1) {
        bounds.min_x = bounds.min_x.min(point.x - 20.0);
        bounds.min_y = bounds.min_y.min(point.y - 20.0);
        bounds.max_x = bounds.max_x.max(point.x + 28.0);
        bounds.max_y = bounds.max_y.max(point.y + 20.0);
    }
    for rect in layout.rects() {
        bounds.min_x = bounds.min_x.min(rect.x);
        bounds.min_y = bounds.min_y.min(rect.y);
        bounds.max_x = bounds.max_x.max(rect.x + rect.width);
        bounds.max_y = bounds.max_y.max(rect.y + rect.height);
    }
    Some(bounds)
}
