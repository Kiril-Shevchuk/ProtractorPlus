use std::fs;
use std::path::Path;

use crate::distance::format_distance;
use crate::draw::{fill_rounded_rect, ContentBounds, PanelRect, Point, LABEL_FONT_SIZE};
use crate::text::{draw_text, layout_text};
use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Rect, Stroke, Transform};

pub const MAX_BOX_POINTS: usize = 10;
const MARKER_RADIUS: f32 = 7.0;
const MARKER_HIT_RADIUS: f32 = 12.0;
const NUMBER_FONT_SIZE: f32 = 10.5;
const SEGMENT_WIDTH: f32 = 1.3;
const DASH_LENGTH: f32 = 6.0;
const DASH_GAP: f32 = 4.5;
const PANEL_OFFSET: f32 = 15.0;
const PANEL_PAIR_GAP: f32 = 3.0;
const PANEL_COLLISION_GAP: f32 = 4.0;
const PANEL_SHIFT_STEP: f32 = 6.0;
const BOX_PANEL_FONT_SIZE: f32 = LABEL_FONT_SIZE * 0.5;
const BOX_PANEL_PAD_X: f32 = 3.4;
const BOX_PANEL_PAD_Y: f32 = 2.15;
const BOX_PANEL_MIN_HEIGHT: f32 = 10.7;
const BOX_PANEL_RADIUS: f32 = 3.0;
const LOCK_SIZE: f32 = 10.0;
const LOCK_RADIUS: f32 = 2.5;
const LOCK_OFFSET_X: f32 = -13.0;
const LOCK_OFFSET_Y: f32 = -12.0;
const LOCK_HIT_PAD: f32 = 3.0;
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

fn magenta_lock_panel(locked: bool) -> Color {
    if locked {
        Color::from_rgba8(224, 55, 180, 220)
    } else {
        Color::from_rgba8(255, 220, 247, 190)
    }
}

