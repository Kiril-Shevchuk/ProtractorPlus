use crate::draw::{fill_rounded_rect, ContentBounds, PanelRect, Point};
use crate::text::{draw_text, layout_text};
use tiny_skia::{Color, Pixmap};

const LABEL_OFFSET: f32 = 18.0;
const DISTANCE_FONT_SIZE: f32 = 14.85;
const DISTANCE_PAD_X: f32 = 6.375;
const DISTANCE_PAD_Y: f32 = 4.42;
const DISTANCE_MIN_HEIGHT: f32 = 19.975;
const DISTANCE_RADIUS: f32 = 5.44;
const PANEL_GAP: f32 = 5.0;
const PANEL_SHIFT_STEP: f32 = 7.0;
const EPSILON: f32 = 0.0001;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DistanceKind {
    Base,
    LeftRay,
    RightRay,
    Hypotenuse,
    FrontLeft,
    FrontRight,
    FrontPerpendicular,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct HiddenDistancePanels {
    base: bool,
    left_ray: bool,
    right_ray: bool,
    hypotenuse: bool,
    front_left: bool,
    front_right: bool,
    front_perpendicular: bool,
}

impl HiddenDistancePanels {
    pub fn is_hidden(self, kind: DistanceKind) -> bool {
        match kind {
            DistanceKind::Base => self.base,
            DistanceKind::LeftRay => self.left_ray,
            DistanceKind::RightRay => self.right_ray,
            DistanceKind::Hypotenuse => self.hypotenuse,
            DistanceKind::FrontLeft => self.front_left,
            DistanceKind::FrontRight => self.front_right,
            DistanceKind::FrontPerpendicular => self.front_perpendicular,
        }
    }

    pub fn hide(&mut self, kind: DistanceKind) {
        match kind {
            DistanceKind::Base => self.base = true,
            DistanceKind::LeftRay => self.left_ray = true,
            DistanceKind::RightRay => self.right_ray = true,
            DistanceKind::Hypotenuse => self.hypotenuse = true,
            DistanceKind::FrontLeft => self.front_left = true,
            DistanceKind::FrontRight => self.front_right = true,
            DistanceKind::FrontPerpendicular => self.front_perpendicular = true,
        }
    }

    pub fn show(&mut self, kind: DistanceKind) {
        match kind {
            DistanceKind::Base => self.base = false,
            DistanceKind::LeftRay => self.left_ray = false,
            DistanceKind::RightRay => self.right_ray = false,
            DistanceKind::Hypotenuse => self.hypotenuse = false,
            DistanceKind::FrontLeft => self.front_left = false,
            DistanceKind::FrontRight => self.front_right = false,
            DistanceKind::FrontPerpendicular => self.front_perpendicular = false,
        }
    }

    pub fn clear_all(&mut self) {
        *self = Self::default();
    }
}

#[derive(Clone, Debug)]
pub struct DistanceEditor {
    pub kind: DistanceKind,
    pub buffer: String,
    pub replace_on_input: bool,
}

#[derive(Clone, Debug)]
pub struct DistancePanel {
    pub kind: DistanceKind,
    pub rect: PanelRect,
    pub text: String,
    pub center: Point,
}

fn length(a: Point, b: Point) -> f32 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    (dx * dx + dy * dy).sqrt()
}

fn midpoint(a: Point, b: Point) -> Point {
    Point {
        x: (a.x + b.x) * 0.5,
        y: (a.y + b.y) * 0.5,
    }
}

fn bisector_direction(points: [Point; 3]) -> Option<(f32, f32)> {
    let vertex = points[1];
    let left = points[0];
    let right = points[2];
    let left_len = length(vertex, left);
    let right_len = length(vertex, right);
    if left_len < EPSILON || right_len < EPSILON {
        return None;
    }

    let left_unit = (
        (left.x - vertex.x) / left_len,
        (left.y - vertex.y) / left_len,
    );
    let right_unit = (
        (right.x - vertex.x) / right_len,
        (right.y - vertex.y) / right_len,
    );
    let mut bx = left_unit.0 + right_unit.0;
    let mut by = left_unit.1 + right_unit.1;
    let magnitude = (bx * bx + by * by).sqrt();
    if magnitude < EPSILON {
        bx = -left_unit.1;
        by = left_unit.0;
    } else {
        bx /= magnitude;
        by /= magnitude;
    }
    Some((bx, by))
}

fn helper_bisector_foot(points: [Point; 3], helper: Point) -> Point {
    let vertex = points[1];
    let Some((bx, by)) = bisector_direction(points) else {
        return vertex;
    };
    let hx = helper.x - vertex.x;
    let hy = helper.y - vertex.y;
    let projection = hx * bx + hy * by;
    Point {
        x: vertex.x + bx * projection,
        y: vertex.y + by * projection,
    }
}

fn segment_for_kind(
    points: [Point; 3],
    helper: Point,
    kind: DistanceKind,
) -> (Point, Point) {
    match kind {
        DistanceKind::Base => (points[1], helper),
        DistanceKind::LeftRay => (points[1], points[0]),
        DistanceKind::RightRay => (points[1], points[2]),
        DistanceKind::Hypotenuse => (points[0], points[2]),
        DistanceKind::FrontLeft => (helper, points[0]),
        DistanceKind::FrontRight => (helper, points[2]),
        DistanceKind::FrontPerpendicular => (helper, helper_bisector_foot(points, helper)),
    }
}

