#![windows_subsystem = "windows"]

mod box_overlay;
mod distance;
mod draw;
mod icon;
mod splash;
mod text;
mod win32_layered;

use std::f32::consts::PI;
use std::fs;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use distance::{
    distance_bounds, distance_panels, draw_distance_overlay, hit_test_distance_panel, meters_for_kind,
    DistanceEditor, DistanceKind, HiddenDistancePanels,
};
use box_overlay::{
    box_bounds, box_point_at, draw_box_geometry, draw_box_panels, is_close_target,
    layout_box_panels, load_box_state, save_box_state, BoxPanelKind, BoxPanelLayout,
    MAX_BOX_POINTS,
};
use draw::{
    angle_between, content_bounds, draw_arc, draw_plus_handle, draw_text_panel,
    fill_rounded_rect, label_panel_rect, stroke_line, text_panel_rect, ContentBounds, PanelRect,
    Point, HANDLE_RADIUS,
};
use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Rect, Stroke, Transform};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

use crate::text::{draw_text, layout_text};
use crate::win32_layered::{
    configure_overlay, ensure_topmost, hwnd_from_window, is_minimized, minimize_window,
    present_pixmap, restore_window, set_click_through, show_context_menu, ContextMenuState,
    MENU_BISECTOR, MENU_CLOSE, MENU_FRONT_PLUS, MENU_HYPOTENUSE, MENU_INVERSION,
    MENU_COURSE_PLUS, MENU_DISTANCE_PLUS, MENU_MINIMIZE, MENU_NORTH_PLUS, MENU_PLUS,
    MENU_PLUS_DEGREES, MENU_XTK_PLUS, MENU_BOX_PLUS, MENU_DELETE_BOX_POINT,
    show_box_point_menu,
};

static CLICK_THROUGH: AtomicBool = AtomicBool::new(false);

const CONTENT_PAD: f32 = 42.0;
const MIN_WINDOW: f32 = 96.0;
const MAX_LINE_LEN: f32 = 1000.0;
const LOCK_PANEL_SIZE: f32 = 15.5;
const LOCK_PANEL_RADIUS: f32 = 3.5;
const LOCK_DISTANCE: f32 = 34.0;
const RED_LOCK_DISTANCE: f32 = 27.0;
const HELPER_LOCK_OFFSET_X: f32 = 18.0;
const HELPER_LOCK_OFFSET_Y: f32 = -18.0;
const EPSILON: f32 = 0.0001;
const HELPER_HANDLE_INDEX: usize = 3;
const HELPER_DISTANCE: f32 = 92.0;
const PLUS_HIT_RADIUS: f32 = 14.0;
const NORTH_DEFAULT_ANGLE: f32 = -PI * 0.5;
const NORTH_ARROW_LENGTH: f32 = 84.0;
const NORTH_ARROW_HEAD: f32 = 9.0;
const NORTH_ARC_MIN_RADIUS: f32 = 38.0;
const NORTH_ARC_MAX_RADIUS: f32 = 72.0;
const NORTH_LABEL_OFFSET: f32 = 28.0;
const NORTH_TEXT_SIZE: f32 = 16.0;
const NORTH_HANDLE_INDEX: usize = 4;
const NORTH_HANDLE_HIT_RADIUS: f32 = 15.0;
const NORTH_LOCK_GAP: f32 = 5.0;
const COURSE_ARC_MIN_RADIUS: f32 = 24.0;
const COURSE_ARC_MAX_RADIUS: f32 = 50.0;
const COURSE_LABEL_OFFSET: f32 = 12.0;
const SPLASH_DURATION: Duration = Duration::from_secs(4);
const SPLASH_SCREEN_FRACTION: f32 = 0.25;
const SPLASH_MIN_SIZE: u32 = 220;
const SPLASH_MAX_SIZE: u32 = 720;

#[derive(Clone, Copy, Debug)]
struct MonitorGeometry {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

#[derive(Clone, Copy, Debug)]
struct SavedState {
    points: [Point; 3],
    helper_point: Option<Point>,
    angle_locked: bool,
    locked_signed_angle: f32,
    red_locked_index: Option<usize>,
    bisector_visible: bool,
    plus_degrees_visible: bool,
    inverted: bool,
    hypotenuse_visible: bool,
    front_plus_visible: bool,
    xtk_visible: bool,
    distance_visible: bool,
    meters_per_pixel: f32,
    north_visible: bool,
    north_angle: f32,
    north_locked: bool,
    course_visible: bool,
    blue_pinned: bool,
    left_red_pinned: bool,
    right_red_pinned: bool,
    helper_pinned: bool,
    window_x: i32,
    window_y: i32,
}

#[derive(Clone, Copy, Debug, Default)]
struct HiddenAnglePanels {
    main: bool,
    north: bool,
    course: bool,
    helper_left: bool,
    helper_right: bool,
    helper_yellow: bool,
    helper_delta: bool,
}

#[derive(Clone, Copy, Debug, Default)]
struct AnglePanelLayout {
    main: Option<PanelRect>,
    north: Option<PanelRect>,
    course: Option<PanelRect>,
    helper_left: Option<PanelRect>,
    helper_right: Option<PanelRect>,
    helper_yellow: Option<PanelRect>,
    helper_delta: Option<PanelRect>,
}

impl AnglePanelLayout {
    fn rects(self) -> Vec<PanelRect> {
        [
            self.main,
            self.north,
            self.course,
            self.helper_left,
            self.helper_right,
            self.helper_yellow,
            self.helper_delta,
        ]
        .into_iter()
        .flatten()
        .collect()
    }
}

fn translate_panel(panel: PanelRect, dx: f32, dy: f32) -> PanelRect {
    PanelRect {
        x: panel.x + dx,
        y: panel.y + dy,
        ..panel
    }
}

fn resolve_angle_panel(panel: PanelRect, occupied: &[PanelRect], anchor: Point) -> PanelRect {
    const GAP: f32 = 5.0;
    const STEP: f32 = 7.0;
    if occupied
        .iter()
        .all(|other| !panels_overlap_with_margin(panel, *other, GAP))
    {
        return panel;
    }

    let center = Point {
        x: panel.x + panel.width * 0.5,
        y: panel.y + panel.height * 0.5,
    };
    let mut ux = center.x - anchor.x;
    let mut uy = center.y - anchor.y;
    let len = (ux * ux + uy * uy).sqrt();
    if len > EPSILON {
        ux /= len;
        uy /= len;
    } else {
        ux = 0.0;
        uy = -1.0;
    }
    let px = -uy;
    let py = ux;

    for ring in 1..=18 {
        let d = ring as f32 * STEP;
        let candidates = [
            (ux * d, uy * d),
            (px * d, py * d),
            (-px * d, -py * d),
            (ux * d + px * d * 0.6, uy * d + py * d * 0.6),
            (ux * d - px * d * 0.6, uy * d - py * d * 0.6),
            (0.0, -d),
            (0.0, d),
            (-d, 0.0),
            (d, 0.0),
        ];
        for (dx, dy) in candidates {
            let candidate = translate_panel(panel, dx, dy);
            if occupied
                .iter()
                .all(|other| !panels_overlap_with_margin(candidate, *other, GAP))
            {
                return candidate;
            }
        }
    }

    translate_panel(panel, 0.0, 19.0 * STEP)
}

fn panel_layout_bounds(rects: &[PanelRect], fallback: Point) -> ContentBounds {
    let mut bounds = ContentBounds {
        min_x: fallback.x,
        min_y: fallback.y,
        max_x: fallback.x,
        max_y: fallback.y,
    };
    for panel in rects {
        bounds.min_x = bounds.min_x.min(panel.x);
        bounds.min_y = bounds.min_y.min(panel.y);
        bounds.max_x = bounds.max_x.max(panel.x + panel.width);
        bounds.max_y = bounds.max_y.max(panel.y + panel.height);
    }
    bounds
}


fn default_points() -> [Point; 3] {
    [
        Point { x: 140.0, y: 280.0 },
        Point { x: 320.0, y: 230.0 },
        Point { x: 500.0, y: 280.0 },
    ]
}

fn settings_path() -> PathBuf {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        return PathBuf::from(appdata)
            .join("ProtractorPlus")
            .join("settings.txt");
    }
    PathBuf::from("ProtractorPlus.settings.txt")
}

fn box_settings_path() -> PathBuf {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        return PathBuf::from(appdata)
            .join("ProtractorPlus")
            .join("box.txt");
    }
    PathBuf::from("ProtractorPlus.box.txt")
}

fn parse_next<T: FromStr>(values: &mut std::str::SplitWhitespace<'_>) -> Option<T> {
    values.next()?.parse().ok()
}

fn load_state() -> Option<SavedState> {
    let text = fs::read_to_string(settings_path()).ok()?;
    let mut values = text.split_whitespace();
    let version: u32 = parse_next(&mut values)?;

    let points = [
        Point {
            x: parse_next(&mut values)?,
            y: parse_next(&mut values)?,
        },
        Point {
            x: parse_next(&mut values)?,
            y: parse_next(&mut values)?,
        },
        Point {
            x: parse_next(&mut values)?,
            y: parse_next(&mut values)?,
        },
    ];

    match version {
        1 => Some(SavedState {
            points,
            helper_point: None,
            angle_locked: parse_next::<u8>(&mut values)? != 0,
            locked_signed_angle: parse_next(&mut values)?,
            red_locked_index: None,
            bisector_visible: true,
            plus_degrees_visible: false,
            inverted: false,
            hypotenuse_visible: false,
            front_plus_visible: false,
            xtk_visible: false,
            distance_visible: false,
            meters_per_pixel: 0.0,
            north_visible: false,
            north_angle: NORTH_DEFAULT_ANGLE,
            north_locked: false,
            course_visible: false,
            blue_pinned: false,
            left_red_pinned: false,
            right_red_pinned: false,
            helper_pinned: false,
            window_x: parse_next(&mut values)?,
            window_y: parse_next(&mut values)?,
        }),
        2 => {
            let helper_enabled = parse_next::<u8>(&mut values)? != 0;
            let helper_x: f32 = parse_next(&mut values)?;
            let helper_y: f32 = parse_next(&mut values)?;
            Some(SavedState {
                points,
                helper_point: helper_enabled.then_some(Point {
                    x: helper_x,
                    y: helper_y,
                }),
                angle_locked: parse_next::<u8>(&mut values)? != 0,
                locked_signed_angle: parse_next(&mut values)?,
                red_locked_index: None,
                bisector_visible: true,
                plus_degrees_visible: helper_enabled,
                inverted: false,
                hypotenuse_visible: false,
                front_plus_visible: false,
                xtk_visible: false,
                distance_visible: false,
                meters_per_pixel: 0.0,
                north_visible: false,
                north_angle: NORTH_DEFAULT_ANGLE,
                north_locked: false,
                course_visible: false,
                blue_pinned: false,
                left_red_pinned: false,
                right_red_pinned: false,
                helper_pinned: false,
                window_x: parse_next(&mut values)?,
                window_y: parse_next(&mut values)?,
            })
        }
        3 => {
            let helper_enabled = parse_next::<u8>(&mut values)? != 0;
            let helper_x: f32 = parse_next(&mut values)?;
            let helper_y: f32 = parse_next(&mut values)?;
            Some(SavedState {
                points,
                helper_point: helper_enabled.then_some(Point {
                    x: helper_x,
                    y: helper_y,
                }),
                angle_locked: parse_next::<u8>(&mut values)? != 0,
                locked_signed_angle: parse_next(&mut values)?,
                red_locked_index: None,
                bisector_visible: parse_next::<u8>(&mut values)? != 0,
                plus_degrees_visible: parse_next::<u8>(&mut values)? != 0,
                inverted: parse_next::<u8>(&mut values)? != 0,
                hypotenuse_visible: parse_next::<u8>(&mut values)? != 0,
                front_plus_visible: parse_next::<u8>(&mut values)? != 0,
                xtk_visible: false,
                distance_visible: false,
                meters_per_pixel: 0.0,
                north_visible: false,
                north_angle: NORTH_DEFAULT_ANGLE,
                north_locked: false,
                course_visible: false,
                blue_pinned: false,
                left_red_pinned: false,
                right_red_pinned: false,
                helper_pinned: false,
                window_x: parse_next(&mut values)?,
                window_y: parse_next(&mut values)?,
            })
        }
        4 => {
            let helper_enabled = parse_next::<u8>(&mut values)? != 0;
            let helper_x: f32 = parse_next(&mut values)?;
            let helper_y: f32 = parse_next(&mut values)?;
            Some(SavedState {
                points,
                helper_point: helper_enabled.then_some(Point {
                    x: helper_x,
                    y: helper_y,
                }),
                angle_locked: parse_next::<u8>(&mut values)? != 0,
                locked_signed_angle: parse_next(&mut values)?,
                red_locked_index: None,
                bisector_visible: parse_next::<u8>(&mut values)? != 0,
                plus_degrees_visible: parse_next::<u8>(&mut values)? != 0,
                inverted: parse_next::<u8>(&mut values)? != 0,
                hypotenuse_visible: parse_next::<u8>(&mut values)? != 0,
                front_plus_visible: parse_next::<u8>(&mut values)? != 0,
                xtk_visible: false,
                distance_visible: parse_next::<u8>(&mut values)? != 0,
                meters_per_pixel: parse_next(&mut values)?,
                north_visible: false,
                north_angle: NORTH_DEFAULT_ANGLE,
                north_locked: false,
                course_visible: false,
                blue_pinned: false,
                left_red_pinned: false,
                right_red_pinned: false,
                helper_pinned: false,
                window_x: parse_next(&mut values)?,
                window_y: parse_next(&mut values)?,
            })
        }
        5 => {
            let helper_enabled = parse_next::<u8>(&mut values)? != 0;
            let helper_x: f32 = parse_next(&mut values)?;
            let helper_y: f32 = parse_next(&mut values)?;
            let angle_locked = parse_next::<u8>(&mut values)? != 0;
            let locked_signed_angle = parse_next(&mut values)?;
            let red_lock_code: u8 = parse_next(&mut values)?;
            Some(SavedState {
                points,
                helper_point: helper_enabled.then_some(Point {
                    x: helper_x,
                    y: helper_y,
                }),
                angle_locked,
                locked_signed_angle,
                red_locked_index: match red_lock_code {
                    1 => Some(0),
                    2 => Some(2),
                    _ => None,
                },
                bisector_visible: parse_next::<u8>(&mut values)? != 0,
                plus_degrees_visible: parse_next::<u8>(&mut values)? != 0,
                inverted: parse_next::<u8>(&mut values)? != 0,
                hypotenuse_visible: parse_next::<u8>(&mut values)? != 0,
                front_plus_visible: parse_next::<u8>(&mut values)? != 0,
                xtk_visible: false,
                distance_visible: parse_next::<u8>(&mut values)? != 0,
                meters_per_pixel: parse_next(&mut values)?,
                north_visible: false,
                north_angle: NORTH_DEFAULT_ANGLE,
                north_locked: false,
                course_visible: false,
                blue_pinned: false,
                left_red_pinned: false,
                right_red_pinned: false,
                helper_pinned: false,
                window_x: parse_next(&mut values)?,
                window_y: parse_next(&mut values)?,
            })
        }
        6 => {
            let helper_enabled = parse_next::<u8>(&mut values)? != 0;
            let helper_x: f32 = parse_next(&mut values)?;
            let helper_y: f32 = parse_next(&mut values)?;
            let angle_locked = parse_next::<u8>(&mut values)? != 0;
            let locked_signed_angle = parse_next(&mut values)?;
            let red_lock_code: u8 = parse_next(&mut values)?;
            Some(SavedState {
                points,
                helper_point: helper_enabled.then_some(Point {
                    x: helper_x,
                    y: helper_y,
                }),
                angle_locked,
                locked_signed_angle,
                red_locked_index: match red_lock_code {
                    1 => Some(0),
                    2 => Some(2),
                    _ => None,
                },
                bisector_visible: parse_next::<u8>(&mut values)? != 0,
                plus_degrees_visible: parse_next::<u8>(&mut values)? != 0,
                inverted: parse_next::<u8>(&mut values)? != 0,
                hypotenuse_visible: parse_next::<u8>(&mut values)? != 0,
                front_plus_visible: parse_next::<u8>(&mut values)? != 0,
                xtk_visible: false,
                distance_visible: parse_next::<u8>(&mut values)? != 0,
                meters_per_pixel: parse_next(&mut values)?,
                north_visible: parse_next::<u8>(&mut values)? != 0,
                north_angle: NORTH_DEFAULT_ANGLE,
                north_locked: false,
                course_visible: false,
                blue_pinned: false,
                left_red_pinned: false,
                right_red_pinned: false,
                helper_pinned: false,
                window_x: parse_next(&mut values)?,
                window_y: parse_next(&mut values)?,
            })
        }
        7 => {
            let helper_enabled = parse_next::<u8>(&mut values)? != 0;
            let helper_x: f32 = parse_next(&mut values)?;
            let helper_y: f32 = parse_next(&mut values)?;
            let angle_locked = parse_next::<u8>(&mut values)? != 0;
            let locked_signed_angle = parse_next(&mut values)?;
            let red_lock_code: u8 = parse_next(&mut values)?;
            Some(SavedState {
                points,
                helper_point: helper_enabled.then_some(Point {
                    x: helper_x,
                    y: helper_y,
                }),
                angle_locked,
                locked_signed_angle,
                red_locked_index: match red_lock_code {
                    1 => Some(0),
                    2 => Some(2),
                    _ => None,
                },
                bisector_visible: parse_next::<u8>(&mut values)? != 0,
                plus_degrees_visible: parse_next::<u8>(&mut values)? != 0,
                inverted: parse_next::<u8>(&mut values)? != 0,
                hypotenuse_visible: parse_next::<u8>(&mut values)? != 0,
                front_plus_visible: parse_next::<u8>(&mut values)? != 0,
                xtk_visible: false,
                distance_visible: parse_next::<u8>(&mut values)? != 0,
                meters_per_pixel: parse_next(&mut values)?,
                north_visible: parse_next::<u8>(&mut values)? != 0,
                north_angle: parse_next(&mut values)?,
                north_locked: parse_next::<u8>(&mut values)? != 0,
                course_visible: false,
                blue_pinned: false,
                left_red_pinned: false,
                right_red_pinned: false,
                helper_pinned: false,
                window_x: parse_next(&mut values)?,
                window_y: parse_next(&mut values)?,
            })
        }
        8 => {
            let helper_enabled = parse_next::<u8>(&mut values)? != 0;
            let helper_x: f32 = parse_next(&mut values)?;
            let helper_y: f32 = parse_next(&mut values)?;
            let angle_locked = parse_next::<u8>(&mut values)? != 0;
            let locked_signed_angle = parse_next(&mut values)?;
            let red_lock_code: u8 = parse_next(&mut values)?;
            Some(SavedState {
                points,
                helper_point: helper_enabled.then_some(Point {
                    x: helper_x,
                    y: helper_y,
                }),
                angle_locked,
                locked_signed_angle,
                red_locked_index: match red_lock_code {
                    1 => Some(0),
                    2 => Some(2),
                    _ => None,
                },
                bisector_visible: parse_next::<u8>(&mut values)? != 0,
                plus_degrees_visible: parse_next::<u8>(&mut values)? != 0,
                inverted: parse_next::<u8>(&mut values)? != 0,
                hypotenuse_visible: parse_next::<u8>(&mut values)? != 0,
                front_plus_visible: parse_next::<u8>(&mut values)? != 0,
                xtk_visible: false,
                distance_visible: parse_next::<u8>(&mut values)? != 0,
                meters_per_pixel: parse_next(&mut values)?,
                north_visible: parse_next::<u8>(&mut values)? != 0,
                north_angle: parse_next(&mut values)?,
                north_locked: parse_next::<u8>(&mut values)? != 0,
                course_visible: false,
                blue_pinned: parse_next::<u8>(&mut values)? != 0,
                left_red_pinned: parse_next::<u8>(&mut values)? != 0,
                right_red_pinned: parse_next::<u8>(&mut values)? != 0,
                helper_pinned: false,
                window_x: parse_next(&mut values)?,
                window_y: parse_next(&mut values)?,
            })
        }
        9 | 10 | 11 => {
            let helper_enabled = parse_next::<u8>(&mut values)? != 0;
            let helper_x: f32 = parse_next(&mut values)?;
            let helper_y: f32 = parse_next(&mut values)?;
            let angle_locked = parse_next::<u8>(&mut values)? != 0;
            let locked_signed_angle = parse_next(&mut values)?;
            let red_lock_code: u8 = parse_next(&mut values)?;
            let bisector_visible = parse_next::<u8>(&mut values)? != 0;
            let plus_degrees_visible = parse_next::<u8>(&mut values)? != 0;
            let inverted = parse_next::<u8>(&mut values)? != 0;
            let hypotenuse_visible = parse_next::<u8>(&mut values)? != 0;
            let front_plus_visible = parse_next::<u8>(&mut values)? != 0;
            let xtk_visible = if version >= 11 {
                parse_next::<u8>(&mut values)? != 0
            } else {
                // Preserve the v2.8 visual when migrating old settings: the
                // transverse line used to be part of Front +.
                front_plus_visible
            };
            let distance_visible = parse_next::<u8>(&mut values)? != 0;
            let meters_per_pixel = parse_next(&mut values)?;
            let north_visible = parse_next::<u8>(&mut values)? != 0;
            let north_angle = parse_next(&mut values)?;
            let north_locked = parse_next::<u8>(&mut values)? != 0;
            let course_visible = parse_next::<u8>(&mut values)? != 0;
            let blue_pinned = parse_next::<u8>(&mut values)? != 0;
            let left_red_pinned = parse_next::<u8>(&mut values)? != 0;
            let right_red_pinned = parse_next::<u8>(&mut values)? != 0;
            let helper_pinned = if version >= 10 {
                parse_next::<u8>(&mut values)? != 0
            } else {
                false
            };
            Some(SavedState {
                points,
                helper_point: helper_enabled.then_some(Point { x: helper_x, y: helper_y }),
                angle_locked,
                locked_signed_angle,
                red_locked_index: match red_lock_code {
                    1 => Some(0),
                    2 => Some(2),
                    _ => None,
                },
                bisector_visible,
                plus_degrees_visible,
                inverted,
                hypotenuse_visible,
                front_plus_visible,
                xtk_visible,
                distance_visible,
                meters_per_pixel,
                north_visible,
                north_angle,
                north_locked,
                course_visible,
                blue_pinned,
                left_red_pinned,
                right_red_pinned,
                helper_pinned,
                window_x: parse_next(&mut values)?,
                window_y: parse_next(&mut values)?,
            })
        }
        _ => None,
    }
}