#[derive(Clone, Debug, Default)]
pub struct BoxSavedState {
    pub visible: bool,
    pub closed: bool,
    pub points: Vec<Point>,
    pub locked: Vec<bool>,
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
    if version != Some(1) && version != Some(2) {
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

    let mut locked = vec![false; points.len()];
    if version == Some(2) {
        for item in locked.iter_mut() {
            *item = values
                .next()
                .and_then(|value| value.parse::<u8>().ok())
                .map(|value| value != 0)
                .unwrap_or(false);
        }
    }

    BoxSavedState {
        visible,
        closed: closed && points.len() >= 3,
        points,
        locked,
    }
}

pub fn save_box_state(
    path: &Path,
    visible: bool,
    closed: bool,
    points: &[Point],
    locked: &[bool],
) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let count = points.len().min(MAX_BOX_POINTS);
    let mut lines = vec![
        "2".to_string(),
        u8::from(visible).to_string(),
        u8::from(closed && count >= 3).to_string(),
        count.to_string(),
    ];
    for point in points.iter().take(count) {
        lines.push(format!("{} {}", point.x, point.y));
    }
    for index in 0..count {
        lines.push(u8::from(locked.get(index).copied().unwrap_or(false)).to_string());
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

fn lock_center(point: Point) -> Point {
    Point {
        x: point.x + LOCK_OFFSET_X,
        y: point.y + LOCK_OFFSET_Y,
    }
}

pub fn box_lock_at(point: Point, points: &[Point]) -> Option<usize> {
    let half = LOCK_SIZE * 0.5 + LOCK_HIT_PAD;
    points.iter().enumerate().find_map(|(index, candidate)| {
        let center = lock_center(*candidate);
        (point.x >= center.x - half
            && point.x <= center.x + half
            && point.y >= center.y - half
            && point.y <= center.y + half)
            .then_some(index)
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

fn draw_lock_icon(pixmap: &mut Pixmap, point: Point, locked: bool) {
    let center = lock_center(point);
    fill_rounded_rect(
        pixmap,
        center.x - LOCK_SIZE * 0.5,
        center.y - LOCK_SIZE * 0.5,
        LOCK_SIZE,
        LOCK_SIZE,
        LOCK_RADIUS,
        magenta_lock_panel(locked),
    );

    let icon_color = if locked {
        Color::from_rgba8(255, 255, 255, 250)
    } else {
        magenta_dark()
    };
    if let Some(body) = Rect::from_xywh(center.x - 2.1, center.y - 0.1, 4.2, 3.5) {
        let mut paint = Paint::default();
        paint.set_color(icon_color);
        paint.anti_alias = true;
        pixmap.fill_rect(body, &paint, Transform::identity(), None);
    }

    let right_x = if locked { center.x + 1.8 } else { center.x + 3.0 };
    let mut builder = PathBuilder::new();
    builder.move_to(center.x - 1.8, center.y - 0.1);
    builder.line_to(center.x - 1.8, center.y - 1.7);
    builder.cubic_to(
        center.x - 1.8,
        center.y - 4.2,
        right_x,
        center.y - 4.2,
        right_x,
        center.y - 1.7,
    );
    if locked {
        builder.line_to(right_x, center.y - 0.1);
    }
    if let Some(path) = builder.finish() {
        let mut paint = Paint::default();
        paint.set_color(icon_color);
        paint.anti_alias = true;
        pixmap.stroke_path(
            &path,
            &paint,
            &Stroke {
                width: 1.1,
                ..Stroke::default()
            },
            Transform::identity(),
            None,
        );
    }
}

pub fn draw_box_geometry(pixmap: &mut Pixmap, points: &[Point], closed: bool, locked: &[bool]) {
    for index in 0..segment_count(points, closed) {
        if let Some((from, to)) = segment(points, closed, index) {
            stroke_dashed_segment(pixmap, from, to);
        }
    }
    for (index, point) in points.iter().enumerate() {
        draw_triangle_marker(pixmap, *point, index + 1);
        draw_lock_icon(
            pixmap,
            *point,
            locked.get(index).copied().unwrap_or(false),
        );
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

fn group_clear(group: &[PanelRect], occupied: &[PanelRect]) -> bool {
    group.iter().all(|rect| {
        occupied
            .iter()
            .all(|other| !rects_overlap(*rect, *other, PANEL_COLLISION_GAP))
    })
}

fn resolve_group(preferred: &[PanelRect], occupied: &[PanelRect]) -> Vec<PanelRect> {
    if group_clear(preferred, occupied) {
        return preferred.to_vec();
    }
    for ring in 1..=24 {
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
            let candidate: Vec<_> = preferred
                .iter()
                .copied()
                .map(|rect| translate_rect(rect, dx, dy))
                .collect();
            if group_clear(&candidate, occupied) {
                return candidate;
            }
        }
    }
    preferred
        .iter()
        .copied()
        .map(|rect| translate_rect(rect, 0.0, 25.0 * PANEL_SHIFT_STEP))
        .collect()
}

fn compact_panel_rect(text: &str, center: Point) -> PanelRect {
    let layout = layout_text(text, BOX_PANEL_FONT_SIZE);
    let width = layout.width + 2.0 * BOX_PANEL_PAD_X;
    let height = (layout.height + 2.0 * BOX_PANEL_PAD_Y).max(BOX_PANEL_MIN_HEIGHT);
    PanelRect {
        x: center.x - width * 0.5,
        y: center.y - height * 0.5,
        width,
        height,
    }
}

fn segment_basis(from: Point, to: Point) -> (Point, f32, f32, f32, f32) {
    let mid = midpoint(from, to);
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    let len = (dx * dx + dy * dy).sqrt().max(EPSILON);
    let ux = dx / len;
    let uy = dy / len;
    let nx = -uy;
    let ny = ux;
    (mid, ux, uy, nx, ny)
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
        let distance_text = (show_distance
            && meters_per_pixel > 0.0
            && hidden_distance_mask & (1u16 << index) == 0)
            .then(|| format_distance(length(from, to) * meters_per_pixel));
        let bearing_text = (show_bearing && hidden_bearing_mask & (1u16 << index) == 0)
            .then(|| format!("{}°", bearing_degrees(from, to, north_angle)));
        if distance_text.is_none() && bearing_text.is_none() {
            continue;
        }

        let (mid, _ux, _uy, nx, ny) = segment_basis(from, to);
        let base = Point {
            x: mid.x - nx * PANEL_OFFSET,
            y: mid.y - ny * PANEL_OFFSET,
        };

        let mut preferred = Vec::new();
        let mut kinds = Vec::new();
        let mut texts = Vec::new();

        match (distance_text, bearing_text) {
            (Some(distance), Some(bearing)) => {
                let distance_size = compact_panel_rect(&distance, Point { x: 0.0, y: 0.0 });
                let bearing_size = compact_panel_rect(&bearing, Point { x: 0.0, y: 0.0 });
                let required = distance_size.width + bearing_size.width + PANEL_PAIR_GAP + 16.0;
                if length(from, to) >= required {
                    let distance_center = Point {
                        x: base.x - (bearing_size.width * 0.5 + PANEL_PAIR_GAP * 0.5),
                        y: base.y,
                    };
                    let bearing_center = Point {
                        x: base.x + (distance_size.width * 0.5 + PANEL_PAIR_GAP * 0.5),
                        y: base.y,
                    };
                    preferred.push(compact_panel_rect(&distance, distance_center));
                    preferred.push(compact_panel_rect(&bearing, bearing_center));
                } else {
                    let total_height = distance_size.height + bearing_size.height + PANEL_PAIR_GAP;
                    let distance_center = Point {
                        x: base.x,
                        y: base.y - total_height * 0.5 + distance_size.height * 0.5,
                    };
                    let bearing_center = Point {
                        x: base.x,
                        y: base.y + total_height * 0.5 - bearing_size.height * 0.5,
                    };
                    preferred.push(compact_panel_rect(&distance, distance_center));
                    preferred.push(compact_panel_rect(&bearing, bearing_center));
                }
                kinds.push(BoxPanelKind::Distance(index));
                kinds.push(BoxPanelKind::Bearing(index));
                texts.push(distance);
                texts.push(bearing);
            }
            (Some(distance), None) => {
                preferred.push(compact_panel_rect(&distance, base));
                kinds.push(BoxPanelKind::Distance(index));
                texts.push(distance);
            }
            (None, Some(bearing)) => {
                preferred.push(compact_panel_rect(&bearing, base));
                kinds.push(BoxPanelKind::Bearing(index));
                texts.push(bearing);
            }
            (None, None) => {}
        }

        let resolved = resolve_group(&preferred, &occupied);
        occupied.extend(resolved.iter().copied());
        for ((kind, text), rect) in kinds.into_iter().zip(texts).zip(resolved) {
            panels.push(BoxPanel { kind, rect, text });
        }
    }

    BoxPanelLayout { panels }
}

fn draw_compact_panel(pixmap: &mut Pixmap, panel: &BoxPanel) {
    fill_rounded_rect(
        pixmap,
        panel.rect.x,
        panel.rect.y,
        panel.rect.width,
        panel.rect.height,
        BOX_PANEL_RADIUS,
        magenta_panel(),
    );
    let layout = layout_text(&panel.text, BOX_PANEL_FONT_SIZE);
    let cx = panel.rect.x + panel.rect.width * 0.5;
    let cy = panel.rect.y + panel.rect.height * 0.5;
    let x = cx - (layout.xmin + layout.xmax) * 0.5;
    let baseline = cy + (layout.ymin + layout.ymax) * 0.5;
    draw_text(
        pixmap,
        &panel.text,
        x,
        baseline,
        BOX_PANEL_FONT_SIZE,
        magenta_dark(),
    );
}

pub fn draw_box_panels(pixmap: &mut Pixmap, layout: &BoxPanelLayout) {
    for panel in &layout.panels {
        draw_compact_panel(pixmap, panel);
    }
}

pub fn box_bounds(points: &[Point], layout: &BoxPanelLayout) -> Option<ContentBounds> {
    let first = *points.first()?;
    let mut bounds = ContentBounds {
        min_x: first.x - 28.0,
        min_y: first.y - 28.0,
        max_x: first.x + 28.0,
        max_y: first.y + 24.0,
    };
    for point in points.iter().skip(1) {
        bounds.min_x = bounds.min_x.min(point.x - 28.0);
        bounds.min_y = bounds.min_y.min(point.y - 28.0);
        bounds.max_x = bounds.max_x.max(point.x + 28.0);
        bounds.max_y = bounds.max_y.max(point.y + 24.0);
    }
    for rect in layout.rects() {
        bounds.min_x = bounds.min_x.min(rect.x);
        bounds.min_y = bounds.min_y.min(rect.y);
        bounds.max_x = bounds.max_x.max(rect.x + rect.width);
        bounds.max_y = bounds.max_y.max(rect.y + rect.height);
    }
    Some(bounds)
}