fn reference_for_kind(points: [Point; 3], helper: Point, kind: DistanceKind) -> Point {
    match kind {
        DistanceKind::Base => midpoint(points[0], points[2]),
        DistanceKind::LeftRay | DistanceKind::RightRay => helper,
        DistanceKind::Hypotenuse
        | DistanceKind::FrontLeft
        | DistanceKind::FrontRight
        | DistanceKind::FrontPerpendicular => points[1],
    }
}

fn panel_center(points: [Point; 3], helper: Point, kind: DistanceKind) -> Point {
    let (from, to) = segment_for_kind(points, helper, kind);
    let mid = midpoint(from, to);
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    let len = (dx * dx + dy * dy).sqrt();
    if len < EPSILON {
        return mid;
    }

    let mut nx = -dy / len;
    let mut ny = dx / len;
    let reference = reference_for_kind(points, helper, kind);
    let toward_reference = nx * (reference.x - mid.x) + ny * (reference.y - mid.y);
    if toward_reference > 0.0 {
        nx = -nx;
        ny = -ny;
    }

    Point {
        x: mid.x + nx * LABEL_OFFSET,
        y: mid.y + ny * LABEL_OFFSET,
    }
}

fn visible_kinds(
    show_hypotenuse: bool,
    show_front_plus: bool,
    show_xtk_plus: bool,
) -> Vec<DistanceKind> {
    let mut kinds = vec![
        DistanceKind::Base,
        DistanceKind::LeftRay,
        DistanceKind::RightRay,
    ];
    if show_hypotenuse {
        kinds.push(DistanceKind::Hypotenuse);
    }
    if show_front_plus {
        kinds.push(DistanceKind::FrontLeft);
        kinds.push(DistanceKind::FrontRight);
    }
    if show_xtk_plus {
        kinds.push(DistanceKind::FrontPerpendicular);
    }
    kinds
}

pub fn meters_for_kind(
    points: [Point; 3],
    helper: Point,
    meters_per_pixel: f32,
    kind: DistanceKind,
) -> f32 {
    let (from, to) = segment_for_kind(points, helper, kind);
    length(from, to) * meters_per_pixel
}

pub fn format_distance(value: f32) -> String {
    if !value.is_finite() {
        return "—".to_string();
    }
    let abs = value.abs();
    if abs >= 100.0 {
        format!("{:.0}м", value)
    } else if abs >= 10.0 {
        format!("{:.1}м", value)
    } else {
        format!("{:.2}м", value)
    }
}

fn panel_text(
    points: [Point; 3],
    helper: Point,
    meters_per_pixel: f32,
    kind: DistanceKind,
    editor: Option<&DistanceEditor>,
) -> String {
    if let Some(editor) = editor {
        if editor.kind == kind {
            return format!("{}м", editor.buffer);
        }
    }
    format_distance(meters_for_kind(points, helper, meters_per_pixel, kind))
}

fn distance_panel_rect(text: &str, cx: f32, cy: f32) -> PanelRect {
    let layout = layout_text(text, DISTANCE_FONT_SIZE);
    let width = layout.width + 2.0 * DISTANCE_PAD_X;
    let height = (layout.height + 2.0 * DISTANCE_PAD_Y).max(DISTANCE_MIN_HEIGHT);
    PanelRect {
        x: cx - width * 0.5,
        y: cy - height * 0.5,
        width,
        height,
    }
}

fn rects_overlap(a: PanelRect, b: PanelRect, gap: f32) -> bool {
    a.x < b.x + b.width + gap
        && a.x + a.width + gap > b.x
        && a.y < b.y + b.height + gap
        && a.y + a.height + gap > b.y
}

fn move_rect(rect: PanelRect, dx: f32, dy: f32) -> PanelRect {
    PanelRect {
        x: rect.x + dx,
        y: rect.y + dy,
        ..rect
    }
}

fn resolve_panel_rect(original: PanelRect, occupied: &[PanelRect]) -> PanelRect {
    if occupied
        .iter()
        .all(|other| !rects_overlap(original, *other, PANEL_GAP))
    {
        return original;
    }

    // Search a compact spiral around the preferred position. Vertical offsets
    // are tried first because they preserve the panel's relation to its line.
    for ring in 1..=18 {
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
            let candidate = move_rect(original, dx, dy);
            if occupied
                .iter()
                .all(|other| !rects_overlap(candidate, *other, PANEL_GAP))
            {
                return candidate;
            }
        }
    }

    move_rect(original, 0.0, 19.0 * PANEL_SHIFT_STEP)
}

