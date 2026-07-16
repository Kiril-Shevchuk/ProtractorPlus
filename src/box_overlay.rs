use std::fs;
use std::path::Path;

use crate::distance::format_distance;
use crate::draw::{fill_rounded_rect, ContentBounds, PanelRect, Point, LABEL_FONT_SIZE};
use crate::text::{draw_text, layout_text};
use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Rect, Stroke, Transform};

pub const MAX_BOX_POINTS: usize = 10;
pub const MAX_BOX_CORRIDORS: usize = 10;
const MARKER_RADIUS: f32 = 7.0;
const MARKER_HIT_RADIUS: f32 = 12.0;
const CORRIDOR_ANCHOR_RADIUS: f32 = 4.2;
const CORRIDOR_ANCHOR_HIT_RADIUS: f32 = 10.0;
const SEGMENT_HIT_RADIUS: f32 = 8.0;
const NUMBER_FONT_SIZE: f32 = 10.5;
const SEGMENT_WIDTH: f32 = 1.3;
const DASH_LENGTH: f32 = 6.0;
const DASH_GAP: f32 = 4.5;
const PANEL_OFFSET: f32 = 15.0;
const PANEL_PAIR_GAP: f32 = 3.5;
const PANEL_COLLISION_GAP: f32 = 4.0;
const PANEL_SHIFT_STEP: f32 = 6.0;
// Version 3.2.0: 15% larger than the compact 3.1.0 box panels.
const BOX_PANEL_FONT_SIZE: f32 = LABEL_FONT_SIZE * 0.575;
const BOX_PANEL_PAD_X: f32 = 3.91;
const BOX_PANEL_PAD_Y: f32 = 2.47;
const BOX_PANEL_MIN_HEIGHT: f32 = 12.31;
const BOX_PANEL_RADIUS: f32 = 3.45;
const LOCK_SIZE: f32 = 10.0;
const LOCK_RADIUS: f32 = 2.5;
const LOCK_OFFSET_X: f32 = -13.0;
const LOCK_OFFSET_Y: f32 = -12.0;
const LOCK_HIT_PAD: f32 = 3.0;
const COURSE_ARROW_LENGTH: f32 = 14.0;
const COURSE_ARROW_GAP: f32 = 5.0;
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

#[derive(Clone, Copy, Debug)]
pub enum BoxAnchor {
    Vertex(usize),
    Segment { index: usize, t: f32 },
    Free(Point),
}

#[derive(Clone, Copy, Debug)]
pub struct BoxCorridor {
    pub anchor: BoxAnchor,
    pub end: Point,
}