fn clamp_line_length(vertex: Point, end: Point) -> Point {
    let dx = end.x - vertex.x;
    let dy = end.y - vertex.y;
    let len = (dx * dx + dy * dy).sqrt();
    if len <= MAX_LINE_LEN || len == 0.0 {
        return end;
    }
    let scale = MAX_LINE_LEN / len;
    Point {
        x: vertex.x + dx * scale,
        y: vertex.y + dy * scale,
    }
}

fn vector_angle(from: Point, to: Point) -> f32 {
    (to.y - from.y).atan2(to.x - from.x)
}

fn vector_length(from: Point, to: Point) -> f32 {
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    (dx * dx + dy * dy).sqrt()
}

fn normalize_signed_angle(angle: f32) -> f32 {
    let mut normalized = angle;
    while normalized > PI {
        normalized -= 2.0 * PI;
    }
    while normalized < -PI {
        normalized += 2.0 * PI;
    }
    normalized
}

/// Normalizes an angle to a clockwise screen-space bearing in the range [0, 2π).
/// In screen coordinates +Y points down, so increasing atan2 angles are clockwise.
fn normalize_bearing_angle(angle: f32) -> f32 {
    let full_turn = 2.0 * PI;
    let mut normalized = angle % full_turn;
    if normalized < 0.0 {
        normalized += full_turn;
    }
    normalized
}

fn bearing_degrees(angle: f32) -> i32 {
    (normalize_bearing_angle(angle).to_degrees().round() as i32).rem_euclid(360)
}

fn point_from_polar(vertex: Point, angle: f32, radius: f32) -> Point {
    Point {
        x: vertex.x + angle.cos() * radius,
        y: vertex.y + angle.sin() * radius,
    }
}

fn angle_bisector_direction(points: [Point; 3]) -> Option<(f32, f32)> {
    let a = points[0];
    let vertex = points[1];
    let b = points[2];
    let len_a = vector_length(vertex, a);
    let len_b = vector_length(vertex, b);
    if len_a < EPSILON || len_b < EPSILON {
        return None;
    }

    let ua = ((a.x - vertex.x) / len_a, (a.y - vertex.y) / len_a);
    let ub = ((b.x - vertex.x) / len_b, (b.y - vertex.y) / len_b);
    let mut bx = ua.0 + ub.0;
    let mut by = ua.1 + ub.1;
    let bisector_len = (bx * bx + by * by).sqrt();
    if bisector_len < EPSILON {
        bx = -ua.1;
        by = ua.0;
    } else {
        bx /= bisector_len;
        by /= bisector_len;
    }
    Some((bx, by))
}

fn foreground_line_color(inverted: bool) -> Color {
    if inverted {
        Color::from_rgba8(255, 255, 255, 235)
    } else {
        Color::from_rgba8(20, 20, 20, 220)
    }
}

fn lock_center(points: [Point; 3]) -> Point {
    let vertex = points[1];
    let (bx, by) = angle_bisector_direction(points).unwrap_or((0.0, 1.0));

    let mut dx = -bx;
    let mut dy = -by - 0.55;
    let len = (dx * dx + dy * dy).sqrt().max(EPSILON);
    dx /= len;
    dy /= len;

    Point {
        x: vertex.x + dx * LOCK_DISTANCE,
        y: vertex.y + dy * LOCK_DISTANCE,
    }
}

fn in_lock_button(point: Point, points: [Point; 3]) -> bool {
    let center = lock_center(points);
    let half = LOCK_PANEL_SIZE * 0.5 + 3.0;
    point.x >= center.x - half
        && point.x <= center.x + half
        && point.y >= center.y - half
        && point.y <= center.y + half
}

fn red_lock_center(points: [Point; 3], index: usize) -> Point {
    let vertex = points[1];
    let red = points[index];
    let other = points[if index == 0 { 2 } else { 0 }];
    let dx = red.x - vertex.x;
    let dy = red.y - vertex.y;
    let length = (dx * dx + dy * dy).sqrt().max(EPSILON);
    let ux = dx / length;
    let uy = dy / length;
    let other_x = other.x - vertex.x;
    let other_y = other.y - vertex.y;
    let cross = ux * other_y - uy * other_x;

    // Pick the normal on the exterior side of the main sector.
    let (nx, ny) = if cross >= 0.0 {
        (uy, -ux)
    } else {
        (-uy, ux)
    };

    Point {
        x: red.x + nx * RED_LOCK_DISTANCE,
        y: red.y + ny * RED_LOCK_DISTANCE,
    }
}

fn in_red_lock_button(point: Point, points: [Point; 3]) -> Option<usize> {
    let half = LOCK_PANEL_SIZE * 0.5 + 4.0;
    [0usize, 2usize].into_iter().find(|index| {
        let center = red_lock_center(points, *index);
        point.x >= center.x - half
            && point.x <= center.x + half
            && point.y >= center.y - half
            && point.y <= center.y + half
    })
}

fn red_point_at(point: Point, points: [Point; 3]) -> Option<usize> {
    let hit_radius = HANDLE_RADIUS + 6.0;
    [0usize, 2usize].into_iter().find(|index| {
        let dx = points[*index].x - point.x;
        let dy = points[*index].y - point.y;
        dx * dx + dy * dy <= hit_radius * hit_radius
    })
}

fn in_angle_label(point: Point, points: [Point; 3]) -> bool {
    let panel = label_panel_rect(points);
    point.x >= panel.x
        && point.x <= panel.x + panel.width
        && point.y >= panel.y
        && point.y <= panel.y + panel.height
}

fn in_helper_handle(point: Point, helper: Point) -> bool {
    let dx = point.x - helper.x;
    let dy = point.y - helper.y;
    dx * dx + dy * dy <= PLUS_HIT_RADIUS * PLUS_HIT_RADIUS
}

fn helper_lock_center(helper: Point) -> Point {
    Point {
        x: helper.x + HELPER_LOCK_OFFSET_X,
        y: helper.y + HELPER_LOCK_OFFSET_Y,
    }
}

fn in_helper_lock_button(point: Point, helper: Point) -> bool {
    let center = helper_lock_center(helper);
    let half = LOCK_PANEL_SIZE * 0.5 + 4.0;
    point.x >= center.x - half
        && point.x <= center.x + half
        && point.y >= center.y - half
        && point.y <= center.y + half
}

fn helper_bisector_foot(points: [Point; 3], helper: Point) -> Option<Point> {
    let vertex = points[1];
    let (bx, by) = angle_bisector_direction(points)?;
    let hx = helper.x - vertex.x;
    let hy = helper.y - vertex.y;
    let projection = hx * bx + hy * by;
    Some(Point {
        x: vertex.x + bx * projection,
        y: vertex.y + by * projection,
    })
}

fn stroke_segment(pixmap: &mut Pixmap, from: Point, to: Point, width: f32, color: Color) {
    let mut builder = PathBuilder::new();
    builder.move_to(from.x, from.y);
    builder.line_to(to.x, to.y);
    if let Some(path) = builder.finish() {
        let mut paint = Paint::default();
        paint.set_color(color);
        paint.anti_alias = true;
        let stroke = Stroke {
            width,
            ..Stroke::default()
        };
        pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }
}

fn draw_dashed_line(
    pixmap: &mut Pixmap,
    from: Point,
    to: Point,
    dash: f32,
    gap: f32,
    width: f32,
    color: Color,
    trim_start: f32,
    trim_end: f32,
) {
    let distance = vector_length(from, to);
    if distance <= trim_start + trim_end + EPSILON {
        return;
    }
    let angle = vector_angle(from, to);
    let end_limit = distance - trim_end;
    let mut position = trim_start;
    while position < end_limit {
        let segment_end = (position + dash).min(end_limit);
        stroke_segment(
            pixmap,
            point_from_polar(from, angle, position),
            point_from_polar(from, angle, segment_end),
            width,
            color,
        );
        position += dash + gap;
    }
}

fn bisector_hypotenuse_distance(points: [Point; 3]) -> Option<f32> {
    let vertex = points[1];
    let (bx, by) = angle_bisector_direction(points)?;
    let a = points[0];
    let b = points[2];
    let segment_x = b.x - a.x;
    let segment_y = b.y - a.y;
    let denominator = cross_2d(bx, by, segment_x, segment_y);
    if denominator.abs() < EPSILON {
        return None;
    }

    let av_x = a.x - vertex.x;
    let av_y = a.y - vertex.y;
    let distance = cross_2d(av_x, av_y, segment_x, segment_y) / denominator;
    let segment_position = cross_2d(av_x, av_y, bx, by) / denominator;
    if distance < 0.0 || !(-EPSILON..=1.0 + EPSILON).contains(&segment_position) {
        return None;
    }
    Some(distance)
}

fn displayed_bisector_length(points: [Point; 3], helper: Option<Point>) -> f32 {
    let vertex = points[1];
    if let Some(helper) = helper {
        // v2.7: while the green-plus ray is present, the bisector follows it
        // but remains slightly shorter. If the plus is inside the red triangle,
        // the ray is still extended at least to the red hypotenuse.
        let helper_length = vector_length(vertex, helper);
        let desired = (helper_length - 14.0).max(HANDLE_RADIUS + 6.0);
        let hypotenuse_minimum = bisector_hypotenuse_distance(points)
            .map(|distance| distance + 2.0)
            .unwrap_or(0.0);
        desired.max(hypotenuse_minimum).min(MAX_LINE_LEN)
    } else {
        vector_length(vertex, points[0])
            .min(vector_length(vertex, points[2]))
            * 0.88
    }
}

fn draw_dashed_bisector(
    pixmap: &mut Pixmap,
    points: [Point; 3],
    helper: Option<Point>,
    inverted: bool,
) {
    let vertex = points[1];
    let Some((bx, by)) = angle_bisector_direction(points) else {
        return;
    };

    let total = displayed_bisector_length(points, helper);
    let color = if helper.is_some() {
        Color::from_rgba8(235, 190, 34, 238)
    } else {
        foreground_line_color(inverted)
    };

    let dash = 6.0;
    let gap = 5.0;
    let mut distance = HANDLE_RADIUS + 3.0;
    while distance < total {
        let end_distance = (distance + dash).min(total);
        stroke_segment(
            pixmap,
            Point {
                x: vertex.x + bx * distance,
                y: vertex.y + by * distance,
            },
            Point {
                x: vertex.x + bx * end_distance,
                y: vertex.y + by * end_distance,
            },
            1.15,
            color,
        );
        distance += dash + gap;
    }
}


fn north_arc_geometry(points: [Point; 3], north_angle: f32) -> Option<(f32, f32, f32, f32)> {
    let vertex = points[1];
    let (bx, by) = angle_bisector_direction(points)?;
    let bisector_angle = by.atan2(bx);
    let red_length = vector_length(vertex, points[0])
        .min(vector_length(vertex, points[2]));
    let radius = (red_length * 0.32).clamp(NORTH_ARC_MIN_RADIUS, NORTH_ARC_MAX_RADIUS);

    // Full clockwise bearing from the user-positioned North arrow to the bisector.
    let sweep = normalize_bearing_angle(bisector_angle - north_angle);
    let mut mid_angle = north_angle + sweep * 0.5;
    if sweep < 0.12 {
        // Keep the 0° label beside the arrow instead of directly on top of it.
        mid_angle += 0.20;
    }
    Some((bisector_angle, sweep, mid_angle, radius))
}

fn panels_overlap_with_margin(a: PanelRect, b: PanelRect, margin: f32) -> bool {
    a.x < b.x + b.width + margin
        && a.x + a.width + margin > b.x
        && a.y < b.y + b.height + margin
        && a.y + a.height + margin > b.y
}

fn north_angle_label_rect(
    points: [Point; 3],
    north_angle: f32,
    course_panel: Option<PanelRect>,
) -> Option<PanelRect> {
    let vertex = points[1];
    let (_bisector_angle, sweep, mid_angle, radius) = north_arc_geometry(points, north_angle)?;
    let angle_text = format!("{}°", bearing_degrees(sweep));
    let mut label_distance = radius + NORTH_LABEL_OFFSET;

    // The blue North label is the one that moves outward. This guarantees that
    // it does not cover the green course label, while keeping the course label
    // close to its green arc.
    for _ in 0..16 {
        let center = point_from_polar(vertex, mid_angle, label_distance);
        let panel = text_panel_rect(&angle_text, center.x, center.y);
        if course_panel
            .map(|other| !panels_overlap_with_margin(panel, other, 5.0))
            .unwrap_or(true)
        {
            return Some(panel);
        }
        label_distance += 7.0;
    }

    let center = point_from_polar(vertex, mid_angle, label_distance);
    Some(text_panel_rect(&angle_text, center.x, center.y))
}

fn in_north_angle_label(
    point: Point,
    points: [Point; 3],
    north_angle: f32,
    course_panel: Option<PanelRect>,
) -> bool {
    let Some(panel) = north_angle_label_rect(points, north_angle, course_panel) else {
        return false;
    };
    point.x >= panel.x
        && point.x <= panel.x + panel.width
        && point.y >= panel.y
        && point.y <= panel.y + panel.height
}

fn draw_bearing_arc(
    pixmap: &mut Pixmap,
    center: Point,
    radius: f32,
    start_angle: f32,
    clockwise_sweep: f32,
    width: f32,
    color: Color,
) {
    if radius <= 0.0 || clockwise_sweep <= 0.001 {
        return;
    }

    let steps = ((clockwise_sweep * radius) / 8.0).ceil().max(8.0) as usize;
    let mut builder = PathBuilder::new();
    for step in 0..=steps {
        let t = step as f32 / steps as f32;
        let angle = start_angle + clockwise_sweep * t;
        let point = point_from_polar(center, angle, radius);
        if step == 0 {
            builder.move_to(point.x, point.y);
        } else {
            builder.line_to(point.x, point.y);
        }
    }

    if let Some(path) = builder.finish() {
        let mut paint = Paint::default();
        paint.set_color(color);
        paint.anti_alias = true;
        pixmap.stroke_path(
            &path,
            &paint,
            &Stroke {
                width,
                ..Stroke::default()
            },
            Transform::identity(),
            None,
        );
    }
}