fn draw_distance_panel(
    pixmap: &mut Pixmap,
    text: &str,
    center: Point,
    background: Color,
    foreground: Color,
) {
    let rect = distance_panel_rect(text, center.x, center.y);
    fill_rounded_rect(
        pixmap,
        rect.x,
        rect.y,
        rect.width,
        rect.height,
        DISTANCE_RADIUS,
        background,
    );
    let layout = layout_text(text, DISTANCE_FONT_SIZE);
    let text_x = center.x - (layout.xmin + layout.xmax) * 0.5;
    let baseline = center.y + (layout.ymin + layout.ymax) * 0.5;
    draw_text(
        pixmap,
        text,
        text_x,
        baseline,
        DISTANCE_FONT_SIZE,
        foreground,
    );
}

pub fn distance_panels(
    points: [Point; 3],
    helper: Point,
    meters_per_pixel: f32,
    show_hypotenuse: bool,
    show_front_plus: bool,
    show_xtk_plus: bool,
    editor: Option<&DistanceEditor>,
    hidden: HiddenDistancePanels,
    avoid_rects: &[PanelRect],
) -> Vec<DistancePanel> {
    let mut occupied = avoid_rects.to_vec();
    let mut panels = Vec::new();

    for kind in visible_kinds(show_hypotenuse, show_front_plus, show_xtk_plus) {
        if hidden.is_hidden(kind) {
            continue;
        }
        let preferred_center = panel_center(points, helper, kind);
        let text = panel_text(points, helper, meters_per_pixel, kind, editor);
        let preferred_rect = distance_panel_rect(&text, preferred_center.x, preferred_center.y);
        let rect = resolve_panel_rect(preferred_rect, &occupied);
        let center = Point {
            x: rect.x + rect.width * 0.5,
            y: rect.y + rect.height * 0.5,
        };
        occupied.push(rect);
        panels.push(DistancePanel {
            kind,
            rect,
            text,
            center,
        });
    }

    panels
}

pub fn draw_distance_overlay(
    pixmap: &mut Pixmap,
    points: [Point; 3],
    helper: Point,
    meters_per_pixel: f32,
    show_hypotenuse: bool,
    show_front_plus: bool,
    show_xtk_plus: bool,
    editor: Option<&DistanceEditor>,
    hidden: HiddenDistancePanels,
    avoid_rects: &[PanelRect],
) {
    for panel in distance_panels(
        points,
        helper,
        meters_per_pixel,
        show_hypotenuse,
        show_front_plus,
        show_xtk_plus,
        editor,
        hidden,
        avoid_rects,
    ) {
        let editing = editor
            .map(|active| active.kind == panel.kind)
            .unwrap_or(false);
        let is_xtk = panel.kind == DistanceKind::FrontPerpendicular;
        let background = match (is_xtk, editing) {
            (true, true) => Color::from_rgba8(255, 205, 205, 225),
            (true, false) => Color::from_rgba8(255, 225, 225, 190),
            (false, true) => Color::from_rgba8(205, 255, 215, 225),
            (false, false) => Color::from_rgba8(255, 255, 255, 178),
        };
        let foreground = if is_xtk {
            Color::from_rgba8(145, 24, 24, 250)
        } else {
            Color::from_rgba8(18, 18, 18, 250)
        };
        draw_distance_panel(
            pixmap,
            &panel.text,
            panel.center,
            background,
            foreground,
        );
    }
}

pub fn distance_bounds(
    points: [Point; 3],
    helper: Point,
    meters_per_pixel: f32,
    show_hypotenuse: bool,
    show_front_plus: bool,
    show_xtk_plus: bool,
    editor: Option<&DistanceEditor>,
    hidden: HiddenDistancePanels,
    avoid_rects: &[PanelRect],
) -> ContentBounds {
    let panels = distance_panels(
        points,
        helper,
        meters_per_pixel,
        show_hypotenuse,
        show_front_plus,
        show_xtk_plus,
        editor,
        hidden,
        avoid_rects,
    );
    let mut bounds = ContentBounds {
        min_x: helper.x,
        min_y: helper.y,
        max_x: helper.x,
        max_y: helper.y,
    };
    for panel in panels {
        bounds.min_x = bounds.min_x.min(panel.rect.x);
        bounds.min_y = bounds.min_y.min(panel.rect.y);
        bounds.max_x = bounds.max_x.max(panel.rect.x + panel.rect.width);
        bounds.max_y = bounds.max_y.max(panel.rect.y + panel.rect.height);
    }
    bounds
}

pub fn hit_test_distance_panel(
    point: Point,
    points: [Point; 3],
    helper: Point,
    meters_per_pixel: f32,
    show_hypotenuse: bool,
    show_front_plus: bool,
    show_xtk_plus: bool,
    editor: Option<&DistanceEditor>,
    hidden: HiddenDistancePanels,
    avoid_rects: &[PanelRect],
) -> Option<DistanceKind> {
    distance_panels(
        points,
        helper,
        meters_per_pixel,
        show_hypotenuse,
        show_front_plus,
        show_xtk_plus,
        editor,
        hidden,
        avoid_rects,
    )
    .into_iter()
    .find(|panel| {
        point.x >= panel.rect.x
            && point.x <= panel.rect.x + panel.rect.width
            && point.y >= panel.rect.y
            && point.y <= panel.rect.y + panel.rect.height
    })
    .map(|panel| panel.kind)
}