#[derive(Clone, Debug, Default)]
pub struct BoxSavedState {
    pub visible: bool,
    pub closed: bool,
    pub points: Vec<Point>,
    pub locked: Vec<bool>,
    pub corridors: Vec<BoxCorridor>,
    pub reverse_bearings: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoxSegmentKind {
    Edge(usize),
    Corridor(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoxPanelKind {
    Distance(BoxSegmentKind),
    Bearing(BoxSegmentKind),
}

#[derive(Clone, Debug)]
pub struct BoxPanel {
    pub kind: BoxPanelKind,
    pub rect: PanelRect,
    pub text: String,
    pub arrow: Option<(Point, Point)>,
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
    if !matches!(version, Some(1) | Some(2) | Some(3)) {
        return BoxSavedState::default();
    }
    let visible = parse_bool(&mut values).unwrap_or(false);
    let closed = parse_bool(&mut values).unwrap_or(false);
    let reverse_bearings = if version == Some(3) {
        parse_bool(&mut values).unwrap_or(false)
    } else {
        false
    };
    let count = values
        .next()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0)
        .min(MAX_BOX_POINTS);
    let mut points = Vec::with_capacity(count);
    for _ in 0..count {
        let Some(point) = parse_point(&mut values) else {
            return BoxSavedState::default();
        };
        points.push(point);
    }

    let mut locked = vec![false; points.len()];
    if matches!(version, Some(2) | Some(3)) {
        for item in &mut locked {
            *item = parse_bool(&mut values).unwrap_or(false);
        }
    }

    let mut corridors = Vec::new();
    if version == Some(3) {
        let corridor_count = values
            .next()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0)
            .min(MAX_BOX_CORRIDORS);
        for _ in 0..corridor_count {
            let Some(anchor_kind) = values.next().and_then(|value| value.parse::<u8>().ok()) else {
                break;
            };
            let Some(index) = values.next().and_then(|value| value.parse::<usize>().ok()) else {
                break;
            };
            let Some(t_or_x) = values.next().and_then(|value| value.parse::<f32>().ok()) else {
                break;
            };
            let Some(extra_y) = values.next().and_then(|value| value.parse::<f32>().ok()) else {
                break;
            };
            let Some(end) = parse_point(&mut values) else {
                break;
            };
            let anchor = match anchor_kind {
                0 => BoxAnchor::Vertex(index),
                1 => BoxAnchor::Segment {
                    index,
                    t: t_or_x.clamp(0.0, 1.0),
                },
                2 => BoxAnchor::Free(Point {
                    x: t_or_x,
                    y: extra_y,
                }),
                _ => continue,
            };
            corridors.push(BoxCorridor { anchor, end });
        }
    }

    let closed = closed && points.len() >= 3;
    locked.resize(points.len(), false);
    locked.truncate(points.len());
    let edge_count = if points.len() < 2 {
        0
    } else if closed {
        points.len()
    } else {
        points.len() - 1
    };
    corridors.retain(|corridor| {
        corridor.end.x.is_finite()
            && corridor.end.y.is_finite()
            && match corridor.anchor {
                BoxAnchor::Vertex(index) => index < points.len(),
                BoxAnchor::Segment { index, t } => {
                    index < edge_count && t.is_finite()
                }
                BoxAnchor::Free(point) => point.x.is_finite() && point.y.is_finite(),
            }
    });

    BoxSavedState {
        visible,
        closed,
        points,
        locked,
        corridors,
        reverse_bearings,
    }
}

fn parse_bool(values: &mut std::str::SplitWhitespace<'_>) -> Option<bool> {
    values.next()?.parse::<u8>().ok().map(|value| value != 0)
}

fn parse_point(values: &mut std::str::SplitWhitespace<'_>) -> Option<Point> {
    let x = values.next()?.parse::<f32>().ok()?;
    let y = values.next()?.parse::<f32>().ok()?;
    (x.is_finite() && y.is_finite()).then_some(Point { x, y })
}

pub fn save_box_state(
    path: &Path,
    visible: bool,
    closed: bool,
    points: &[Point],
    locked: &[bool],
    corridors: &[BoxCorridor],
    reverse_bearings: bool,
) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let point_count = points.len().min(MAX_BOX_POINTS);
    let corridor_count = corridors.len().min(MAX_BOX_CORRIDORS);
    let mut lines = vec![
        "3".to_string(),
        u8::from(visible).to_string(),
        u8::from(closed && point_count >= 3).to_string(),
        u8::from(reverse_bearings).to_string(),
        point_count.to_string(),
    ];
    for point in points.iter().take(point_count) {
        lines.push(format!("{} {}", point.x, point.y));
    }
    for index in 0..point_count {
        lines.push(u8::from(locked.get(index).copied().unwrap_or(false)).to_string());
    }
    lines.push(corridor_count.to_string());
    for corridor in corridors.iter().take(corridor_count) {
        match corridor.anchor {
            BoxAnchor::Vertex(index) => lines.push(format!(
                "0 {} 0 0 {} {}",
                index, corridor.end.x, corridor.end.y
            )),
            BoxAnchor::Segment { index, t } => lines.push(format!(
                "1 {} {} 0 {} {}",
                index, t, corridor.end.x, corridor.end.y
            )),
            BoxAnchor::Free(point) => lines.push(format!(
                "2 0 {} {} {} {}",
                point.x, point.y, corridor.end.x, corridor.end.y
            )),
        }
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

pub fn box_corridor_end_at(point: Point, corridors: &[BoxCorridor]) -> Option<usize> {
    corridors.iter().enumerate().find_map(|(index, corridor)| {
        let dx = corridor.end.x - point.x;
        let dy = corridor.end.y - point.y;
        (dx * dx + dy * dy <= MARKER_HIT_RADIUS * MARKER_HIT_RADIUS).then_some(index)
    })
}


pub fn box_corridor_anchor_at(
    point: Point,
    points: &[Point],
    closed: bool,
    corridors: &[BoxCorridor],
) -> Option<usize> {
    corridors.iter().enumerate().find_map(|(index, corridor)| {
        let anchor = box_anchor_position(points, closed, &corridor.anchor);
        let dx = anchor.x - point.x;
        let dy = anchor.y - point.y;
        (dx * dx + dy * dy
            <= CORRIDOR_ANCHOR_HIT_RADIUS * CORRIDOR_ANCHOR_HIT_RADIUS)
            .then_some(index)
    })
}

pub fn snap_corridor_anchor(
    point: Point,
    points: &[Point],
    closed: bool,
    corridors: &[BoxCorridor],
    moving_corridor: usize,
) -> BoxAnchor {
    let mut best: Option<(f32, BoxAnchor)> = None;
    let mut consider = |distance: f32, anchor: BoxAnchor| {
        if distance.is_finite()
            && best
                .as_ref()
                .map(|(best_distance, _)| distance < *best_distance)
                .unwrap_or(true)
        {
            best = Some((distance, anchor));
        }
    };

    for (index, candidate) in points.iter().enumerate() {
        consider(length(point, *candidate), BoxAnchor::Vertex(index));
    }

    for index in 0..segment_count(points, closed) {
        if let Some((from, to)) = segment(points, closed, index) {
            let (distance, t) = distance_to_segment(point, from, to);
            consider(distance, BoxAnchor::Segment { index, t });
        }
    }

    for (index, corridor) in corridors.iter().enumerate() {
        if index == moving_corridor {
            continue;
        }
        consider(length(point, corridor.end), BoxAnchor::Free(corridor.end));
        let start = box_anchor_position(points, closed, &corridor.anchor);
        let (distance, t) = distance_to_segment(point, start, corridor.end);
        let projected = Point {
            x: start.x + (corridor.end.x - start.x) * t,
            y: start.y + (corridor.end.y - start.y) * t,
        };
        consider(distance, BoxAnchor::Free(projected));
    }

    best.map(|(_, anchor)| anchor).unwrap_or(BoxAnchor::Free(point))
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

pub fn box_anchor_position(points: &[Point], closed: bool, anchor: &BoxAnchor) -> Point {
    match *anchor {
        BoxAnchor::Vertex(index) => points.get(index).copied().unwrap_or(Point { x: 0.0, y: 0.0 }),
        BoxAnchor::Segment { index, t } => {
            if let Some((from, to)) = segment(points, closed, index) {
                Point {
                    x: from.x + (to.x - from.x) * t.clamp(0.0, 1.0),
                    y: from.y + (to.y - from.y) * t.clamp(0.0, 1.0),
                }
            } else {
                Point { x: 0.0, y: 0.0 }
            }
        }
        BoxAnchor::Free(point) => point,
    }
}

pub fn translate_free_corridor_anchors(corridors: &mut [BoxCorridor], dx: f32, dy: f32) {
    for corridor in corridors {
        if let BoxAnchor::Free(point) = &mut corridor.anchor {
            point.x += dx;
            point.y += dy;
        }
        corridor.end.x += dx;
        corridor.end.y += dy;
    }
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

fn projection_parameter(point: Point, from: Point, to: Point) -> f32 {
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    let denom = dx * dx + dy * dy;
    if denom < EPSILON {
        0.0
    } else {
        (((point.x - from.x) * dx + (point.y - from.y) * dy) / denom).clamp(0.0, 1.0)
    }
}

fn distance_to_segment(point: Point, from: Point, to: Point) -> (f32, f32) {
    let t = projection_parameter(point, from, to);
    let projected = Point {
        x: from.x + (to.x - from.x) * t,
        y: from.y + (to.y - from.y) * t,
    };
    (length(point, projected), t)
}

pub fn box_segment_at(
    point: Point,
    points: &[Point],
    closed: bool,
    corridors: &[BoxCorridor],
) -> Option<(BoxSegmentKind, Point)> {
    let mut best: Option<(BoxSegmentKind, Point, f32)> = None;
    for index in 0..segment_count(points, closed) {
        let Some((from, to)) = segment(points, closed, index) else { continue; };
        let (distance, t) = distance_to_segment(point, from, to);
        if distance <= SEGMENT_HIT_RADIUS
            && best.as_ref().map(|item| distance < item.2).unwrap_or(true)
        {
            best = Some((
                BoxSegmentKind::Edge(index),
                Point {
                    x: from.x + (to.x - from.x) * t,
                    y: from.y + (to.y - from.y) * t,
                },
                distance,
            ));
        }
    }
    for (index, corridor) in corridors.iter().enumerate() {
        let from = box_anchor_position(points, closed, &corridor.anchor);
        let to = corridor.end;
        let (distance, t) = distance_to_segment(point, from, to);
        if distance <= SEGMENT_HIT_RADIUS
            && best.as_ref().map(|item| distance < item.2).unwrap_or(true)
        {
            best = Some((
                BoxSegmentKind::Corridor(index),
                Point {
                    x: from.x + (to.x - from.x) * t,
                    y: from.y + (to.y - from.y) * t,
                },
                distance,
            ));
        }
    }
    best.map(|(kind, projected, _)| (kind, projected))
}

pub fn anchor_for_segment(
    kind: BoxSegmentKind,
    projected: Point,
    points: &[Point],
    closed: bool,
    corridors: &[BoxCorridor],
) -> BoxAnchor {
    match kind {
        BoxSegmentKind::Edge(index) => {
            if let Some((from, to)) = segment(points, closed, index) {
                BoxAnchor::Segment {
                    index,
                    t: projection_parameter(projected, from, to),
                }
            } else {
                BoxAnchor::Free(projected)
            }
        }
        BoxSegmentKind::Corridor(index) => {
            // A branch from an existing corridor remains at the selected screen point.
            let _ = corridors.get(index);
            BoxAnchor::Free(projected)
        }
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

fn bearing_degrees(from: Point, to: Point, north_angle: f32, reverse: bool) -> i32 {
    let segment_angle = (to.y - from.y).atan2(to.x - from.x);
    let extra = if reverse { std::f32::consts::PI } else { 0.0 };
    let degrees = normalize_bearing(segment_angle + extra - north_angle)
        .to_degrees()
        .round() as i32;
    degrees.rem_euclid(360)
}

fn stroke_hit_segment(pixmap: &mut Pixmap, from: Point, to: Point) {
    let mut builder = PathBuilder::new();
    builder.move_to(from.x, from.y);
    builder.line_to(to.x, to.y);
    if let Some(path) = builder.finish() {
        let mut paint = Paint::default();
        // A nearly invisible continuous stroke keeps the whole dashed segment
        // interactive in the Windows layered window, including the dash gaps.
        paint.set_color(Color::from_rgba8(214, 32, 168, 1));
        paint.anti_alias = true;
        pixmap.stroke_path(
            &path,
            &paint,
            &Stroke {
                width: SEGMENT_HIT_RADIUS * 2.0,
                ..Stroke::default()
            },
            Transform::identity(),
            None,
        );
    }
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

fn draw_triangle_marker(pixmap: &mut Pixmap, center: Point, label: &str) {
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

    let layout = layout_text(label, NUMBER_FONT_SIZE);
    let text_center = Point {
        x: center.x + MARKER_RADIUS + 5.5,
        y: center.y - MARKER_RADIUS * 0.55,
    };
    let x = text_center.x - (layout.xmin + layout.xmax) * 0.5;
    let baseline = text_center.y + (layout.ymin + layout.ymax) * 0.5;
    draw_text(
        pixmap,
        label,
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

fn draw_corridor_anchor_marker(pixmap: &mut Pixmap, center: Point) {
    let mut outer = PathBuilder::new();
    outer.push_circle(center.x, center.y, CORRIDOR_ANCHOR_RADIUS + 1.5);
    if let Some(path) = outer.finish() {
        let mut paint = Paint::default();
        paint.set_color(Color::from_rgba8(255, 255, 255, 235));
        paint.anti_alias = true;
        pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
    }

    let mut inner = PathBuilder::new();
    inner.push_circle(center.x, center.y, CORRIDOR_ANCHOR_RADIUS);
    if let Some(path) = inner.finish() {
        let mut paint = Paint::default();
        paint.set_color(Color::from_rgba8(18, 18, 18, 250));
        paint.anti_alias = true;
        pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
    }
}

pub fn draw_corridor_anchor_preview(pixmap: &mut Pixmap, center: Point) {
    draw_corridor_anchor_marker(pixmap, center);
}

pub fn draw_box_geometry(
    pixmap: &mut Pixmap,
    points: &[Point],
    closed: bool,
    locked: &[bool],
    corridors: &[BoxCorridor],
) {
    for index in 0..segment_count(points, closed) {
        if let Some((from, to)) = segment(points, closed, index) {
            stroke_hit_segment(pixmap, from, to);
            stroke_dashed_segment(pixmap, from, to);
        }
    }
    for corridor in corridors {
        let start = box_anchor_position(points, closed, &corridor.anchor);
        stroke_hit_segment(pixmap, start, corridor.end);
        stroke_dashed_segment(pixmap, start, corridor.end);
    }
    for (index, point) in points.iter().enumerate() {
        draw_triangle_marker(pixmap, *point, &(index + 1).to_string());
        draw_lock_icon(
            pixmap,
            *point,
            locked.get(index).copied().unwrap_or(false),
        );
    }
    for (index, corridor) in corridors.iter().enumerate() {
        draw_triangle_marker(pixmap, corridor.end, &format!("C{}", index + 1));
    }
    // Draw the corridor origin last so the black attachment point remains
    // visible even when it is snapped directly onto a magenta triangle.
    for corridor in corridors {
        let start = box_anchor_position(points, closed, &corridor.anchor);
        draw_corridor_anchor_marker(pixmap, start);
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

fn mask_bit(kind: BoxSegmentKind) -> u32 {
    match kind {
        BoxSegmentKind::Edge(index) => 1u32 << index.min(15),
        BoxSegmentKind::Corridor(index) => 1u32 << (16 + index.min(15)),
    }
}

fn add_segment_panels(
    panels: &mut Vec<BoxPanel>,
    occupied: &mut Vec<PanelRect>,
    kind: BoxSegmentKind,
    from: Point,
    to: Point,
    meters_per_pixel: f32,
    north_angle: f32,
    reverse_bearings: bool,
    show_distance: bool,
    show_bearing: bool,
    hidden_distance_mask: u32,
    hidden_bearing_mask: u32,
) {
    let bit = mask_bit(kind);
    let distance_text = (show_distance
        && meters_per_pixel > 0.0
        && hidden_distance_mask & bit == 0)
        .then(|| format_distance(length(from, to) * meters_per_pixel));
    let bearing_text = (show_bearing && hidden_bearing_mask & bit == 0)
        .then(|| format!("{}°", bearing_degrees(from, to, north_angle, reverse_bearings)));
    if distance_text.is_none() && bearing_text.is_none() {
        return;
    }

    let (mid, ux, uy, nx, ny) = segment_basis(from, to);
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
            kinds.push(BoxPanelKind::Distance(kind));
            kinds.push(BoxPanelKind::Bearing(kind));
            texts.push(distance);
            texts.push(bearing);
        }
        (Some(distance), None) => {
            preferred.push(compact_panel_rect(&distance, base));
            kinds.push(BoxPanelKind::Distance(kind));
            texts.push(distance);
        }
        (None, Some(bearing)) => {
            preferred.push(compact_panel_rect(&bearing, base));
            kinds.push(BoxPanelKind::Bearing(kind));
            texts.push(bearing);
        }
        (None, None) => {}
    }

    let resolved = resolve_group(&preferred, occupied);
    occupied.extend(resolved.iter().copied());
    for ((panel_kind, text), rect) in kinds.into_iter().zip(texts).zip(resolved) {
        let arrow = if matches!(panel_kind, BoxPanelKind::Bearing(_)) {
            let direction = if reverse_bearings { -1.0 } else { 1.0 };
            let center = Point {
                x: rect.x + rect.width * 0.5,
                y: rect.y - COURSE_ARROW_GAP,
            };
            let half = COURSE_ARROW_LENGTH * 0.5;
            Some((
                Point {
                    x: center.x - ux * half * direction,
                    y: center.y - uy * half * direction,
                },
                Point {
                    x: center.x + ux * half * direction,
                    y: center.y + uy * half * direction,
                },
            ))
        } else {
            None
        };
        panels.push(BoxPanel {
            kind: panel_kind,
            rect,
            text,
            arrow,
        });
    }
}

pub fn layout_box_panels(
    points: &[Point],
    closed: bool,
    corridors: &[BoxCorridor],
    meters_per_pixel: f32,
    north_angle: f32,
    reverse_bearings: bool,
    show_distance: bool,
    show_bearing: bool,
    hidden_distance_mask: u32,
    hidden_bearing_mask: u32,
    avoid_rects: &[PanelRect],
) -> BoxPanelLayout {
    let mut occupied = avoid_rects.to_vec();
    let mut panels = Vec::new();

    for index in 0..segment_count(points, closed) {
        if let Some((from, to)) = segment(points, closed, index) {
            add_segment_panels(
                &mut panels,
                &mut occupied,
                BoxSegmentKind::Edge(index),
                from,
                to,
                meters_per_pixel,
                north_angle,
                reverse_bearings,
                show_distance,
                show_bearing,
                hidden_distance_mask,
                hidden_bearing_mask,
            );
        }
    }
    for (index, corridor) in corridors.iter().enumerate() {
        let from = box_anchor_position(points, closed, &corridor.anchor);
        add_segment_panels(
            &mut panels,
            &mut occupied,
            BoxSegmentKind::Corridor(index),
            from,
            corridor.end,
            meters_per_pixel,
            north_angle,
            reverse_bearings,
            show_distance,
            show_bearing,
            hidden_distance_mask,
            hidden_bearing_mask,
        );
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

fn draw_course_arrow(pixmap: &mut Pixmap, from: Point, to: Point) {
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    let len = (dx * dx + dy * dy).sqrt();
    if len < EPSILON {
        return;
    }
    let ux = dx / len;
    let uy = dy / len;
    let mut builder = PathBuilder::new();
    builder.move_to(from.x, from.y);
    builder.line_to(to.x, to.y);
    let head = 3.8;
    let px = -uy;
    let py = ux;
    builder.move_to(to.x, to.y);
    builder.line_to(to.x - ux * head + px * head * 0.65, to.y - uy * head + py * head * 0.65);
    builder.move_to(to.x, to.y);
    builder.line_to(to.x - ux * head - px * head * 0.65, to.y - uy * head - py * head * 0.65);
    if let Some(path) = builder.finish() {
        let mut paint = Paint::default();
        paint.set_color(magenta_dark());
        paint.anti_alias = true;
        pixmap.stroke_path(
            &path,
            &paint,
            &Stroke {
                width: 1.15,
                ..Stroke::default()
            },
            Transform::identity(),
            None,
        );
    }
}

pub fn draw_box_panels(pixmap: &mut Pixmap, layout: &BoxPanelLayout) {
    for panel in &layout.panels {
        if let Some((from, to)) = panel.arrow {
            draw_course_arrow(pixmap, from, to);
        }
        draw_compact_panel(pixmap, panel);
    }
}

pub fn box_bounds(
    points: &[Point],
    closed: bool,
    corridors: &[BoxCorridor],
    layout: &BoxPanelLayout,
) -> Option<ContentBounds> {
    let first = points
        .first()
        .copied()
        .or_else(|| corridors.first().map(|corridor| corridor.end))?;
    let mut bounds = ContentBounds {
        min_x: first.x - 28.0,
        min_y: first.y - 32.0,
        max_x: first.x + 28.0,
        max_y: first.y + 24.0,
    };
    for point in points.iter().skip(1) {
        bounds.min_x = bounds.min_x.min(point.x - 28.0);
        bounds.min_y = bounds.min_y.min(point.y - 32.0);
        bounds.max_x = bounds.max_x.max(point.x + 28.0);
        bounds.max_y = bounds.max_y.max(point.y + 24.0);
    }
    for corridor in corridors {
        let start = box_anchor_position(points, closed, &corridor.anchor);
        for point in [start, corridor.end] {
            bounds.min_x = bounds.min_x.min(point.x - 28.0);
            bounds.min_y = bounds.min_y.min(point.y - 32.0);
            bounds.max_x = bounds.max_x.max(point.x + 28.0);
            bounds.max_y = bounds.max_y.max(point.y + 24.0);
        }
    }
    for panel in &layout.panels {
        let rect = panel.rect;
        bounds.min_x = bounds.min_x.min(rect.x);
        bounds.min_y = bounds.min_y.min(rect.y - 12.0);
        bounds.max_x = bounds.max_x.max(rect.x + rect.width);
        bounds.max_y = bounds.max_y.max(rect.y + rect.height);
        if let Some((from, to)) = panel.arrow {
            bounds.min_x = bounds.min_x.min(from.x.min(to.x));
            bounds.min_y = bounds.min_y.min(from.y.min(to.y));
            bounds.max_x = bounds.max_x.max(from.x.max(to.x));
            bounds.max_y = bounds.max_y.max(from.y.max(to.y));
        }
    }
    Some(bounds)
}