fn north_arrow_tip(vertex: Point, north_angle: f32) -> Point {
    point_from_polar(vertex, north_angle, NORTH_ARROW_LENGTH)
}

fn north_text_center(vertex: Point, north_angle: f32) -> Point {
    point_from_polar(vertex, north_angle, NORTH_ARROW_LENGTH + 15.0)
}

fn north_lock_center(vertex: Point, north_angle: f32) -> Point {
    // The North lock stays immediately to the screen-right of the upright N,
    // regardless of the arrow direction.
    let n_center = north_text_center(vertex, north_angle);
    let layout = layout_text("N", NORTH_TEXT_SIZE);
    Point {
        x: n_center.x + layout.width * 0.5 + NORTH_LOCK_GAP + LOCK_PANEL_SIZE * 0.5,
        y: n_center.y,
    }
}

fn in_north_lock_button(point: Point, vertex: Point, north_angle: f32) -> bool {
    let center = north_lock_center(vertex, north_angle);
    let half = LOCK_PANEL_SIZE * 0.5 + 4.0;
    point.x >= center.x - half
        && point.x <= center.x + half
        && point.y >= center.y - half
        && point.y <= center.y + half
}

fn in_north_handle(point: Point, vertex: Point, north_angle: f32) -> bool {
    let tip = north_arrow_tip(vertex, north_angle);
    let dx = point.x - tip.x;
    let dy = point.y - tip.y;
    dx * dx + dy * dy <= NORTH_HANDLE_HIT_RADIUS * NORTH_HANDLE_HIT_RADIUS
}

fn draw_centered_text(
    pixmap: &mut Pixmap,
    text: &str,
    center: Point,
    size: f32,
    color: Color,
) {
    let layout = layout_text(text, size);
    let text_x = center.x - (layout.xmin + layout.xmax) * 0.5;
    let baseline = center.y + (layout.ymin + layout.ymax) * 0.5;
    draw_text(pixmap, text, text_x, baseline, size, color);
}

fn draw_north_overlay(
    pixmap: &mut Pixmap,
    points: [Point; 3],
    bisector_visible: bool,
    inverted: bool,
    north_angle: f32,
    north_locked: bool,
    show_angle_label: bool,
    course_panel: Option<PanelRect>,
) {
    let vertex = points[1];
    let line_color = foreground_line_color(inverted);
    let arrow_tip = north_arrow_tip(vertex, north_angle);
    let arrow_start = point_from_polar(vertex, north_angle, HANDLE_RADIUS + 4.0);

    stroke_segment(pixmap, arrow_start, arrow_tip, 1.55, line_color);

    // Conventional magnetic-north arrowhead.
    let left = point_from_polar(
        arrow_tip,
        north_angle + PI - 0.48,
        NORTH_ARROW_HEAD,
    );
    let right = point_from_polar(
        arrow_tip,
        north_angle + PI + 0.48,
        NORTH_ARROW_HEAD,
    );
    stroke_segment(pixmap, arrow_tip, left, 1.55, line_color);
    stroke_segment(pixmap, arrow_tip, right, 1.55, line_color);

    let n_center = north_text_center(vertex, north_angle);
    draw_centered_text(
        pixmap,
        "N",
        n_center,
        NORTH_TEXT_SIZE,
        line_color,
    );

    let open_color = if inverted {
        Color::from_rgba8(255, 255, 255, 235)
    } else {
        Color::from_rgba8(45, 45, 45, 235)
    };
    draw_lock_at(
        pixmap,
        north_lock_center(vertex, north_angle),
        north_locked,
        false,
        Color::from_rgba8(35, 112, 225, 255),
        open_color,
    );

    if !bisector_visible {
        return;
    }

    let Some((_bisector_angle, sweep, mid_angle, radius)) =
        north_arc_geometry(points, north_angle)
    else {
        return;
    };
    let arc_color = Color::from_rgba8(54, 123, 235, 242);
    draw_bearing_arc(
        pixmap,
        vertex,
        radius,
        north_angle,
        sweep,
        1.65,
        arc_color,
    );

    if show_angle_label {
        let angle_text = format!("{}°", bearing_degrees(sweep));
        if let Some(panel) = north_angle_label_rect(points, north_angle, course_panel) {
            draw_text_panel(
                pixmap,
                &angle_text,
                panel.x + panel.width * 0.5,
                panel.y + panel.height * 0.5,
                Color::from_rgba8(218, 233, 255, 188),
                Color::from_rgba8(28, 87, 190, 252),
            );
        }
    }
}

fn north_overlay_bounds(
    points: [Point; 3],
    bisector_visible: bool,
    north_angle: f32,
    course_panel: Option<PanelRect>,
) -> ContentBounds {
    let vertex = points[1];
    let arrow_tip = north_arrow_tip(vertex, north_angle);
    let n_center = north_text_center(vertex, north_angle);
    let lock = north_lock_center(vertex, north_angle);
    let mut bounds = ContentBounds {
        min_x: vertex.x.min(arrow_tip.x).min(n_center.x).min(lock.x) - 18.0,
        min_y: vertex.y.min(arrow_tip.y).min(n_center.y).min(lock.y) - 18.0,
        max_x: vertex.x.max(arrow_tip.x).max(n_center.x).max(lock.x) + 18.0,
        max_y: vertex.y.max(arrow_tip.y).max(n_center.y).max(lock.y) + 18.0,
    };

    if bisector_visible {
        if let Some((_bisector_angle, sweep, mid_angle, radius)) =
            north_arc_geometry(points, north_angle)
        {
            include_bearing_arc_in_bounds(
                &mut bounds,
                vertex,
                radius,
                north_angle,
                sweep,
            );
            let panel = north_angle_label_rect(points, north_angle, course_panel)
                .unwrap_or_else(|| {
                    let angle_text = format!("{}°", bearing_degrees(sweep));
                    let label_center =
                        point_from_polar(vertex, mid_angle, radius + NORTH_LABEL_OFFSET);
                    text_panel_rect(&angle_text, label_center.x, label_center.y)
                });
            bounds.min_x = bounds.min_x.min(panel.x);
            bounds.min_y = bounds.min_y.min(panel.y);
            bounds.max_x = bounds.max_x.max(panel.x + panel.width);
            bounds.max_y = bounds.max_y.max(panel.y + panel.height);
        }
    }

    bounds
}

fn course_arc_geometry(vertex: Point, helper: Point, north_angle: f32) -> Option<(f32, f32, f32)> {
    let helper_length = vector_length(vertex, helper);
    if helper_length < EPSILON {
        return None;
    }
    let helper_angle = vector_angle(vertex, helper);
    let sweep = normalize_bearing_angle(helper_angle - north_angle);
    let mut mid_angle = north_angle + sweep * 0.5;
    if sweep < 0.12 {
        mid_angle += 0.20;
    }
    let radius = (helper_length * 0.30).clamp(COURSE_ARC_MIN_RADIUS, COURSE_ARC_MAX_RADIUS);
    Some((sweep, mid_angle, radius))
}

fn course_angle_label_rect(vertex: Point, helper: Point, north_angle: f32) -> Option<PanelRect> {
    let (sweep, mid_angle, radius) = course_arc_geometry(vertex, helper, north_angle)?;
    let angle_text = format!("{}°", bearing_degrees(sweep));
    let center = point_from_polar(vertex, mid_angle, radius + COURSE_LABEL_OFFSET);
    Some(text_panel_rect(&angle_text, center.x, center.y))
}

fn in_course_angle_label(point: Point, vertex: Point, helper: Point, north_angle: f32) -> bool {
    let Some(panel) = course_angle_label_rect(vertex, helper, north_angle) else {
        return false;
    };
    point.x >= panel.x
        && point.x <= panel.x + panel.width
        && point.y >= panel.y
        && point.y <= panel.y + panel.height
}

fn draw_course_overlay(
    pixmap: &mut Pixmap,
    vertex: Point,
    helper: Point,
    north_angle: f32,
    show_angle_label: bool,
) {
    let Some((sweep, mid_angle, radius)) = course_arc_geometry(vertex, helper, north_angle) else {
        return;
    };
    let arc_color = Color::from_rgba8(48, 205, 88, 235);
    draw_bearing_arc(pixmap, vertex, radius, north_angle, sweep, 1.65, arc_color);
    if show_angle_label {
        let angle_text = format!("{}°", bearing_degrees(sweep));
        let label_center = point_from_polar(vertex, mid_angle, radius + COURSE_LABEL_OFFSET);
        draw_text_panel(
            pixmap,
            &angle_text,
            label_center.x,
            label_center.y,
            Color::from_rgba8(225, 255, 225, 188),
            Color::from_rgba8(22, 132, 42, 252),
        );
    }
}

fn course_overlay_bounds(vertex: Point, helper: Point, north_angle: f32) -> ContentBounds {
    let mut bounds = ContentBounds {
        min_x: vertex.x.min(helper.x) - 20.0,
        min_y: vertex.y.min(helper.y) - 20.0,
        max_x: vertex.x.max(helper.x) + 20.0,
        max_y: vertex.y.max(helper.y) + 20.0,
    };
    if let Some((sweep, mid_angle, radius)) = course_arc_geometry(vertex, helper, north_angle) {
        include_bearing_arc_in_bounds(&mut bounds, vertex, radius, north_angle, sweep);
        let angle_text = format!("{}°", bearing_degrees(sweep));
        let label_center = point_from_polar(vertex, mid_angle, radius + COURSE_LABEL_OFFSET);
        let panel = text_panel_rect(&angle_text, label_center.x, label_center.y);
        bounds.min_x = bounds.min_x.min(panel.x);
        bounds.min_y = bounds.min_y.min(panel.y);
        bounds.max_x = bounds.max_x.max(panel.x + panel.width);
        bounds.max_y = bounds.max_y.max(panel.y + panel.height);
    }
    bounds
}

fn draw_lock_at(
    pixmap: &mut Pixmap,
    center: Point,
    locked: bool,
    pinned: bool,
    locked_color: Color,
    open_color: Color,
) {
    let panel_color = if pinned {
        Color::from_rgba8(255, 235, 120, 205)
    } else {
        Color::from_rgba8(255, 255, 255, 155)
    };
    fill_rounded_rect(
        pixmap,
        center.x - LOCK_PANEL_SIZE * 0.5,
        center.y - LOCK_PANEL_SIZE * 0.5,
        LOCK_PANEL_SIZE,
        LOCK_PANEL_SIZE,
        LOCK_PANEL_RADIUS,
        panel_color,
    );

    let effective_locked = locked || pinned;
    let icon_color = if pinned {
        Color::from_rgba8(245, 190, 18, 255)
    } else if locked {
        locked_color
    } else {
        open_color
    };

    if let Some(body) = Rect::from_xywh(center.x - 3.0, center.y - 0.5, 6.0, 5.0) {
        let mut paint = Paint::default();
        paint.set_color(icon_color);
        pixmap.fill_rect(body, &paint, Transform::identity(), None);
    }

    let right_x = if effective_locked { center.x + 2.5 } else { center.x + 4.0 };
    let mut builder = PathBuilder::new();
    builder.move_to(center.x - 2.5, center.y - 0.5);
    builder.line_to(center.x - 2.5, center.y - 3.0);
    builder.cubic_to(
        center.x - 2.5,
        center.y - 6.8,
        right_x,
        center.y - 6.8,
        right_x,
        center.y - 3.0,
    );
    if effective_locked {
        builder.line_to(right_x, center.y - 0.5);
    }
    if let Some(path) = builder.finish() {
        let mut paint = Paint::default();
        paint.set_color(icon_color);
        paint.anti_alias = true;
        pixmap.stroke_path(
            &path,
            &paint,
            &Stroke {
                width: 1.5,
                ..Stroke::default()
            },
            Transform::identity(),
            None,
        );
    }

    stroke_segment(
        pixmap,
        Point {
            x: center.x,
            y: center.y + 1.0,
        },
        Point {
            x: center.x,
            y: center.y + 3.0,
        },
        1.0,
        Color::from_rgba8(255, 255, 255, 245),
    );
}

fn draw_lock_icon(
    pixmap: &mut Pixmap,
    points: [Point; 3],
    locked: bool,
    pinned: bool,
    inverted: bool,
) {
    let open_color = if inverted {
        Color::from_rgba8(245, 245, 245, 245)
    } else {
        Color::from_rgba8(45, 45, 45, 235)
    };
    draw_lock_at(
        pixmap,
        lock_center(points),
        locked,
        pinned,
        Color::from_rgba8(32, 105, 218, 255),
        open_color,
    );
}

fn draw_red_lock_icons(
    pixmap: &mut Pixmap,
    points: [Point; 3],
    locked_index: Option<usize>,
    left_pinned: bool,
    right_pinned: bool,
    inverted: bool,
) {
    let open_color = if inverted {
        Color::from_rgba8(255, 225, 225, 245)
    } else {
        Color::from_rgba8(110, 45, 45, 235)
    };
    for index in [0usize, 2usize] {
        draw_lock_at(
            pixmap,
            red_lock_center(points, index),
            locked_index == Some(index),
            if index == 0 { left_pinned } else { right_pinned },
            Color::from_rgba8(224, 55, 55, 255),
            open_color,
        );
    }
}

fn draw_helper_lock_icon(
    pixmap: &mut Pixmap,
    helper: Point,
    pinned: bool,
    inverted: bool,
) {
    let open_color = if inverted {
        Color::from_rgba8(220, 255, 228, 245)
    } else {
        Color::from_rgba8(28, 135, 62, 245)
    };
    // The helper lock is green when the plus is fixed on screen. Unlike the
    // blue/red absolute pins, it deliberately does not use the global yellow
    // pinned palette.
    draw_lock_at(
        pixmap,
        helper_lock_center(helper),
        pinned,
        false,
        Color::from_rgba8(48, 205, 88, 255),
        open_color,
    );
}

fn draw_dash_dot_line(
    pixmap: &mut Pixmap,
    from: Point,
    to: Point,
    color: Color,
    trim_start: f32,
    trim_end: f32,
) {
    let distance = vector_length(from, to);
    if distance <= trim_start + trim_end + EPSILON {
        return;
    }

    let angle = vector_angle(from, to);
    let dash_length = 8.0;
    let gap = 3.0;
    let dot_radius = 1.35;
    let mut position = trim_start;
    let end_limit = distance - trim_end;

    while position < end_limit {
        let dash_end = (position + dash_length).min(end_limit);
        stroke_segment(
            pixmap,
            point_from_polar(from, angle, position),
            point_from_polar(from, angle, dash_end),
            1.15,
            color,
        );

        let dot_center_distance = dash_end + gap + dot_radius;
        if dot_center_distance + dot_radius <= end_limit {
            let dot_center = point_from_polar(from, angle, dot_center_distance);
            let mut dot = PathBuilder::new();
            dot.push_circle(dot_center.x, dot_center.y, dot_radius);
            if let Some(path) = dot.finish() {
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
        }

        position = dot_center_distance + dot_radius + gap;
    }
}

fn draw_dash_dot_helper_line(
    pixmap: &mut Pixmap,
    vertex: Point,
    helper: Point,
    inverted: bool,
) {
    draw_dash_dot_line(
        pixmap,
        vertex,
        helper,
        foreground_line_color(inverted),
        HANDLE_RADIUS + 3.0,
        PLUS_HIT_RADIUS * 0.6,
    );
}

fn cross_2d(ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    ax * by - ay * bx
}

/// Intersects the ray V->helper with the red hypotenuse segment A-B.
/// The returned scalar uses V + t * (helper - V), so t=1 is the plus itself.
fn helper_hypotenuse_intersection(points: [Point; 3], helper: Point) -> Option<(Point, f32)> {
    let vertex = points[1];
    let ray_x = helper.x - vertex.x;
    let ray_y = helper.y - vertex.y;
    if ray_x * ray_x + ray_y * ray_y < EPSILON * EPSILON {
        return None;
    }

    let a = points[0];
    let b = points[2];
    let seg_x = b.x - a.x;
    let seg_y = b.y - a.y;
    let denominator = cross_2d(ray_x, ray_y, seg_x, seg_y);
    if denominator.abs() < EPSILON {
        return None;
    }

    let av_x = a.x - vertex.x;
    let av_y = a.y - vertex.y;
    let t = cross_2d(av_x, av_y, seg_x, seg_y) / denominator;
    let u = cross_2d(av_x, av_y, ray_x, ray_y) / denominator;
    if t < 0.0 || !(-EPSILON..=1.0 + EPSILON).contains(&u) {
        return None;
    }

    Some((
        Point {
            x: vertex.x + ray_x * t,
            y: vertex.y + ray_y * t,
        },
        t,
    ))
}

#[derive(Clone, Copy, Debug)]
struct OutsideAngleInfo {
    boundary_angle: f32,
    helper_angle: f32,
    mid_angle: f32,
    delta_radians: f32,
}

/// Returns the angular overrun when the plus ray leaves the smaller red sector.
fn helper_outside_angle(points: [Point; 3], helper: Point) -> Option<OutsideAngleInfo> {
    let vertex = points[1];
    let angle_a = vector_angle(vertex, points[0]);
    let angle_b = vector_angle(vertex, points[2]);
    let angle_h = vector_angle(vertex, helper);
    let main = normalize_signed_angle(angle_b - angle_a);
    let sweep = main.abs();
    if sweep < EPSILON {
        return None;
    }

    let orientation = if main >= 0.0 { 1.0 } else { -1.0 };
    let directed = normalize_signed_angle(orientation * normalize_signed_angle(angle_h - angle_a));
    if directed >= -EPSILON && directed <= sweep + EPSILON {
        return None;
    }

    let delta_a = normalize_signed_angle(angle_h - angle_a).abs();
    let delta_b = normalize_signed_angle(angle_h - angle_b).abs();
    let (boundary_angle, delta_radians) = if delta_a <= delta_b {
        (angle_a, delta_a)
    } else {
        (angle_b, delta_b)
    };
    let helper_delta = normalize_signed_angle(angle_h - boundary_angle);

    Some(OutsideAngleInfo {
        boundary_angle,
        helper_angle: angle_h,
        mid_angle: boundary_angle + helper_delta * 0.5,
        delta_radians,
    })
}

fn helper_arc_geometry(points: [Point; 3], helper: Point) -> (f32, f32, f32, f32, f32, f32) {
    let vertex = points[1];
    let angle_a = vector_angle(vertex, points[0]);
    let angle_h = vector_angle(vertex, helper);
    let angle_b = vector_angle(vertex, points[2]);
    let delta_ah = normalize_signed_angle(angle_h - angle_a);
    let delta_hb = normalize_signed_angle(angle_b - angle_h);
    let mid_ah = angle_a + delta_ah * 0.5;
    let mid_hb = angle_h + delta_hb * 0.5;

    // v1.7: the helper arcs are anchored to the centre of the V->plus line.
    let helper_length = vector_length(vertex, helper);
    let radius = (helper_length * 0.5).max(14.0);
    (angle_a, angle_h, angle_b, mid_ah, mid_hb, radius)
}

/// Geometry for the yellow bisector-to-plus measurement introduced in v2.5.
/// The double arc sits halfway between the existing helper arc and the plus.
fn helper_bisector_arc_geometry(
    points: [Point; 3],
    helper: Point,
) -> Option<(f32, f32, f32, f32, f32)> {
    let vertex = points[1];
    let (bx, by) = angle_bisector_direction(points)?;
    let bisector_angle = by.atan2(bx);
    let helper_angle = vector_angle(vertex, helper);
    let signed_delta = normalize_signed_angle(helper_angle - bisector_angle);
    let mid_angle = bisector_angle + signed_delta * 0.5;

    let helper_length = vector_length(vertex, helper);
    if helper_length < EPSILON {
        return None;
    }
    let green_radius = (helper_length * 0.5).max(14.0);
    let radius = (green_radius + helper_length) * 0.5;

    Some((
        bisector_angle,
        helper_angle,
        mid_angle,
        radius,
        signed_delta.abs(),
    ))
}

fn point_in_panel(point: Point, panel: PanelRect) -> bool {
    point.x >= panel.x
        && point.x <= panel.x + panel.width
        && point.y >= panel.y
        && point.y <= panel.y + panel.height
}

fn helper_angle_panel_rects(points: [Point; 3], helper: Point) -> (PanelRect, PanelRect) {
    let vertex = points[1];
    let (_, _, _, mid_left, mid_right, radius) = helper_arc_geometry(points, helper);
    let left_text = format!(
        "{}°",
        angle_between(points[0], vertex, helper).round() as i32
    );
    let right_text = format!(
        "{}°",
        angle_between(helper, vertex, points[2]).round() as i32
    );
    let left_center = point_from_polar(vertex, mid_left, radius);
    let right_center = point_from_polar(vertex, mid_right, radius);
    (
        text_panel_rect(&left_text, left_center.x, left_center.y),
        text_panel_rect(&right_text, right_center.x, right_center.y),
    )
}

fn helper_yellow_panel_rect(points: [Point; 3], helper: Point) -> Option<PanelRect> {
    let vertex = points[1];
    let (_, _, mid_angle, radius, delta) = helper_bisector_arc_geometry(points, helper)?;
    let text = format!("{}°", delta.to_degrees().round() as i32);
    let center = point_from_polar(vertex, mid_angle, radius);
    Some(text_panel_rect(&text, center.x, center.y))
}

fn helper_delta_panel_rect(points: [Point; 3], helper: Point) -> Option<PanelRect> {
    let vertex = points[1];
    let outside = helper_outside_angle(points, helper)?;
    let (_, _, _, _, _, radius) = helper_arc_geometry(points, helper);
    let text = format!("Δ {}°", outside.delta_radians.to_degrees().round() as i32);
    let center = point_from_polar(vertex, outside.mid_angle, radius + 28.0);
    Some(text_panel_rect(&text, center.x, center.y))
}

fn include_point_in_bounds(bounds: &mut ContentBounds, point: Point, padding: f32) {
    bounds.min_x = bounds.min_x.min(point.x - padding);
    bounds.min_y = bounds.min_y.min(point.y - padding);
    bounds.max_x = bounds.max_x.max(point.x + padding);
    bounds.max_y = bounds.max_y.max(point.y + padding);
}

fn include_arc_in_bounds(
    bounds: &mut ContentBounds,
    center: Point,
    radius: f32,
    start_angle: f32,
    end_angle: f32,
) {
    let delta = normalize_signed_angle(end_angle - start_angle);
    for step in 0..=24 {
        let t = step as f32 / 24.0;
        include_point_in_bounds(
            bounds,
            point_from_polar(center, start_angle + delta * t, radius),
            4.0,
        );
    }
}

fn include_bearing_arc_in_bounds(
    bounds: &mut ContentBounds,
    center: Point,
    radius: f32,
    start_angle: f32,
    clockwise_sweep: f32,
) {
    let steps = ((clockwise_sweep / (PI / 24.0)).ceil() as usize).max(1);
    for step in 0..=steps {
        let t = step as f32 / steps as f32;
        include_point_in_bounds(
            bounds,
            point_from_polar(center, start_angle + clockwise_sweep * t, radius),
            4.0,
        );
    }
}

fn helper_overlay_bounds(
    points: [Point; 3],
    helper: Point,
    plus_degrees_visible: bool,
    bisector_visible: bool,
) -> ContentBounds {
    let vertex = points[1];
    let mut bounds = ContentBounds {
        min_x: helper.x.min(vertex.x) - PLUS_HIT_RADIUS,
        min_y: helper.y.min(vertex.y) - PLUS_HIT_RADIUS,
        max_x: helper.x.max(vertex.x) + PLUS_HIT_RADIUS,
        max_y: helper.y.max(vertex.y) + PLUS_HIT_RADIUS,
    };

    if bisector_visible {
        if let Some((bx, by)) = angle_bisector_direction(points) {
            let length = displayed_bisector_length(points, Some(helper));
            include_point_in_bounds(
                &mut bounds,
                Point {
                    x: vertex.x + bx * length,
                    y: vertex.y + by * length,
                },
                5.0,
            );
        }
    }

    let (angle_a, angle_h, angle_b, mid1, mid2, radius) =
        helper_arc_geometry(points, helper);
    let outside = helper_outside_angle(points, helper);
    if let Some(outside_info) = outside {
        let (_, end) = delta_boundary_extension(points, helper, outside_info);
        include_point_in_bounds(&mut bounds, end, 5.0);
    }
    if plus_degrees_visible {
        include_arc_in_bounds(&mut bounds, vertex, radius, angle_a, angle_h);
        include_arc_in_bounds(&mut bounds, vertex, radius, angle_h, angle_b);
        if bisector_visible {
            if let Some((bisector_angle, helper_angle, _, yellow_radius, _)) =
                helper_bisector_arc_geometry(points, helper)
            {
                include_arc_in_bounds(
                    &mut bounds,
                    vertex,
                    yellow_radius - 2.0,
                    bisector_angle,
                    helper_angle,
                );
                include_arc_in_bounds(
                    &mut bounds,
                    vertex,
                    yellow_radius + 2.0,
                    bisector_angle,
                    helper_angle,
                );
            }
        }
    } else if let Some(outside) = outside {
        include_arc_in_bounds(
            &mut bounds,
            vertex,
            radius,
            outside.boundary_angle,
            outside.helper_angle,
        );
    }

    if let Some((intersection, t)) = helper_hypotenuse_intersection(points, helper) {
        include_point_in_bounds(&mut bounds, intersection, 5.0);
        if t < 1.0 - EPSILON {
            let helper_distance = vector_length(vertex, helper);
            for red in [points[0], points[2]] {
                let red_angle = vector_angle(vertex, red);
                let red_distance = vector_length(vertex, red);
                let extension_distance = helper_distance.max(red_distance + 40.0);
                include_point_in_bounds(
                    &mut bounds,
                    point_from_polar(vertex, red_angle, extension_distance),
                    4.0,
                );
            }
        }
    }

    if plus_degrees_visible {
        let text1 = format!("{}°", angle_between(points[0], vertex, helper).round() as i32);
        let text2 = format!("{}°", angle_between(helper, vertex, points[2]).round() as i32);
        let center1 = point_from_polar(vertex, mid1, radius);
        let center2 = point_from_polar(vertex, mid2, radius);
        for panel in [
            text_panel_rect(&text1, center1.x, center1.y),
            text_panel_rect(&text2, center2.x, center2.y),
        ] {
            bounds.min_x = bounds.min_x.min(panel.x);
            bounds.min_y = bounds.min_y.min(panel.y);
            bounds.max_x = bounds.max_x.max(panel.x + panel.width);
            bounds.max_y = bounds.max_y.max(panel.y + panel.height);
        }

        if bisector_visible {
            if let Some((_, _, yellow_mid, yellow_radius, yellow_delta)) =
                helper_bisector_arc_geometry(points, helper)
            {
                let yellow_text = format!("{}°", yellow_delta.to_degrees().round() as i32);
                let yellow_center = point_from_polar(vertex, yellow_mid, yellow_radius);
                let panel = text_panel_rect(&yellow_text, yellow_center.x, yellow_center.y);
                bounds.min_x = bounds.min_x.min(panel.x);
                bounds.min_y = bounds.min_y.min(panel.y);
                bounds.max_x = bounds.max_x.max(panel.x + panel.width);
                bounds.max_y = bounds.max_y.max(panel.y + panel.height);
            }
        }
    }

    if let Some(outside) = outside {
        let delta_text = format!("Δ {}°", outside.delta_radians.to_degrees().round() as i32);
        let delta_center = point_from_polar(vertex, outside.mid_angle, radius + 28.0);
        let panel = text_panel_rect(&delta_text, delta_center.x, delta_center.y);
        bounds.min_x = bounds.min_x.min(panel.x);
        bounds.min_y = bounds.min_y.min(panel.y);
        bounds.max_x = bounds.max_x.max(panel.x + panel.width);
        bounds.max_y = bounds.max_y.max(panel.y + panel.height);
    }

    bounds
}

fn merge_bounds(a: ContentBounds, b: ContentBounds) -> ContentBounds {
    ContentBounds {
        min_x: a.min_x.min(b.min_x),
        min_y: a.min_y.min(b.min_y),
        max_x: a.max_x.max(b.max_x),
        max_y: a.max_y.max(b.max_y),
    }
}

fn draw_red_ray_extensions(pixmap: &mut Pixmap, points: [Point; 3], helper: Point) {
    let vertex = points[1];
    let helper_distance = vector_length(vertex, helper);
    let color = Color::from_rgba8(235, 50, 50, 225);

    for red in [points[0], points[2]] {
        let red_angle = vector_angle(vertex, red);
        let red_distance = vector_length(vertex, red);
        let extension_distance = helper_distance.max(red_distance + 40.0);
        let start_distance = red_distance + HANDLE_RADIUS + 2.0;
        if extension_distance <= start_distance + EPSILON {
            continue;
        }
        draw_dashed_line(
            pixmap,
            point_from_polar(vertex, red_angle, start_distance),
            point_from_polar(vertex, red_angle, extension_distance),
            7.0,
            5.0,
            1.35,
            color,
            0.0,
            0.0,
        );
    }
}

fn delta_boundary_extension(
    points: [Point; 3],
    helper: Point,
    outside: OutsideAngleInfo,
) -> (Point, Point) {
    let vertex = points[1];
    let left_angle = vector_angle(vertex, points[0]);
    let right_angle = vector_angle(vertex, points[2]);
    let red = if normalize_signed_angle(outside.boundary_angle - left_angle).abs()
        <= normalize_signed_angle(outside.boundary_angle - right_angle).abs()
    {
        points[0]
    } else {
        points[2]
    };
    let red_angle = vector_angle(vertex, red);
    let red_distance = vector_length(vertex, red);
    let helper_distance = vector_length(vertex, helper);
    let (_, _, _, _, _, delta_radius) = helper_arc_geometry(points, helper);
    let start_distance = red_distance + HANDLE_RADIUS + 2.0;
    let end_distance = helper_distance
        .max(delta_radius + 42.0)
        .max(red_distance + 58.0);
    (
        point_from_polar(vertex, red_angle, start_distance),
        point_from_polar(vertex, red_angle, end_distance),
    )
}

fn draw_red_delta_boundary_extension(
    pixmap: &mut Pixmap,
    points: [Point; 3],
    helper: Point,
    outside: OutsideAngleInfo,
) {
    let (start, end) = delta_boundary_extension(points, helper, outside);
    draw_dashed_line(
        pixmap,
        start,
        end,
        7.0,
        5.0,
        1.35,
        Color::from_rgba8(235, 50, 50, 225),
        0.0,
        0.0,
    );
}

fn draw_helper_overlay(
    pixmap: &mut Pixmap,
    points: [Point; 3],
    helper: Point,
    plus_degrees_visible: bool,
    bisector_visible: bool,
    hidden_panels: HiddenAnglePanels,
    inverted: bool,
) {
    let vertex = points[1];
    draw_dash_dot_helper_line(pixmap, vertex, helper, inverted);

    if let Some((intersection, t)) = helper_hypotenuse_intersection(points, helper) {
        if t > 1.0 + EPSILON {
            // The plus is before the hypotenuse: continue its sight line in green.
            draw_dash_dot_line(
                pixmap,
                helper,
                intersection,
                Color::from_rgba8(48, 205, 88, 235),
                PLUS_HIT_RADIUS * 0.65,
                2.0,
            );
        } else if t < 1.0 - EPSILON {
            // The plus has crossed the hypotenuse: extend both red boundary rays.
            draw_red_ray_extensions(pixmap, points, helper);
        }
    }

    let (angle_a, angle_h, angle_b, mid1, mid2, radius) =
        helper_arc_geometry(points, helper);
    let outside = helper_outside_angle(points, helper);
    if let Some(outside_info) = outside {
        // Keep the nearest red boundary ray visible even after the helper ray
        // leaves the main sector. The extension grows enough to contain the
        // red delta arc between the boundary and the helper line.
        draw_red_delta_boundary_extension(pixmap, points, helper, outside_info);
    }
    if plus_degrees_visible {
        let arc_color = if outside.is_some() {
            Color::from_rgba8(235, 62, 62, 235)
        } else {
            Color::from_rgba8(48, 205, 88, 230)
        };
        draw_arc(pixmap, vertex, radius, angle_a, angle_h, 1.5, arc_color);
        draw_arc(pixmap, vertex, radius, angle_h, angle_b, 1.5, arc_color);

        if bisector_visible {
            if let Some((bisector_angle, helper_angle, _, yellow_radius, _)) =
                helper_bisector_arc_geometry(points, helper)
            {
                let yellow = Color::from_rgba8(235, 190, 34, 238);
                // A pair of close concentric strokes makes the new measurement
                // visually distinct without making it heavy.
                draw_arc(
                    pixmap,
                    vertex,
                    yellow_radius - 2.0,
                    bisector_angle,
                    helper_angle,
                    1.15,
                    yellow,
                );
                draw_arc(
                    pixmap,
                    vertex,
                    yellow_radius + 2.0,
                    bisector_angle,
                    helper_angle,
                    1.15,
                    yellow,
                );
            }
        }
    } else if let Some(outside) = outside {
        // When “Градуси +” is off, ordinary green helper arcs are hidden.
        // Only the red angular overrun and its delta label remain.
        draw_arc(
            pixmap,
            vertex,
            radius,
            outside.boundary_angle,
            outside.helper_angle,
            1.5,
            Color::from_rgba8(235, 62, 62, 235),
        );
    }

    if plus_degrees_visible {
        let text1 = format!("{}°", angle_between(points[0], vertex, helper).round() as i32);
        let text2 = format!("{}°", angle_between(helper, vertex, points[2]).round() as i32);
        let center1 = point_from_polar(vertex, mid1, radius);
        let center2 = point_from_polar(vertex, mid2, radius);
        let panel_background = if outside.is_some() {
            Color::from_rgba8(255, 225, 225, 175)
        } else {
            Color::from_rgba8(255, 255, 255, 148)
        };
        let panel_text = if outside.is_some() {
            Color::from_rgba8(150, 24, 24, 250)
        } else {
            Color::from_rgba8(18, 18, 18, 248)
        };

        if !hidden_panels.helper_left {
            draw_text_panel(
                pixmap,
                &text1,
                center1.x,
                center1.y,
                panel_background,
                panel_text,
            );
        }
        if !hidden_panels.helper_right {
            draw_text_panel(
                pixmap,
                &text2,
                center2.x,
                center2.y,
                panel_background,
                panel_text,
            );
        }

        if bisector_visible && !hidden_panels.helper_yellow {
            if let Some((_, _, yellow_mid, yellow_radius, yellow_delta)) =
                helper_bisector_arc_geometry(points, helper)
            {
                let yellow_text = format!("{}°", yellow_delta.to_degrees().round() as i32);
                let yellow_center = point_from_polar(vertex, yellow_mid, yellow_radius);
                draw_text_panel(
                    pixmap,
                    &yellow_text,
                    yellow_center.x,
                    yellow_center.y,
                    Color::from_rgba8(255, 244, 178, 185),
                    Color::from_rgba8(112, 82, 0, 250),
                );
            }
        }
    }

    if let Some(outside) = outside {
        // The additional panel reports only the angular overrun beyond the sector.
        if !hidden_panels.helper_delta {
            let delta_text = format!("Δ {}°", outside.delta_radians.to_degrees().round() as i32);
            let delta_center = point_from_polar(vertex, outside.mid_angle, radius + 28.0);
            draw_text_panel(
                pixmap,
                &delta_text,
                delta_center.x,
                delta_center.y,
                Color::from_rgba8(255, 205, 205, 205),
                Color::from_rgba8(165, 24, 24, 252),
            );
        }
    }

    draw_plus_handle(pixmap, helper);
}

fn draw_hypotenuse(pixmap: &mut Pixmap, points: [Point; 3]) {
    draw_dashed_line(
        pixmap,
        points[0],
        points[2],
        7.0,
        5.0,
        1.4,
        Color::from_rgba8(235, 50, 50, 235),
        HANDLE_RADIUS + 2.0,
        HANDLE_RADIUS + 2.0,
    );
}

fn draw_front_plus(pixmap: &mut Pixmap, points: [Point; 3], helper: Point) {
    let green = Color::from_rgba8(48, 205, 88, 225);
    for red in [points[0], points[2]] {
        draw_dashed_line(
            pixmap,
            helper,
            red,
            6.0,
            4.0,
            1.25,
            green,
            PLUS_HIT_RADIUS * 0.65,
            HANDLE_RADIUS + 2.0,
        );
    }
}

fn draw_xtk_plus(pixmap: &mut Pixmap, points: [Point; 3], helper: Point) {
    // XTK + is an independent perpendicular measurement from the green plus
    // to the infinite geometric bisector of the main angle.
    if let Some(foot) = helper_bisector_foot(points, helper) {
        draw_dashed_line(
            pixmap,
            helper,
            foot,
            6.0,
            4.0,
            1.35,
            Color::from_rgba8(235, 50, 50, 238),
            PLUS_HIT_RADIUS * 0.65,
            0.0,
        );
    }
}

fn xtk_overlay_bounds(points: [Point; 3], helper: Point) -> ContentBounds {
    let foot = helper_bisector_foot(points, helper).unwrap_or(helper);
    ContentBounds {
        min_x: helper.x.min(foot.x) - 5.0,
        min_y: helper.y.min(foot.y) - 5.0,
        max_x: helper.x.max(foot.x) + 5.0,
        max_y: helper.y.max(foot.y) + 5.0,
    }
}

struct App {
    window: Option<Arc<Window>>,
    splash_window: Option<Arc<Window>>,
    splash_deadline: Option<Instant>,
    startup_monitor: Option<MonitorGeometry>,
    points: [Point; 3],
    helper_point: Option<Point>,
    active_handle: Option<usize>,
    cursor_pos: Option<Point>,
    was_minimized: bool,
    angle_locked: bool,
    locked_signed_angle: f32,
    red_locked_index: Option<usize>,
    bisector_visible: bool,
    plus_degrees_visible: bool,
    inverted: bool,
    hypotenuse_visible: bool,
    front_plus_visible: bool,
    xtk_visible: bool,
    distance_visible: bool,
    meters_per_pixel: f32,
    north_visible: bool,
    north_angle: f32,
    north_locked: bool,
    course_visible: bool,
    box_visible: bool,
    box_points: Vec<Point>,
    box_closed: bool,
    hidden_box_distance_panels: u16,
    hidden_box_bearing_panels: u16,
    hidden_angle_panels: HiddenAnglePanels,
    hidden_distance_panels: HiddenDistancePanels,
    blue_pinned: bool,
    left_red_pinned: bool,
    right_red_pinned: bool,
    helper_pinned: bool,
    distance_editor: Option<DistanceEditor>,
    last_distance_click: Option<(DistanceKind, Instant)>,
    last_plus_click: Option<Instant>,
    last_red_click: Option<(usize, Instant)>,
    angle_wheel_accumulator: f32,
    rotation_wheel_accumulator: f32,
    north_wheel_accumulator: f32,
    course_wheel_accumulator: f32,
}

impl App {
    fn new() -> Self {
        let saved = load_state();
        let box_saved = load_box_state(&box_settings_path());
        let mut app = Self {
            window: None,
            splash_window: None,
            splash_deadline: None,
            startup_monitor: None,
            points: saved.map(|state| state.points).unwrap_or_else(default_points),
            helper_point: saved.and_then(|state| state.helper_point),
            active_handle: None,
            cursor_pos: None,
            was_minimized: false,
            angle_locked: saved.map(|state| state.angle_locked).unwrap_or(false),
            locked_signed_angle: saved
                .map(|state| state.locked_signed_angle)
                .unwrap_or(0.0),
            red_locked_index: saved.and_then(|state| state.red_locked_index),
            bisector_visible: saved.map(|state| state.bisector_visible).unwrap_or(true),
            plus_degrees_visible: saved
                .map(|state| state.plus_degrees_visible)
                .unwrap_or(false),
            inverted: saved.map(|state| state.inverted).unwrap_or(false),
            hypotenuse_visible: saved
                .map(|state| state.hypotenuse_visible)
                .unwrap_or(false),
            front_plus_visible: saved
                .map(|state| state.front_plus_visible)
                .unwrap_or(false),
            xtk_visible: saved.map(|state| state.xtk_visible).unwrap_or(false),
            distance_visible: saved.map(|state| state.distance_visible).unwrap_or(false),
            meters_per_pixel: saved
                .map(|state| state.meters_per_pixel)
                .unwrap_or(0.0),
            north_visible: saved.map(|state| state.north_visible).unwrap_or(false),
            north_angle: saved.map(|state| state.north_angle).unwrap_or(NORTH_DEFAULT_ANGLE),
            north_locked: saved.map(|state| state.north_locked).unwrap_or(false),
            course_visible: saved.map(|state| state.course_visible).unwrap_or(false),
            box_visible: box_saved.visible,
            box_points: box_saved.points,
            box_closed: box_saved.closed,
            hidden_box_distance_panels: 0,
            hidden_box_bearing_panels: 0,
            hidden_angle_panels: HiddenAnglePanels::default(),
            hidden_distance_panels: HiddenDistancePanels::default(),
            blue_pinned: saved.map(|state| state.blue_pinned).unwrap_or(false),
            left_red_pinned: saved.map(|state| state.left_red_pinned).unwrap_or(false),
            right_red_pinned: saved.map(|state| state.right_red_pinned).unwrap_or(false),
            helper_pinned: saved.map(|state| state.helper_pinned).unwrap_or(false),
            distance_editor: None,
            last_distance_click: None,
            last_plus_click: None,
            last_red_click: None,
            angle_wheel_accumulator: 0.0,
            rotation_wheel_accumulator: 0.0,
            north_wheel_accumulator: 0.0,
            course_wheel_accumulator: 0.0,
        };
        if app.course_visible {
            app.north_visible = true;
            if app.helper_point.is_none() {
                app.helper_point = Some(app.default_helper_point());
            }
        }
        app.rebase_geometry_for_startup();
        app
    }

    fn rebase_geometry_for_startup(&mut self) {
        // Saved coordinates are local to the old overlay window. Rebase the
        // entire construction around a stable local anchor before creating the
        // new window, so stale off-screen window coordinates cannot strand it.
        let anchor = Point { x: 320.0, y: 230.0 };
        let vertex = self.points[1];
        if !vertex.x.is_finite()
            || !vertex.y.is_finite()
            || self.points.iter().any(|point| !point.x.is_finite() || !point.y.is_finite())
        {
            self.points = default_points();
            if self.helper_point.is_some() {
                self.helper_point = Some(self.default_helper_point());
            }
            self.box_points.clear();
            self.box_visible = false;
            self.box_closed = false;
            return;
        }
        let dx = anchor.x - vertex.x;
        let dy = anchor.y - vertex.y;
        for point in &mut self.points {
            point.x += dx;
            point.y += dy;
        }
        if let Some(helper) = &mut self.helper_point {
            helper.x += dx;
            helper.y += dy;
        }
        for point in &mut self.box_points {
            point.x += dx;
            point.y += dy;
        }
    }

    fn detect_startup_monitor(event_loop: &ActiveEventLoop) -> MonitorGeometry {
        let monitor = event_loop
            .primary_monitor()
            .or_else(|| event_loop.available_monitors().next());
        if let Some(monitor) = monitor {
            let position = monitor.position();
            let size = monitor.size();
            MonitorGeometry {
                x: position.x,
                y: position.y,
                width: size.width.max(1),
                height: size.height.max(1),
            }
        } else {
            MonitorGeometry {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            }
        }
    }

    fn splash_size(monitor: MonitorGeometry) -> u32 {
        ((monitor.width.min(monitor.height) as f32 * SPLASH_SCREEN_FRACTION).round() as u32)
            .clamp(SPLASH_MIN_SIZE, SPLASH_MAX_SIZE)
    }

    fn center_window_on_monitor(window: &Window, monitor: MonitorGeometry) {
        let size = window.inner_size();
        let x = monitor.x + ((monitor.width as i64 - size.width as i64) / 2) as i32;
        let y = monitor.y + ((monitor.height as i64 - size.height as i64) / 2) as i32;
        window.set_outer_position(PhysicalPosition::new(x, y));
    }

    fn create_splash_window(&mut self, event_loop: &ActiveEventLoop) {
        let monitor = Self::detect_startup_monitor(event_loop);
        let side = Self::splash_size(monitor);
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("ProtractorPlus")
                        .with_inner_size(PhysicalSize::new(side, side))
                        .with_transparent(true)
                        .with_decorations(false)
                        .with_resizable(false)
                        .with_window_icon(Some(icon::window_icon())),
                )
                .expect("create splash window"),
        );
        Self::center_window_on_monitor(&window, monitor);
        let hwnd = hwnd_from_window(&window);
        unsafe {
            configure_overlay(hwnd);
            set_click_through(hwnd, true);
        }
        self.startup_monitor = Some(monitor);
        self.splash_deadline = Some(Instant::now() + SPLASH_DURATION);
        self.splash_window = Some(window);
        if let Some(window) = &self.splash_window {
            window.request_redraw();
        }
    }

    fn create_main_window(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("ProtractorPlus")
                        .with_inner_size(LogicalSize::new(640.0, 420.0))
                        .with_transparent(true)
                        .with_decorations(false)
                        .with_resizable(true)
                        .with_window_icon(Some(icon::window_icon())),
                )
                .expect("create main window"),
        );
        let hwnd = hwnd_from_window(&window);
        unsafe {
            configure_overlay(hwnd);
            set_click_through(hwnd, CLICK_THROUGH.load(Ordering::Relaxed));
        }
        self.window = Some(window);
        self.fit_window_to_content();

        if let (Some(window), Some(monitor)) = (&self.window, self.startup_monitor) {
            let blue = self.points[1];
            let screen_center_x = monitor.x + monitor.width as i32 / 2;
            let screen_center_y = monitor.y + monitor.height as i32 / 2;
            window.set_outer_position(PhysicalPosition::new(
                screen_center_x - blue.x.round() as i32,
                screen_center_y - blue.y.round() as i32,
            ));
        } else {
            self.center_blue_on_current_monitor();
        }
        self.redraw();
    }

    fn redraw_splash(&self) {
        let Some(window) = &self.splash_window else {
            return;
        };
        let size = window.inner_size();
        let pixmap = splash::render(size.width.max(1), size.height.max(1));
        let pos = window
            .outer_position()
            .unwrap_or(PhysicalPosition::new(0, 0));
        unsafe {
            present_pixmap(hwnd_from_window(window), &pixmap, pos.x, pos.y);
        }
    }

    fn finish_splash(&mut self, event_loop: &ActiveEventLoop) {
        self.splash_deadline = None;
        if let Some(window) = self.splash_window.take() {
            window.set_visible(false);
        }
        self.create_main_window(event_loop);
    }

    fn red_point_pinned(&self, index: usize) -> bool {
        match index {
            0 => self.left_red_pinned,
            2 => self.right_red_pinned,
            _ => false,
        }
    }

    fn any_red_pinned(&self) -> bool {
        self.left_red_pinned || self.right_red_pinned
    }

    fn any_screen_pin(&self) -> bool {
        self.blue_pinned || self.any_red_pinned() || self.helper_pinned
    }

    fn toggle_helper_pin(&mut self) {
        if self.helper_point.is_none() {
            return;
        }
        self.helper_pinned = !self.helper_pinned;
        if self.helper_pinned && self.active_handle == Some(HELPER_HANDLE_INDEX) {
            self.active_handle = None;
        }
        self.save_state();
        self.request_redraw();
    }

    fn toggle_blue_pin(&mut self) {
        self.blue_pinned = !self.blue_pinned;
        self.active_handle = None;
        self.save_state();
        self.request_redraw();
    }

    fn toggle_red_pin(&mut self, index: usize) {
        match index {
            0 => self.left_red_pinned = !self.left_red_pinned,
            2 => self.right_red_pinned = !self.right_red_pinned,
            _ => return,
        }
        self.active_handle = None;
        self.save_state();
        self.request_redraw();
    }

    fn standard_distance_rects(&self, angle_rects: &[PanelRect]) -> Vec<PanelRect> {
        if !self.distance_visible || self.meters_per_pixel <= 0.0 {
            return Vec::new();
        }
        let Some(helper) = self.helper_point else {
            return Vec::new();
        };
        distance_panels(
            self.points,
            helper,
            self.meters_per_pixel,
            self.hypotenuse_visible,
            self.front_plus_visible,
            self.xtk_visible,
            self.distance_editor.as_ref(),
            self.hidden_distance_panels,
            angle_rects,
        )
        .into_iter()
        .map(|panel| panel.rect)
        .collect()
    }

    fn box_panel_layout_with_angle_rects(&self, angle_rects: &[PanelRect]) -> BoxPanelLayout {
        if !self.box_visible || self.box_points.len() < 2 {
            return BoxPanelLayout::default();
        }
        let mut avoid = angle_rects.to_vec();
        avoid.extend(self.standard_distance_rects(angle_rects));
        layout_box_panels(
            &self.box_points,
            self.box_closed,
            self.meters_per_pixel,
            self.north_angle,
            self.distance_visible,
            self.plus_degrees_visible,
            self.hidden_box_distance_panels,
            self.hidden_box_bearing_panels,
            &avoid,
        )
    }

    fn box_panel_layout(&self) -> BoxPanelLayout {
        let angle_rects = self.angle_panel_layout().rects();
        self.box_panel_layout_with_angle_rects(&angle_rects)
    }

    fn hide_box_panel_at(&mut self, point: Point) -> bool {
        let Some(kind) = self.box_panel_layout().hit_test(point) else {
            return false;
        };
        match kind {
            BoxPanelKind::Distance(index) => {
                self.hidden_box_distance_panels |= 1u16 << index;
            }
            BoxPanelKind::Bearing(index) => {
                self.hidden_box_bearing_panels |= 1u16 << index;
            }
        }
        self.fit_window_to_content();
        self.request_redraw();
        true
    }

    fn toggle_box_mode(&mut self) {
        self.box_visible = !self.box_visible;
        self.hidden_box_distance_panels = 0;
        self.hidden_box_bearing_panels = 0;
        self.fit_window_to_content();
        self.save_state();
        self.request_redraw();
    }

    fn add_or_close_box_point(&mut self, point: Point) -> bool {
        if !self.box_visible || self.box_closed {
            return false;
        }
        if is_close_target(point, &self.box_points) {
            self.box_closed = true;
            self.hidden_box_distance_panels = 0;
            self.hidden_box_bearing_panels = 0;
            self.fit_window_to_content();
            self.save_state();
            self.request_redraw();
            return true;
        }
        if box_point_at(point, &self.box_points).is_some() {
            return true;
        }
        if self.box_points.len() >= MAX_BOX_POINTS {
            return true;
        }
        self.box_points.push(point);
        self.hidden_box_distance_panels = 0;
        self.hidden_box_bearing_panels = 0;
        self.fit_window_to_content();
        self.save_state();
        self.request_redraw();
        true
    }

    fn delete_box_point(&mut self, index: usize) {
        if index >= self.box_points.len() {
            return;
        }
        self.box_points.remove(index);
        if self.box_points.len() < 3 {
            self.box_closed = false;
        }
        if self.box_points.is_empty() {
            self.box_visible = false;
            self.box_closed = false;
        }
        self.hidden_box_distance_panels = 0;
        self.hidden_box_bearing_panels = 0;
        self.fit_window_to_content();
        self.save_state();
        self.request_redraw();
    }

    fn course_panel_for_north(&self) -> Option<PanelRect> {
        if !self.course_visible
            || !self.north_visible
            || self.hidden_angle_panels.course
        {
            return None;
        }
        let helper = self.helper_point?;
        course_angle_label_rect(self.points[1], helper, self.north_angle)
    }

    fn angle_panel_layout(&self) -> AnglePanelLayout {
        let mut layout = AnglePanelLayout::default();
        let mut occupied = Vec::new();
        let anchor = self.points[1];

        // The main angle panel is mandatory and keeps its canonical position.
        let main = label_panel_rect(self.points);
        layout.main = Some(main);
        occupied.push(main);

        if self.course_visible
            && self.north_visible
            && !self.hidden_angle_panels.course
        {
            if let Some(helper) = self.helper_point {
                if let Some(preferred) =
                    course_angle_label_rect(self.points[1], helper, self.north_angle)
                {
                    let resolved = resolve_angle_panel(preferred, &occupied, anchor);
                    layout.course = Some(resolved);
                    occupied.push(resolved);
                }
            }
        }

        if self.north_visible
            && self.bisector_visible
            && !self.hidden_angle_panels.north
        {
            if let Some(preferred) = north_angle_label_rect(self.points, self.north_angle, None) {
                let resolved = resolve_angle_panel(preferred, &occupied, anchor);
                layout.north = Some(resolved);
                occupied.push(resolved);
            }
        }

        if let Some(helper) = self.helper_point {
            if self.plus_degrees_visible {
                let (left, right) = helper_angle_panel_rects(self.points, helper);
                if !self.hidden_angle_panels.helper_left {
                    let resolved = resolve_angle_panel(left, &occupied, anchor);
                    layout.helper_left = Some(resolved);
                    occupied.push(resolved);
                }
                if !self.hidden_angle_panels.helper_right {
                    let resolved = resolve_angle_panel(right, &occupied, anchor);
                    layout.helper_right = Some(resolved);
                    occupied.push(resolved);
                }
                if self.bisector_visible && !self.hidden_angle_panels.helper_yellow {
                    if let Some(preferred) = helper_yellow_panel_rect(self.points, helper) {
                        let resolved = resolve_angle_panel(preferred, &occupied, anchor);
                        layout.helper_yellow = Some(resolved);
                        occupied.push(resolved);
                    }
                }
            }

            if !self.hidden_angle_panels.helper_delta {
                if let Some(preferred) = helper_delta_panel_rect(self.points, helper) {
                    let resolved = resolve_angle_panel(preferred, &occupied, anchor);
                    layout.helper_delta = Some(resolved);
                }
            }
        }

        layout
    }

    fn draw_angle_panels(&self, pixmap: &mut Pixmap, layout: AnglePanelLayout) {
        if let Some(panel) = layout.main {
            let text = format!(
                "{}°",
                angle_between(self.points[0], self.points[1], self.points[2]).round() as i32
            );
            draw_text_panel(
                pixmap,
                &text,
                panel.x + panel.width * 0.5,
                panel.y + panel.height * 0.5,
                Color::from_rgba8(255, 255, 255, 148),
                Color::from_rgba8(18, 18, 18, 248),
            );
        }

        if let Some(panel) = layout.course {
            if let Some(helper) = self.helper_point {
                if let Some((sweep, _, _)) =
                    course_arc_geometry(self.points[1], helper, self.north_angle)
                {
                    let text = format!("{}°", bearing_degrees(sweep));
                    draw_text_panel(
                        pixmap,
                        &text,
                        panel.x + panel.width * 0.5,
                        panel.y + panel.height * 0.5,
                        Color::from_rgba8(225, 255, 225, 188),
                        Color::from_rgba8(22, 132, 42, 252),
                    );
                }
            }
        }

        if let Some(panel) = layout.north {
            if let Some((_, sweep, _, _)) = north_arc_geometry(self.points, self.north_angle) {
                let text = format!("{}°", bearing_degrees(sweep));
                draw_text_panel(
                    pixmap,
                    &text,
                    panel.x + panel.width * 0.5,
                    panel.y + panel.height * 0.5,
                    Color::from_rgba8(218, 233, 255, 188),
                    Color::from_rgba8(28, 87, 190, 252),
                );
            }
        }

        if let Some(helper) = self.helper_point {
            let outside = helper_outside_angle(self.points, helper);
            let panel_background = if outside.is_some() {
                Color::from_rgba8(255, 225, 225, 175)
            } else {
                Color::from_rgba8(255, 255, 255, 148)
            };
            let panel_text = if outside.is_some() {
                Color::from_rgba8(150, 24, 24, 250)
            } else {
                Color::from_rgba8(18, 18, 18, 248)
            };

            if let Some(panel) = layout.helper_left {
                let text = format!(
                    "{}°",
                    angle_between(self.points[0], self.points[1], helper).round() as i32
                );
                draw_text_panel(
                    pixmap,
                    &text,
                    panel.x + panel.width * 0.5,
                    panel.y + panel.height * 0.5,
                    panel_background,
                    panel_text,
                );
            }
            if let Some(panel) = layout.helper_right {
                let text = format!(
                    "{}°",
                    angle_between(helper, self.points[1], self.points[2]).round() as i32
                );
                draw_text_panel(
                    pixmap,
                    &text,
                    panel.x + panel.width * 0.5,
                    panel.y + panel.height * 0.5,
                    panel_background,
                    panel_text,
                );
            }
            if let Some(panel) = layout.helper_yellow {
                if let Some((_, _, _, _, delta)) =
                    helper_bisector_arc_geometry(self.points, helper)
                {
                    let text = format!("{}°", delta.to_degrees().round() as i32);
                    draw_text_panel(
                        pixmap,
                        &text,
                        panel.x + panel.width * 0.5,
                        panel.y + panel.height * 0.5,
                        Color::from_rgba8(255, 244, 178, 185),
                        Color::from_rgba8(112, 82, 0, 250),
                    );
                }
            }
            if let (Some(panel), Some(outside)) = (layout.helper_delta, outside) {
                let text = format!("Δ {}°", outside.delta_radians.to_degrees().round() as i32);
                draw_text_panel(
                    pixmap,
                    &text,
                    panel.x + panel.width * 0.5,
                    panel.y + panel.height * 0.5,
                    Color::from_rgba8(255, 205, 205, 205),
                    Color::from_rgba8(165, 24, 24, 252),
                );
            }
        }
    }

    fn hide_distance_panel_at(&mut self, point: Point) -> bool {
        let Some(kind) = self.distance_panel_at(point) else {
            return false;
        };
        self.hidden_distance_panels.hide(kind);
        if self
            .distance_editor
            .as_ref()
            .map(|editor| editor.kind == kind)
            .unwrap_or(false)
        {
            self.distance_editor = None;
        }
        self.fit_window_to_content();
        self.request_redraw();
        true
    }

    fn current_signed_angle(&self) -> f32 {
        let vertex = self.points[1];
        normalize_signed_angle(
            vector_angle(vertex, self.points[2]) - vector_angle(vertex, self.points[0]),
        )
    }

    fn default_helper_point(&self) -> Point {
        let vertex = self.points[1];
        let (bx, by) = angle_bisector_direction(self.points).unwrap_or((0.0, -1.0));
        clamp_line_length(
            vertex,
            Point {
                x: vertex.x + bx * HELPER_DISTANCE,
                y: vertex.y + by * HELPER_DISTANCE,
            },
        )
    }

    fn snap_helper_to_hypotenuse_midpoint(&mut self) {
        if self.helper_point.is_none() || self.helper_pinned {
            return;
        }
        self.helper_point = Some(Point {
            x: (self.points[0].x + self.points[2].x) * 0.5,
            y: (self.points[0].y + self.points[2].y) * 0.5,
        });
        self.fit_window_to_content();
        self.save_state();
        self.request_redraw();
    }


    fn snap_hypotenuse_midpoint_to_helper(&mut self) {
        let Some(helper) = self.helper_point else {
            return;
        };

        // Two closed locks freeze the red system completely. A double click
        // must not rotate or rebuild it until one of the locks is opened.
        if self.both_locks_closed() || self.any_red_pinned() {
            return;
        }

        let vertex = self.points[1];
        let signed_angle = self.current_signed_angle();
        let old_centre_angle = normalize_signed_angle(
            vector_angle(vertex, self.points[0]) + signed_angle * 0.5,
        );
        let half_angle = signed_angle * 0.5;
        let centre_distance = vector_length(vertex, helper);
        let projection = half_angle.cos().abs();

        if centre_distance < EPSILON || projection < EPSILON {
            return;
        }

        // Keep the blue vertex fixed. For two equally long red rays, the
        // midpoint of the hypotenuse lies on the angle bisector at
        // radius * cos(angle / 2). Rebuild the two red points around the fixed
        // blue vertex so that this midpoint coincides with the green plus.
        // Do not translate the blue vertex and do not clamp the reconstructed
        // radius: clamping would move the hypotenuse midpoint away from the
        // requested green-plus position. The equality below therefore remains
        // exact for the current angle.
        let radius = centre_distance / projection;
        if !radius.is_finite() {
            return;
        }
        let centre_angle = vector_angle(vertex, helper);
        self.points[0] = point_from_polar(
            vertex,
            centre_angle - signed_angle * 0.5,
            radius,
        );
        self.points[2] = point_from_polar(
            vertex,
            centre_angle + signed_angle * 0.5,
            radius,
        );
        self.rotate_north_if_unlocked(normalize_signed_angle(
            centre_angle - old_centre_angle,
        ));

        if self.angle_locked {
            self.locked_signed_angle = signed_angle;
        }
        self.fit_window_to_content();
        self.save_state();
        self.request_redraw();
    }

    fn both_locks_closed(&self) -> bool {
        self.angle_locked && self.red_locked_index.is_some()
    }

    fn toggle_red_lock(&mut self, index: usize) {
        if index != 0 && index != 2 {
            return;
        }
        if self.red_point_pinned(index) {
            return;
        }
        self.red_locked_index = if self.red_locked_index == Some(index) {
            None
        } else {
            Some(index)
        };
        if self.angle_locked {
            self.locked_signed_angle = self.current_signed_angle();
        }
        self.save_state();
        self.request_redraw();
    }

    fn toggle_angle_lock(&mut self) {
        if self.blue_pinned {
            return;
        }
        self.angle_locked = !self.angle_locked;
        if self.angle_locked {
            self.locked_signed_angle = self.current_signed_angle();
        }
        self.save_state();
        self.request_redraw();
    }

    fn toggle_north_lock(&mut self) {
        if !self.north_visible {
            return;
        }
        self.north_locked = !self.north_locked;
        self.save_state();
        self.request_redraw();
    }

    fn reset_helper_angle_panels(&mut self) {
        self.hidden_angle_panels.helper_left = false;
        self.hidden_angle_panels.helper_right = false;
        self.hidden_angle_panels.helper_yellow = false;
        self.hidden_angle_panels.helper_delta = false;
    }

    fn hide_angle_panel_at(&mut self, point: Point) -> bool {
        // The main blue-vertex angle panel is intentionally permanent.
        let layout = self.angle_panel_layout();

        if let Some(panel) = layout.helper_delta {
            if point_in_panel(point, panel) {
                self.hidden_angle_panels.helper_delta = true;
                self.fit_window_to_content();
                self.request_redraw();
                return true;
            }
        }
        if let Some(panel) = layout.helper_yellow {
            if point_in_panel(point, panel) {
                self.hidden_angle_panels.helper_yellow = true;
                self.fit_window_to_content();
                self.request_redraw();
                return true;
            }
        }
        if let Some(panel) = layout.helper_right {
            if point_in_panel(point, panel) {
                self.hidden_angle_panels.helper_right = true;
                self.fit_window_to_content();
                self.request_redraw();
                return true;
            }
        }
        if let Some(panel) = layout.helper_left {
            if point_in_panel(point, panel) {
                self.hidden_angle_panels.helper_left = true;
                self.fit_window_to_content();
                self.request_redraw();
                return true;
            }
        }
        if let Some(panel) = layout.course {
            if point_in_panel(point, panel) {
                self.hidden_angle_panels.course = true;
                self.fit_window_to_content();
                self.request_redraw();
                return true;
            }
        }
        if let Some(panel) = layout.north {
            if point_in_panel(point, panel) {
                self.hidden_angle_panels.north = true;
                self.fit_window_to_content();
                self.request_redraw();
                return true;
            }
        }

        false
    }

    fn rotate_north_if_unlocked(&mut self, delta_radians: f32) {
        if self.north_visible && !self.north_locked {
            self.north_angle = normalize_signed_angle(self.north_angle + delta_radians);
        }
    }

    fn rotate_helper_about_vertex_by_degrees(&mut self, delta_degrees: f32) {
        if self.helper_pinned {
            return;
        }
        let Some(helper) = self.helper_point else { return; };
        let vertex = self.points[1];
        let radius = vector_length(vertex, helper);
        if radius < EPSILON { return; }
        let angle = vector_angle(vertex, helper) + delta_degrees.to_radians();
        self.helper_point = Some(point_from_polar(vertex, angle, radius));
    }

    fn toggle_helper_point(&mut self) {
        self.reset_helper_angle_panels();
        self.hidden_angle_panels.course = false;
        self.hidden_distance_panels.clear_all();
        self.helper_point = if self.helper_point.is_some() {
            self.distance_visible = false;
            self.distance_editor = None;
            self.course_visible = false;
            self.xtk_visible = false;
            self.helper_pinned = false;
            None
        } else {
            self.helper_pinned = false;
            Some(self.default_helper_point())
        };
        self.fit_window_to_content();
        self.save_state();
        self.request_redraw();
    }

    fn toggle_distance_mode(&mut self) {
        self.hidden_distance_panels.clear_all();
        self.hidden_box_distance_panels = 0;
        if self.distance_visible {
            self.distance_visible = false;
            self.distance_editor = None;
        } else {
            if self.helper_point.is_none() {
                self.helper_point = Some(self.default_helper_point());
            }
            if let Some(helper) = self.helper_point {
                let base_pixels = vector_length(self.points[1], helper);
                if base_pixels > EPSILON {
                    // The geometry present at activation is calibrated as 1000 metres.
                    self.meters_per_pixel = 1000.0 / base_pixels;
                }
            }
            self.distance_visible = true;
        }
        self.fit_window_to_content();
        self.save_state();
        self.request_redraw();
    }

    fn toggle_feature(&mut self, command: u32) {
        match command {
            MENU_BISECTOR => {
                self.bisector_visible = !self.bisector_visible;
                // Both the North/bisector label and the yellow helper label
                // belong to the bisector feature, so toggling it restores them.
                self.hidden_angle_panels.north = false;
                self.hidden_angle_panels.helper_yellow = false;
            }
            MENU_PLUS => {
                self.toggle_helper_point();
                return;
            }
            MENU_COURSE_PLUS => {
                self.hidden_angle_panels.course = false;
                self.course_visible = !self.course_visible;
                if self.course_visible {
                    if self.helper_point.is_none() {
                        self.helper_point = Some(self.default_helper_point());
                    }
                    self.north_visible = true;
                }
            }
            MENU_PLUS_DEGREES => {
                self.reset_helper_angle_panels();
                self.hidden_box_bearing_panels = 0;
                self.plus_degrees_visible = !self.plus_degrees_visible;
            }
            MENU_INVERSION => self.inverted = !self.inverted,
            MENU_HYPOTENUSE => {
                self.hypotenuse_visible = !self.hypotenuse_visible;
                self.hidden_distance_panels.show(DistanceKind::Hypotenuse);
            },
            MENU_FRONT_PLUS => {
                self.front_plus_visible = !self.front_plus_visible;
                self.hidden_distance_panels.show(DistanceKind::FrontLeft);
                self.hidden_distance_panels.show(DistanceKind::FrontRight);
            },
            MENU_XTK_PLUS => {
                self.xtk_visible = !self.xtk_visible;
                self.hidden_distance_panels
                    .show(DistanceKind::FrontPerpendicular);
                if self.xtk_visible && self.helper_point.is_none() {
                    self.helper_point = Some(self.default_helper_point());
                }
            }
            MENU_BOX_PLUS => {
                self.toggle_box_mode();
                return;
            }
            MENU_DISTANCE_PLUS => {
                self.toggle_distance_mode();
                return;
            }
            MENU_NORTH_PLUS => {
                self.hidden_angle_panels.north = false;
                self.hidden_angle_panels.course = false;
                self.north_visible = !self.north_visible;
                if !self.north_visible {
                    self.course_visible = false;
                }
            }
            _ => return,
        }
        self.fit_window_to_content();
        self.save_state();
        self.request_redraw();
    }

    fn context_menu_state(&self) -> ContextMenuState {
        ContextMenuState {
            bisector: self.bisector_visible,
            plus: self.helper_point.is_some(),
            course_plus: self.course_visible,
            plus_degrees: self.plus_degrees_visible,
            inversion: self.inverted,
            hypotenuse: self.hypotenuse_visible,
            front_plus: self.front_plus_visible,
            xtk_plus: self.xtk_visible,
            distance_plus: self.distance_visible,
            north_plus: self.north_visible,
            box_plus: self.box_visible,
        }
    }

    fn save_state(&self) {
        let (window_x, window_y) = self
            .window
            .as_ref()
            .and_then(|window| window.outer_position().ok())
            .map(|position| (position.x, position.y))
            .unwrap_or((0, 0));
        let p = self.points;
        let helper_enabled = u8::from(self.helper_point.is_some());
        let helper = self.helper_point.unwrap_or(Point { x: 0.0, y: 0.0 });
        let red_lock_code = match self.red_locked_index {
            Some(0) => 1u8,
            Some(2) => 2u8,
            _ => 0u8,
        };
        let lines = [
            "11".to_string(),
            format!("{} {}", p[0].x, p[0].y),
            format!("{} {}", p[1].x, p[1].y),
            format!("{} {}", p[2].x, p[2].y),
            helper_enabled.to_string(),
            format!("{} {}", helper.x, helper.y),
            u8::from(self.angle_locked).to_string(),
            self.locked_signed_angle.to_string(),
            red_lock_code.to_string(),
            u8::from(self.bisector_visible).to_string(),
            u8::from(self.plus_degrees_visible).to_string(),
            u8::from(self.inverted).to_string(),
            u8::from(self.hypotenuse_visible).to_string(),
            u8::from(self.front_plus_visible).to_string(),
            u8::from(self.xtk_visible).to_string(),
            u8::from(self.distance_visible).to_string(),
            self.meters_per_pixel.to_string(),
            u8::from(self.north_visible).to_string(),
            self.north_angle.to_string(),
            u8::from(self.north_locked).to_string(),
            u8::from(self.course_visible).to_string(),
            u8::from(self.blue_pinned).to_string(),
            u8::from(self.left_red_pinned).to_string(),
            u8::from(self.right_red_pinned).to_string(),
            u8::from(self.helper_pinned).to_string(),
            format!("{} {}", window_x, window_y),
        ];
        let text = format!("{}\n", lines.join("\n"));
        let path = settings_path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(path, text);
        save_box_state(
            &box_settings_path(),
            self.box_visible,
            self.box_closed,
            &self.box_points,
        );
    }

    fn redraw(&self) {
        let Some(window) = &self.window else {
            return;
        };
        let hwnd = hwnd_from_window(window);
        if unsafe { is_minimized(hwnd) } {
            return;
        }

        let size = window.inner_size();
        let width = size.width.max(1);
        let height = size.height.max(1);
        let angle_layout = self.angle_panel_layout();
        let angle_rects = angle_layout.rects();
        let box_layout = self.box_panel_layout_with_angle_rects(&angle_rects);
        let mut pixmap = draw::render_angle_measure(
            width,
            height,
            self.points,
            self.inverted,
            false,
        );

        if self.hypotenuse_visible {
            draw_hypotenuse(&mut pixmap, self.points);
        }
        if let Some(helper) = self.helper_point {
            if self.front_plus_visible {
                draw_front_plus(&mut pixmap, self.points, helper);
            }
            if self.xtk_visible {
                draw_xtk_plus(&mut pixmap, self.points, helper);
            }
        }
        if self.bisector_visible {
            draw_dashed_bisector(&mut pixmap, self.points, self.helper_point, self.inverted);
        }
        if self.north_visible {
            draw_north_overlay(
                &mut pixmap,
                self.points,
                self.bisector_visible,
                self.inverted,
                self.north_angle,
                self.north_locked,
                false,
                None,
            );
        }
        if self.course_visible {
            if let Some(helper) = self.helper_point {
                if self.north_visible {
                    draw_course_overlay(
                        &mut pixmap,
                        self.points[1],
                        helper,
                        self.north_angle,
                        false,
                    );
                }
            }
        }
        if let Some(helper) = self.helper_point {
            let suppressed_helper_labels = HiddenAnglePanels {
                main: true,
                north: true,
                course: true,
                helper_left: true,
                helper_right: true,
                helper_yellow: true,
                helper_delta: true,
            };
            draw_helper_overlay(
                &mut pixmap,
                self.points,
                helper,
                self.plus_degrees_visible,
                self.bisector_visible,
                suppressed_helper_labels,
                self.inverted,
            );
            if self.distance_visible && self.meters_per_pixel > 0.0 {
                draw_distance_overlay(
                    &mut pixmap,
                    self.points,
                    helper,
                    self.meters_per_pixel,
                    self.hypotenuse_visible,
                    self.front_plus_visible,
                    self.xtk_visible,
                    self.distance_editor.as_ref(),
                    self.hidden_distance_panels,
                    &angle_rects,
                );
            }
            draw_helper_lock_icon(&mut pixmap, helper, self.helper_pinned, self.inverted);
        }

        if self.box_visible {
            draw_box_geometry(&mut pixmap, &self.box_points, self.box_closed);
            draw_box_panels(&mut pixmap, &box_layout);
        }

        // All degree panels are drawn from one collision-resolved layout so
        // they cannot cover each other or any distance panel.
        self.draw_angle_panels(&mut pixmap, angle_layout);

        draw_lock_icon(
            &mut pixmap,
            self.points,
            self.angle_locked,
            self.blue_pinned,
            self.inverted,
        );
        draw_red_lock_icons(
            &mut pixmap,
            self.points,
            self.red_locked_index,
            self.left_red_pinned,
            self.right_red_pinned,
            self.inverted,
        );

        let pos = window
            .outer_position()
            .unwrap_or(PhysicalPosition::new(0, 0));
        unsafe {
            present_pixmap(hwnd, &pixmap, pos.x, pos.y);
        }
    }

    fn distance_panel_at(&self, point: Point) -> Option<DistanceKind> {
        if !self.distance_visible || self.meters_per_pixel <= 0.0 {
            return None;
        }
        let helper = self.helper_point?;
        let angle_rects = self.angle_panel_layout().rects();
        hit_test_distance_panel(
            point,
            self.points,
            helper,
            self.meters_per_pixel,
            self.hypotenuse_visible,
            self.front_plus_visible,
            self.xtk_visible,
            self.distance_editor.as_ref(),
            self.hidden_distance_panels,
            &angle_rects,
        )
    }

    fn begin_distance_edit(&mut self, kind: DistanceKind) {
        let Some(helper) = self.helper_point else {
            return;
        };
        let value = meters_for_kind(self.points, helper, self.meters_per_pixel, kind);
        let buffer = if (value - value.round()).abs() < 0.01 {
            format!("{:.0}", value)
        } else {
            let mut text = format!("{:.2}", value);
            while text.ends_with('0') {
                text.pop();
            }
            if text.ends_with('.') {
                text.pop();
            }
            text
        };
        self.distance_editor = Some(DistanceEditor {
            kind,
            buffer,
            replace_on_input: true,
        });
        self.fit_window_to_content();
        self.request_redraw();
    }

    fn cancel_distance_edit(&mut self) {
        self.distance_editor = None;
        self.fit_window_to_content();
        self.request_redraw();
    }

    fn commit_distance_edit(&mut self) {
        let Some(editor) = self.distance_editor.take() else {
            return;
        };
        let normalized = editor.buffer.replace(',', ".");
        if let Ok(value) = normalized.parse::<f32>() {
            if value.is_finite() && value > 0.0 {
                self.apply_distance_value(editor.kind, value);
            }
        }
        self.fit_window_to_content();
        self.save_state();
        self.request_redraw();
    }

    fn append_distance_input(&mut self, text: &str) {
        let Some(editor) = &mut self.distance_editor else {
            return;
        };
        let contains_input = text
            .chars()
            .any(|character| character.is_ascii_digit() || character == '.' || character == ',');
        if editor.replace_on_input && contains_input {
            editor.buffer.clear();
            editor.replace_on_input = false;
        }
        for character in text.chars() {
            if character.is_ascii_digit() && editor.buffer.len() < 12 {
                editor.buffer.push(character);
            } else if (character == '.' || character == ',')
                && !editor.buffer.contains('.')
                && !editor.buffer.contains(',')
                && editor.buffer.len() < 12
            {
                editor.buffer.push('.');
            }
        }
        self.fit_window_to_content();
        self.request_redraw();
    }

    fn backspace_distance_input(&mut self) {
        if let Some(editor) = &mut self.distance_editor {
            if editor.replace_on_input {
                editor.buffer.clear();
                editor.replace_on_input = false;
            } else {
                editor.buffer.pop();
            }
            self.fit_window_to_content();
            self.request_redraw();
        }
    }

    fn set_shared_red_radius(&mut self, radius_pixels: f32) {
        if self.any_red_pinned() {
            return;
        }
        let vertex = self.points[1];
        let radius = radius_pixels.clamp(HANDLE_RADIUS + 8.0, MAX_LINE_LEN);
        let left_angle = vector_angle(vertex, self.points[0]);
        let right_angle = vector_angle(vertex, self.points[2]);
        self.points[0] = point_from_polar(vertex, left_angle, radius);
        self.points[2] = point_from_polar(vertex, right_angle, radius);
    }

    fn apply_distance_value(&mut self, kind: DistanceKind, metres: f32) {
        let Some(helper) = self.helper_point else {
            return;
        };
        let scale = self.meters_per_pixel;
        if scale <= EPSILON {
            return;
        }

        match kind {
            DistanceKind::Base => {
                let base_pixels = vector_length(self.points[1], helper);
                if base_pixels > EPSILON {
                    self.meters_per_pixel = (metres / base_pixels).clamp(0.000_001, 1_000_000.0);
                }
            }
            DistanceKind::LeftRay | DistanceKind::RightRay => {
                self.set_shared_red_radius(metres / scale);
            }
            DistanceKind::Hypotenuse => {
                let half_angle = self.current_signed_angle().abs() * 0.5;
                let sine = half_angle.sin().abs();
                if sine > EPSILON {
                    self.set_shared_red_radius((metres / scale) / (2.0 * sine));
                }
            }
            DistanceKind::FrontPerpendicular => {
                if self.helper_pinned {
                    return;
                }
                let vertex = self.points[1];
                let Some((bx, by)) = angle_bisector_direction(self.points) else {
                    return;
                };
                let hx = helper.x - vertex.x;
                let hy = helper.y - vertex.y;
                let along = hx * bx + hy * by;
                let nx = -by;
                let ny = bx;
                let signed_offset = hx * nx + hy * ny;
                let sign = if signed_offset < 0.0 { -1.0 } else { 1.0 };
                let target_pixels = metres / scale;
                self.helper_point = Some(clamp_line_length(
                    vertex,
                    Point {
                        x: vertex.x + bx * along + nx * sign * target_pixels,
                        y: vertex.y + by * along + ny * sign * target_pixels,
                    },
                ));
            }
            DistanceKind::FrontLeft | DistanceKind::FrontRight => {
                let index = if kind == DistanceKind::FrontLeft { 0 } else { 2 };
                let vertex = self.points[1];
                let ray_angle = vector_angle(vertex, self.points[index]);
                let unit_x = ray_angle.cos();
                let unit_y = ray_angle.sin();
                let helper_x = helper.x - vertex.x;
                let helper_y = helper.y - vertex.y;
                let projection = helper_x * unit_x + helper_y * unit_y;
                let helper_sq = helper_x * helper_x + helper_y * helper_y;
                let target_pixels = metres / scale;
                let discriminant = target_pixels * target_pixels - helper_sq
                    + projection * projection;
                if discriminant >= 0.0 {
                    let root = discriminant.sqrt();
                    let current = vector_length(vertex, self.points[index]);
                    let candidates = [projection + root, projection - root];
                    let chosen = candidates
                        .into_iter()
                        .filter(|value| *value > HANDLE_RADIUS + 8.0)
                        .min_by(|a, b| {
                            (*a - current)
                                .abs()
                                .partial_cmp(&(*b - current).abs())
                                .unwrap_or(std::cmp::Ordering::Equal)
                        });
                    if let Some(radius) = chosen {
                        self.set_shared_red_radius(radius);
                    }
                }
            }
        }
    }

    fn adjust_angle_by_degrees(&mut self, delta_deg: f32) {
        if self.both_locks_closed() || self.any_red_pinned() {
            return;
        }
        let vertex = self.points[1];
        let current_signed = self.current_signed_angle();
        let current_abs_deg = current_signed.abs().to_degrees();
        let target_abs_deg = (current_abs_deg + delta_deg).clamp(1.0, 179.0);
        let sign = if current_signed < 0.0 { -1.0 } else { 1.0 };
        let new_signed = sign * target_abs_deg.to_radians();

        let a_angle = vector_angle(vertex, self.points[0]);
        let center_angle = normalize_signed_angle(a_angle + current_signed * 0.5);
        let shared_radius = ((vector_length(vertex, self.points[0])
            + vector_length(vertex, self.points[2]))
            * 0.5)
            .clamp(HANDLE_RADIUS + 8.0, MAX_LINE_LEN);

        self.points[0] = point_from_polar(vertex, center_angle - new_signed * 0.5, shared_radius);
        self.points[2] = point_from_polar(vertex, center_angle + new_signed * 0.5, shared_radius);

        if self.angle_locked {
            self.locked_signed_angle = new_signed;
        }
    }

    fn rotate_red_system_by_degrees(&mut self, visual_degrees: f32) {
        if self.both_locks_closed() || self.any_red_pinned() {
            return;
        }
        let vertex = self.points[1];
        // Screen Y grows downward, therefore negative mathematical rotation is
        // counter-clockwise on screen.
        let delta = visual_degrees.to_radians();
        for index in [0usize, 2usize] {
            let angle = vector_angle(vertex, self.points[index]) + delta;
            let radius = vector_length(vertex, self.points[index]);
            self.points[index] = point_from_polar(vertex, angle, radius);
        }
        self.rotate_north_if_unlocked(delta);
    }


    fn rotate_about_red_lock_by_degrees(&mut self, index: usize, visual_degrees: f32) {
        if self.both_locks_closed() {
            return;
        }
        if index != 0 && index != 2 {
            return;
        }
        let other = if index == 0 { 2 } else { 0 };
        // The selected red point is the pivot and may itself be screen-pinned.
        // The operation is blocked if it would move another pinned point.
        if self.blue_pinned || self.red_point_pinned(other) {
            return;
        }
        let pivot = self.points[index];
        let delta = visual_degrees.to_radians();

        for moving_index in [1usize, other] {
            let angle = vector_angle(pivot, self.points[moving_index]) + delta;
            let radius = vector_length(pivot, self.points[moving_index]);
            self.points[moving_index] = point_from_polar(pivot, angle, radius);
        }
        self.rotate_north_if_unlocked(delta);
    }

    fn center_blue_on_current_monitor(&self) {
        let Some(window) = &self.window else {
            return;
        };
        let Some(monitor) = window.current_monitor() else {
            return;
        };
        let monitor_position = monitor.position();
        let monitor_size = monitor.size();
        let screen_center_x = monitor_position.x + monitor_size.width as i32 / 2;
        let screen_center_y = monitor_position.y + monitor_size.height as i32 / 2;
        let blue = self.points[1];
        window.set_outer_position(PhysicalPosition::new(
            screen_center_x - blue.x.round() as i32,
            screen_center_y - blue.y.round() as i32,
        ));
    }

    fn wheel_units(delta: MouseScrollDelta) -> f32 {
        match delta {
            MouseScrollDelta::LineDelta(_, y) => y,
            MouseScrollDelta::PixelDelta(position) => position.y as f32 / 50.0,
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() || self.splash_window.is_some() {
            return;
        }
        self.create_splash_window(event_loop);
        if let Some(deadline) = self.splash_deadline {
            event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if self
            .splash_window
            .as_ref()
            .map(|window| window.id() == window_id)
            .unwrap_or(false)
        {
            match event {
                WindowEvent::CloseRequested => event_loop.exit(),
                WindowEvent::RedrawRequested => self.redraw_splash(),
                WindowEvent::Resized(_) | WindowEvent::Moved(_) => {
                    if let Some(window) = &self.splash_window {
                        window.request_redraw();
                    }
                }
                WindowEvent::KeyboardInput { event, .. }
                    if event.state == ElementState::Pressed
                        && event.logical_key == Key::Named(NamedKey::Escape) =>
                {
                    event_loop.exit();
                }
                _ => {}
            }
            return;
        }

        if self
            .window
            .as_ref()
            .map(|window| window.id() != window_id)
            .unwrap_or(true)
        {
            return;
        }

        match event {
            WindowEvent::CloseRequested => {
                self.save_state();
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => self.redraw(),
            WindowEvent::Focused(focused) => {
                if !focused {
                    return;
                }
                if let Some(window) = &self.window {
                    let hwnd = hwnd_from_window(window);
                    let minimized = unsafe { is_minimized(hwnd) };
                    if self.was_minimized && !minimized {
                        unsafe { restore_window(hwnd) };
                        self.request_redraw();
                    }
                    self.was_minimized = minimized;
                }
            }
            WindowEvent::Resized(_) | WindowEvent::Moved(_) => {
                if let Some(window) = &self.window {
                    let hwnd = hwnd_from_window(window);
                    if !unsafe { is_minimized(hwnd) } {
                        window.request_redraw();
                    }
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed {
                    return;
                }
                if self.distance_editor.is_some() {
                    match event.logical_key {
                        Key::Named(NamedKey::Enter) => self.commit_distance_edit(),
                        Key::Named(NamedKey::Escape) => self.cancel_distance_edit(),
                        Key::Named(NamedKey::Backspace) => self.backspace_distance_input(),
                        Key::Character(ref text) => self.append_distance_input(text),
                        _ => {}
                    }
                    return;
                }
                match event.logical_key {
                    Key::Named(NamedKey::Escape) => {
                        self.save_state();
                        event_loop.exit();
                    }
                    Key::Character(ref ch) if ch == "r" || ch == "R" => {
                        self.reset_points();
                        self.save_state();
                        self.request_redraw();
                    }
                    Key::Character(ref ch) if ch == "l" || ch == "L" => {
                        self.toggle_angle_lock();
                    }
                    Key::Character(ref ch) if ch == "t" || ch == "T" => {
                        let next = !CLICK_THROUGH.load(Ordering::Relaxed);
                        CLICK_THROUGH.store(next, Ordering::Relaxed);
                        if let Some(window) = &self.window {
                            let hwnd = hwnd_from_window(window);
                            unsafe { set_click_through(hwnd, next) };
                        }
                    }
                    _ => {}
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let Some(window) = self.window.clone() else {
                    return;
                };
                if button == MouseButton::Right && state == ElementState::Pressed {
                    if self.distance_editor.is_some() {
                        self.commit_distance_edit();
                    }
                    let hwnd = hwnd_from_window(&window);
                    if self.box_visible {
                        if let Some(pos) = self.cursor_pos {
                            if let Some(index) = box_point_at(pos, &self.box_points) {
                                if unsafe { show_box_point_menu(hwnd) }
                                    == Some(MENU_DELETE_BOX_POINT)
                                {
                                    self.delete_box_point(index);
                                }
                                return;
                            }
                        }
                    }
                    if let Some(command) =
                        unsafe { show_context_menu(hwnd, self.context_menu_state()) }
                    {
                        match command {
                            MENU_BISECTOR
                            | MENU_PLUS
                            | MENU_COURSE_PLUS
                            | MENU_PLUS_DEGREES
                            | MENU_INVERSION
                            | MENU_HYPOTENUSE
                            | MENU_FRONT_PLUS
                            | MENU_XTK_PLUS
                            | MENU_BOX_PLUS
                            | MENU_DISTANCE_PLUS
                            | MENU_NORTH_PLUS => self.toggle_feature(command),
                            MENU_MINIMIZE => {
                                self.save_state();
                                self.was_minimized = true;
                                unsafe { minimize_window(hwnd) };
                            }
                            MENU_CLOSE => {
                                self.save_state();
                                event_loop.exit();
                            }
                            _ => {}
                        }
                    }
                    return;
                }
                if button == MouseButton::Middle && state == ElementState::Pressed {
                    if let Some(pos) = self.cursor_pos {
                        if self.hide_distance_panel_at(pos) {
                            return;
                        }
                        if self.hide_box_panel_at(pos) {
                            return;
                        }
                        if self.hide_angle_panel_at(pos) {
                            return;
                        }
                        if let Some(helper) = self.helper_point {
                            if in_helper_lock_button(pos, helper) {
                                self.toggle_helper_pin();
                                return;
                            }
                        }
                        if let Some(index) = in_red_lock_button(pos, self.points) {
                            self.toggle_red_pin(index);
                            return;
                        }
                        if in_lock_button(pos, self.points) {
                            self.toggle_blue_pin();
                            return;
                        }
                    }
                    return;
                }
                if button != MouseButton::Left {
                    return;
                }
                match state {
                    ElementState::Pressed => {
                        if let Some(pos) = self.cursor_pos {
                            if self.north_visible {
                                let vertex = self.points[1];
                                if in_north_lock_button(pos, vertex, self.north_angle) {
                                    self.toggle_north_lock();
                                    return;
                                }
                                if in_north_handle(pos, vertex, self.north_angle) {
                                    self.active_handle = if self.north_locked {
                                        None
                                    } else {
                                        Some(NORTH_HANDLE_INDEX)
                                    };
                                    return;
                                }
                            }
                            if let Some(helper) = self.helper_point {
                                if in_helper_lock_button(pos, helper) {
                                    self.toggle_helper_pin();
                                    return;
                                }
                                if in_helper_handle(pos, helper) {
                                    let now = Instant::now();
                                    let is_double = self
                                        .last_plus_click
                                        .map(|last| {
                                            now.duration_since(last) <= Duration::from_millis(450)
                                        })
                                        .unwrap_or(false);
                                    if is_double {
                                        self.last_plus_click = None;
                                        self.active_handle = None;
                                        self.snap_helper_to_hypotenuse_midpoint();
                                    } else {
                                        self.last_plus_click = Some(now);
                                        self.active_handle = Some(HELPER_HANDLE_INDEX);
                                    }
                                    return;
                                }
                            }
                            self.last_plus_click = None;

                            if let Some(index) = red_point_at(pos, self.points) {
                                if self.red_point_pinned(index) {
                                    self.active_handle = None;
                                    return;
                                }
                                let now = Instant::now();
                                let is_double = self
                                    .last_red_click
                                    .as_ref()
                                    .map(|(previous, time)| {
                                        *previous == index
                                            && time.elapsed() <= Duration::from_millis(450)
                                    })
                                    .unwrap_or(false);
                                if is_double {
                                    self.last_red_click = None;
                                    self.active_handle = None;
                                    self.snap_hypotenuse_midpoint_to_helper();
                                } else {
                                    self.last_red_click = Some((index, now));
                                    self.active_handle = Some(index);
                                }
                                return;
                            }
                            self.last_red_click = None;

                            if let Some(kind) = self.distance_panel_at(pos) {
                                let now = Instant::now();
                                let is_double = self
                                    .last_distance_click
                                    .as_ref()
                                    .map(|(previous, time)| {
                                        *previous == kind
                                            && time.elapsed() <= Duration::from_millis(450)
                                    })
                                    .unwrap_or(false);
                                if is_double {
                                    self.last_distance_click = None;
                                    self.begin_distance_edit(kind);
                                } else {
                                    self.last_distance_click = Some((kind, now));
                                }
                                return;
                            }
                            if self.distance_editor.is_some() {
                                self.commit_distance_edit();
                                return;
                            }
                            if let Some(index) = in_red_lock_button(pos, self.points) {
                                self.toggle_red_lock(index);
                                return;
                            }
                            if in_lock_button(pos, self.points) {
                                self.toggle_angle_lock();
                                return;
                            }
                            self.active_handle = self.hit_test(pos.x, pos.y);
                            if self.active_handle.is_some() {
                                return;
                            }
                            let over_angle_panel = self
                                .angle_panel_layout()
                                .rects()
                                .into_iter()
                                .any(|panel| point_in_panel(pos, panel));
                            let over_box_panel = self.box_panel_layout().hit_test(pos).is_some();
                            if self.box_visible && !over_angle_panel && !over_box_panel {
                                if self.add_or_close_box_point(pos) {
                                    self.active_handle = None;
                                    return;
                                }
                            }
                            if !self.any_screen_pin() {
                                let _ = window.drag_window();
                            }
                        }
                    }
                    ElementState::Released => {
                        if self.active_handle.take().is_some() {
                            self.save_state();
                        }
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if self.active_handle.is_some() {
                    return;
                }
                let Some(cursor) = self.cursor_pos else {
                    return;
                };
                let wheel_units = Self::wheel_units(delta);
                let angle_layout = self.angle_panel_layout();

                if angle_layout
                    .course
                    .map(|panel| point_in_panel(cursor, panel))
                    .unwrap_or(false)
                {
                    self.angle_wheel_accumulator = 0.0;
                    self.rotation_wheel_accumulator = 0.0;
                    self.north_wheel_accumulator = 0.0;
                    self.course_wheel_accumulator += wheel_units;
                    let whole_steps = self.course_wheel_accumulator.trunc() as i32;
                    if whole_steps != 0 {
                        self.course_wheel_accumulator -= whole_steps as f32;
                        // Wheel up increases the displayed clockwise course bearing.
                        self.rotate_helper_about_vertex_by_degrees(whole_steps as f32);
                        self.fit_window_to_content();
                        self.save_state();
                        self.request_redraw();
                    }
                    return;
                }

                if angle_layout
                    .north
                    .map(|panel| point_in_panel(cursor, panel))
                    .unwrap_or(false)
                {
                    self.angle_wheel_accumulator = 0.0;
                    self.rotation_wheel_accumulator = 0.0;
                    self.course_wheel_accumulator = 0.0;
                    if self.north_locked {
                        self.north_wheel_accumulator = 0.0;
                        return;
                    }
                    self.north_wheel_accumulator += wheel_units;
                    let whole_steps = self.north_wheel_accumulator.trunc() as i32;
                    if whole_steps != 0 {
                        self.north_wheel_accumulator -= whole_steps as f32;
                        // Wheel up rotates North counter-clockwise; wheel down clockwise.
                        self.north_angle = normalize_signed_angle(
                            self.north_angle - (whole_steps as f32).to_radians(),
                        );
                        self.fit_window_to_content();
                        self.save_state();
                        self.request_redraw();
                    }
                    return;
                }

                self.north_wheel_accumulator = 0.0;

                if self.distance_panel_at(cursor).is_some() {
                    return;
                }

                // When both the blue and a red lock are closed, the angle and
                // absolute orientation are frozen. No wheel target may rotate
                // or open the construction, including the degree label.
                if self.both_locks_closed() {
                    self.angle_wheel_accumulator = 0.0;
                    self.rotation_wheel_accumulator = 0.0;
                    return;
                }

                let blue_lock_hovered = in_lock_button(cursor, self.points);
                let red_lock_hovered = in_red_lock_button(cursor, self.points);

                if let Some(red_index) = self.red_locked_index {
                    let active_red_lock_hovered = red_lock_hovered == Some(red_index);
                    if self.angle_locked && (blue_lock_hovered || active_red_lock_hovered) {
                        // Both locks are closed: global rotation is intentionally blocked.
                        self.angle_wheel_accumulator = 0.0;
                        self.rotation_wheel_accumulator = 0.0;
                        return;
                    }
                    if !self.angle_locked && (blue_lock_hovered || active_red_lock_hovered) {
                        self.angle_wheel_accumulator = 0.0;
                        self.rotation_wheel_accumulator += wheel_units;
                        let whole_steps = self.rotation_wheel_accumulator.trunc() as i32;
                        if whole_steps != 0 {
                            self.rotation_wheel_accumulator -= whole_steps as f32;
                            // The locked red point is the pivot. Wheel up rotates
                            // counter-clockwise, wheel down clockwise.
                            self.rotate_about_red_lock_by_degrees(
                                red_index,
                                -(whole_steps as f32),
                            );
                            self.fit_window_to_content();
                            self.save_state();
                            self.request_redraw();
                        }
                        return;
                    }
                }

                if self.angle_locked && blue_lock_hovered {
                    self.angle_wheel_accumulator = 0.0;
                    self.rotation_wheel_accumulator += wheel_units;
                    let whole_steps = self.rotation_wheel_accumulator.trunc() as i32;
                    if whole_steps != 0 {
                        self.rotation_wheel_accumulator -= whole_steps as f32;
                        // Wheel up: counter-clockwise. Wheel down: clockwise.
                        self.rotate_red_system_by_degrees(-(whole_steps as f32));
                        self.fit_window_to_content();
                        self.save_state();
                        self.request_redraw();
                    }
                    return;
                }

                if angle_layout
                    .main
                    .map(|panel| point_in_panel(cursor, panel))
                    .unwrap_or(false)
                {
                    self.rotation_wheel_accumulator = 0.0;
                    self.angle_wheel_accumulator += wheel_units;
                    let whole_steps = self.angle_wheel_accumulator.trunc() as i32;
                    if whole_steps != 0 {
                        self.angle_wheel_accumulator -= whole_steps as f32;
                        self.adjust_angle_by_degrees(whole_steps as f32);
                        self.fit_window_to_content();
                        self.save_state();
                        self.request_redraw();
                    }
                    return;
                }

                self.angle_wheel_accumulator = 0.0;
                self.rotation_wheel_accumulator = 0.0;
                self.north_wheel_accumulator = 0.0;
                self.course_wheel_accumulator = 0.0;
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_pos = Some(Point {
                    x: position.x as f32,
                    y: position.y as f32,
                });
                if let Some(index) = self.active_handle {
                    let target = Point {
                        x: position.x as f32,
                        y: position.y as f32,
                    };
                    self.move_handle(index, target);
                    self.fit_window_to_content();
                    self.request_redraw();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.splash_window.is_some() {
            let deadline = self
                .splash_deadline
                .unwrap_or_else(|| Instant::now() + SPLASH_DURATION);
            if Instant::now() >= deadline {
                self.finish_splash(event_loop);
                event_loop.set_control_flow(ControlFlow::Wait);
            } else {
                event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
            }
            return;
        }

        event_loop.set_control_flow(ControlFlow::Wait);
        let Some(window) = &self.window else {
            return;
        };
        let hwnd = hwnd_from_window(window);
        let minimized = unsafe { is_minimized(hwnd) };
        if minimized {
            self.was_minimized = true;
            return;
        }
        if self.was_minimized {
            self.was_minimized = false;
            unsafe { ensure_topmost(hwnd) };
            self.request_redraw();
        }
    }
}

impl App {
    fn reset_points(&mut self) {
        if let Some(window) = &self.window {
            let size = window.inner_size();
            let w = size.width as f32;
            let h = size.height as f32;
            self.points = [
                Point { x: w * 0.22, y: h * 0.68 },
                Point { x: w * 0.50, y: h * 0.55 },
                Point { x: w * 0.78, y: h * 0.68 },
            ];
            self.blue_pinned = false;
            self.left_red_pinned = false;
            self.right_red_pinned = false;
            self.helper_pinned = false;
            if self.helper_point.is_some() {
                self.helper_point = Some(self.default_helper_point());
            }
            if self.angle_locked {
                self.locked_signed_angle = self.current_signed_angle();
            }
            self.fit_window_to_content();
        }
    }

    fn calculated_content_bounds(&self) -> ContentBounds {
        let angle_layout = self.angle_panel_layout();
        let angle_rects = angle_layout.rects();
        let box_layout = self.box_panel_layout_with_angle_rects(&angle_rects);
        let mut bounds = content_bounds(self.points);

        if self.north_visible {
            bounds = merge_bounds(
                bounds,
                north_overlay_bounds(
                    self.points,
                    self.bisector_visible,
                    self.north_angle,
                    None,
                ),
            );
        }
        if self.course_visible {
            if let Some(helper) = self.helper_point {
                if self.north_visible {
                    bounds = merge_bounds(
                        bounds,
                        course_overlay_bounds(self.points[1], helper, self.north_angle),
                    );
                }
            }
        }
        if let Some(helper) = self.helper_point {
            bounds = merge_bounds(
                bounds,
                helper_overlay_bounds(
                    self.points,
                    helper,
                    self.plus_degrees_visible,
                    self.bisector_visible,
                ),
            );
            if self.xtk_visible {
                bounds = merge_bounds(bounds, xtk_overlay_bounds(self.points, helper));
            }
            if self.distance_visible && self.meters_per_pixel > 0.0 {
                bounds = merge_bounds(
                    bounds,
                    distance_bounds(
                        self.points,
                        helper,
                        self.meters_per_pixel,
                        self.hypotenuse_visible,
                        self.front_plus_visible,
                        self.xtk_visible,
                        self.distance_editor.as_ref(),
                        self.hidden_distance_panels,
                        &angle_rects,
                    ),
                );
            }
        }

        if self.box_visible {
            if let Some(box_content) = box_bounds(&self.box_points, &box_layout) {
                bounds = merge_bounds(bounds, box_content);
            }
        }

        merge_bounds(
            bounds,
            panel_layout_bounds(&angle_rects, self.points[1]),
        )
    }

    fn fit_window_to_content(&mut self) {
        let Some(window) = &self.window else {
            return;
        };
        let bounds = self.calculated_content_bounds();
        let mut lock_centers = vec![
            lock_center(self.points),
            red_lock_center(self.points, 0),
            red_lock_center(self.points, 2),
        ];
        if let Some(helper) = self.helper_point {
            lock_centers.push(helper_lock_center(helper));
        }
        let mut min_x = bounds.min_x;
        let mut min_y = bounds.min_y;
        for center in &lock_centers {
            min_x = min_x.min(center.x - LOCK_PANEL_SIZE * 0.5);
            min_y = min_y.min(center.y - LOCK_PANEL_SIZE * 0.5);
        }

        let mut shift_x = 0.0f32;
        let mut shift_y = 0.0f32;
        if min_x < CONTENT_PAD {
            shift_x = CONTENT_PAD - min_x;
        }
        if min_y < CONTENT_PAD {
            shift_y = CONTENT_PAD - min_y;
        }
        if shift_x > 0.0 || shift_y > 0.0 {
            for point in &mut self.points {
                point.x += shift_x;
                point.y += shift_y;
            }
            if let Some(helper) = &mut self.helper_point {
                helper.x += shift_x;
                helper.y += shift_y;
            }
            for point in &mut self.box_points {
                point.x += shift_x;
                point.y += shift_y;
            }
            if let Ok(outer) = window.outer_position() {
                window.set_outer_position(PhysicalPosition::new(
                    outer.x - shift_x.round() as i32,
                    outer.y - shift_y.round() as i32,
                ));
            }
        }

        let bounds = self.calculated_content_bounds();
        let mut lock_centers = vec![
            lock_center(self.points),
            red_lock_center(self.points, 0),
            red_lock_center(self.points, 2),
        ];
        if let Some(helper) = self.helper_point {
            lock_centers.push(helper_lock_center(helper));
        }
        let mut max_x = bounds.max_x;
        let mut max_y = bounds.max_y;
        for center in lock_centers {
            max_x = max_x.max(center.x + LOCK_PANEL_SIZE * 0.5);
            max_y = max_y.max(center.y + LOCK_PANEL_SIZE * 0.5);
        }
        let new_w = (max_x + CONTENT_PAD)
            .ceil()
            .max(MIN_WINDOW)
            .min(8192.0) as u32;
        let new_h = (max_y + CONTENT_PAD)
            .ceil()
            .max(MIN_WINDOW)
            .min(8192.0) as u32;
        let size = window.inner_size();
        if new_w != size.width || new_h != size.height {
            let _ = window.request_inner_size(PhysicalSize::new(new_w, new_h));
        }
    }

    fn move_handle(&mut self, index: usize, target: Point) {
        if index == NORTH_HANDLE_INDEX {
            if self.north_visible && !self.north_locked {
                let vertex = self.points[1];
                if vector_length(vertex, target) >= HANDLE_RADIUS + 8.0 {
                    self.north_angle = normalize_signed_angle(vector_angle(vertex, target));
                }
            }
            return;
        }

        if index == 1 {
            if self.blue_pinned {
                return;
            }
            if let Some(red_index) = self.red_locked_index {
                if !self.angle_locked {
                    // With only a red lock closed, the red point becomes the
                    // pivot. Dragging the blue vertex rotates the blue vertex
                    // and the opposite red point as one rigid system.
                    let pivot = self.points[red_index];
                    let old_vertex = self.points[1];
                    let target_radius = vector_length(pivot, target);
                    if target_radius >= EPSILON {
                        let other_index = if red_index == 0 { 2 } else { 0 };
                        if self.red_point_pinned(other_index) {
                            return;
                        }
                        let old_angle = vector_angle(pivot, old_vertex);
                        let new_angle = vector_angle(pivot, target);
                        let delta = normalize_signed_angle(new_angle - old_angle);
                        let blue_radius = vector_length(pivot, old_vertex);
                        self.points[1] = point_from_polar(pivot, new_angle, blue_radius);

                        let other_angle = vector_angle(pivot, self.points[other_index]) + delta;
                        let other_radius = vector_length(pivot, self.points[other_index]);
                        self.points[other_index] =
                            point_from_polar(pivot, other_angle, other_radius);
                        self.rotate_north_if_unlocked(delta);
                    }
                    return;
                }
            }

            // Normal blue-point movement translates the complete construction.
            // A yellow-pinned red point must remain at its absolute screen position.
            if self.any_red_pinned() {
                return;
            }
            let old_vertex = self.points[1];
            let a_off = (
                self.points[0].x - old_vertex.x,
                self.points[0].y - old_vertex.y,
            );
            let b_off = (
                self.points[2].x - old_vertex.x,
                self.points[2].y - old_vertex.y,
            );
            let helper_off = self
                .helper_point
                .map(|helper| (helper.x - old_vertex.x, helper.y - old_vertex.y));
            self.points[1] = target;
            self.points[0] = clamp_line_length(
                target,
                Point {
                    x: target.x + a_off.0,
                    y: target.y + a_off.1,
                },
            );
            self.points[2] = clamp_line_length(
                target,
                Point {
                    x: target.x + b_off.0,
                    y: target.y + b_off.1,
                },
            );
            if !self.helper_pinned {
                if let Some((hx, hy)) = helper_off {
                    self.helper_point = Some(clamp_line_length(
                        target,
                        Point {
                            x: target.x + hx,
                            y: target.y + hy,
                        },
                    ));
                }
            }
            return;
        }

        let vertex = self.points[1];
        if index == HELPER_HANDLE_INDEX {
            if self.helper_pinned {
                return;
            }
            let moved = clamp_line_length(vertex, target);
            if vector_length(vertex, moved) >= EPSILON {
                self.helper_point = Some(moved);
            }
            return;
        }

        if self.any_red_pinned() {
            return;
        }

        let moved = clamp_line_length(vertex, target);
        let moved_radius = vector_length(vertex, moved);
        if moved_radius < EPSILON {
            return;
        }

        if self.both_locks_closed() {
            // Both locks closed: neither red ray may rotate and the angle may
            // not change. Dragging either red point only adjusts the common
            // length along the two already established absolute directions.
            let left_angle = vector_angle(vertex, self.points[0]);
            let right_angle = vector_angle(vertex, self.points[2]);
            self.points[0] = point_from_polar(vertex, left_angle, moved_radius);
            self.points[2] = point_from_polar(vertex, right_angle, moved_radius);
            self.locked_signed_angle = self.current_signed_angle();
            return;
        }

        if !self.angle_locked {
            let other_index = if index == 0 { 2 } else { 0 };
            let other_angle = vector_angle(vertex, self.points[other_index]);
            self.points[index] = moved;
            self.points[other_index] = point_from_polar(vertex, other_angle, moved_radius);
            return;
        }

        if index == 0 {
            let old_angle = vector_angle(vertex, self.points[0]);
            let moved_angle = vector_angle(vertex, moved);
            self.points[0] = moved;
            self.points[2] = point_from_polar(
                vertex,
                moved_angle + self.locked_signed_angle,
                moved_radius,
            );
            self.rotate_north_if_unlocked(normalize_signed_angle(moved_angle - old_angle));
        } else {
            let old_angle = vector_angle(vertex, self.points[2]);
            let moved_angle = vector_angle(vertex, moved);
            self.points[2] = moved;
            self.points[0] = point_from_polar(
                vertex,
                moved_angle - self.locked_signed_angle,
                moved_radius,
            );
            self.rotate_north_if_unlocked(normalize_signed_angle(moved_angle - old_angle));
        }
    }

    fn hit_test(&self, x: f32, y: f32) -> Option<usize> {
        if self.north_visible && !self.north_locked {
            let point = Point { x, y };
            if in_north_handle(point, self.points[1], self.north_angle) {
                return Some(NORTH_HANDLE_INDEX);
            }
        }

        if let Some(helper) = self.helper_point {
            let dx = helper.x - x;
            let dy = helper.y - y;
            if !self.helper_pinned
                && dx * dx + dy * dy <= PLUS_HIT_RADIUS * PLUS_HIT_RADIUS
            {
                return Some(HELPER_HANDLE_INDEX);
            }
        }

        let hit_radius = HANDLE_RADIUS + 6.0;
        self.points
            .iter()
            .enumerate()
            .find(|(index, point)| {
                if (*index == 1 && self.blue_pinned) || self.red_point_pinned(*index) {
                    return false;
                }
                let dx = point.x - x;
                let dy = point.y - y;
                dx * dx + dy * dy <= hit_radius * hit_radius
            })
            .map(|(index, _)| index)
    }

    fn request_redraw(&self) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        self.save_state();
    }
}

fn main() {
    let event_loop = EventLoop::new().expect("event loop");
    let mut app = App::new();
    event_loop.run_app(&mut app).expect("run");
}
