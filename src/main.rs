#![windows_subsystem = "windows"]

mod draw;
mod icon;
mod text;
mod win32_layered;

use std::f32::consts::PI;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use draw::{content_bounds, Point, HANDLE_RADIUS};
use tiny_skia::{Color, Paint, PathBuilder, Pixmap, Rect, Stroke, Transform};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

use crate::win32_layered::{
    configure_overlay, ensure_topmost, hwnd_from_window, is_minimized, minimize_window,
    present_pixmap, restore_window, set_click_through, show_context_menu, MENU_CLOSE,
    MENU_MINIMIZE,
};

static CLICK_THROUGH: AtomicBool = AtomicBool::new(false);

const CONTENT_PAD: f32 = 42.0;
const MIN_WINDOW: f32 = 96.0;
const MAX_LINE_LEN: f32 = 1000.0;
const LOCK_SIZE: f32 = 22.0;
const LOCK_GAP: f32 = 16.0;
const EPSILON: f32 = 0.0001;

#[derive(Clone, Copy, Debug)]
struct SavedState {
    points: [Point; 3],
    angle_locked: bool,
    locked_signed_angle: f32,
    window_x: i32,
    window_y: i32,
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

fn load_state() -> Option<SavedState> {
    let text = fs::read_to_string(settings_path()).ok()?;
    let mut values = text.split_whitespace();
    let version: u32 = values.next()?.parse().ok()?;
    if version != 1 {
        return None;
    }

    let mut number = || values.next()?.parse::<f32>().ok();
    let points = [
        Point { x: number()?, y: number()? },
        Point { x: number()?, y: number()? },
        Point { x: number()?, y: number()? },
    ];
    let angle_locked = values.next()?.parse::<u8>().ok()? != 0;
    let locked_signed_angle = values.next()?.parse::<f32>().ok()?;
    let window_x = values.next()?.parse::<i32>().ok()?;
    let window_y = values.next()?.parse::<i32>().ok()?;

    Some(SavedState {
        points,
        angle_locked,
        locked_signed_angle,
        window_x,
        window_y,
    })
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

fn point_from_polar(vertex: Point, angle: f32, radius: f32) -> Point {
    Point {
        x: vertex.x + angle.cos() * radius,
        y: vertex.y + angle.sin() * radius,
    }
}

fn lock_center(vertex: Point) -> Point {
    Point {
        x: vertex.x + HANDLE_RADIUS + LOCK_GAP + LOCK_SIZE * 0.5,
        y: vertex.y - HANDLE_RADIUS - LOCK_GAP - LOCK_SIZE * 0.5,
    }
}

fn in_lock_button(point: Point, vertex: Point) -> bool {
    let center = lock_center(vertex);
    let half = LOCK_SIZE * 0.75;
    point.x >= center.x - half
        && point.x <= center.x + half
        && point.y >= center.y - half
        && point.y <= center.y + half
}

fn stroke_segment(pixmap: &mut Pixmap, from: Point, to: Point, width: f32, color: Color) {
    let mut builder = PathBuilder::new();
    builder.move_to(from.x, from.y);
    builder.line_to(to.x, to.y);
    if let Some(path) = builder.finish() {
        let mut paint = Paint::default();
        paint.set_color(color);
        let stroke = Stroke {
            width,
            ..Stroke::default()
        };
        pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }
}

fn draw_dashed_bisector(pixmap: &mut Pixmap, points: [Point; 3]) {
    let a = points[0];
    let vertex = points[1];
    let b = points[2];
    let len_a = vector_length(vertex, a);
    let len_b = vector_length(vertex, b);
    if len_a < EPSILON || len_b < EPSILON {
        return;
    }

    let ua = ((a.x - vertex.x) / len_a, (a.y - vertex.y) / len_a);
    let ub = ((b.x - vertex.x) / len_b, (b.y - vertex.y) / len_b);
    let mut bx = ua.0 + ub.0;
    let mut by = ua.1 + ub.1;
    let bisector_len = (bx * bx + by * by).sqrt();
    if bisector_len < EPSILON {
        // Для розгорнутого кута вибираємо стабільний перпендикуляр.
        bx = -ua.1;
        by = ua.0;
    } else {
        bx /= bisector_len;
        by /= bisector_len;
    }

    let total = len_a.min(len_b) * 0.88;
    let dash = 6.0;
    let gap = 5.0;
    let mut distance = HANDLE_RADIUS + 3.0;
    let color = Color::from_rgba8(20, 20, 20, 205);
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
            1.0,
            color,
        );
        distance += dash + gap;
    }
}

fn draw_lock_icon(pixmap: &mut Pixmap, vertex: Point, locked: bool) {
    let center = lock_center(vertex);
    let panel = Rect::from_xywh(
        center.x - LOCK_SIZE * 0.7,
        center.y - LOCK_SIZE * 0.7,
        LOCK_SIZE * 1.4,
        LOCK_SIZE * 1.4,
    );
    if let Some(panel) = panel {
        let mut paint = Paint::default();
        paint.set_color(Color::from_rgba8(255, 255, 255, 220));
        pixmap.fill_rect(panel, &paint, Transform::identity(), None);
    }

    let icon_color = if locked {
        Color::from_rgba8(32, 105, 218, 255)
    } else {
        Color::from_rgba8(45, 45, 45, 235)
    };

    let body = Rect::from_xywh(center.x - 6.0, center.y - 1.0, 12.0, 10.0);
    if let Some(body) = body {
        let mut paint = Paint::default();
        paint.set_color(icon_color);
        pixmap.fill_rect(body, &paint, Transform::identity(), None);
    }

    // Дужка замка. У відкритому стані права ніжка відсунута.
    let right_x = if locked { center.x + 5.0 } else { center.x + 8.0 };
    let mut builder = PathBuilder::new();
    builder.move_to(center.x - 5.0, center.y - 1.0);
    builder.line_to(center.x - 5.0, center.y - 5.0);
    builder.cubic_to(
        center.x - 5.0,
        center.y - 11.0,
        right_x,
        center.y - 11.0,
        right_x,
        center.y - 5.0,
    );
    if locked {
        builder.line_to(right_x, center.y - 1.0);
    }
    if let Some(path) = builder.finish() {
        let mut paint = Paint::default();
        paint.set_color(icon_color);
        let stroke = Stroke {
            width: 2.2,
            ..Stroke::default()
        };
        pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }

    stroke_segment(
        pixmap,
        Point { x: center.x, y: center.y + 2.0 },
        Point { x: center.x, y: center.y + 6.0 },
        1.5,
        Color::from_rgba8(255, 255, 255, 245),
    );
}

struct App {
    window: Option<Arc<Window>>,
    points: [Point; 3],
    active_handle: Option<usize>,
    cursor_pos: Option<Point>,
    was_minimized: bool,
    angle_locked: bool,
    locked_signed_angle: f32,
    restored_window_pos: Option<(i32, i32)>,
}

impl App {
    fn new() -> Self {
        let saved = load_state();
        Self {
            window: None,
            points: saved.map(|state| state.points).unwrap_or_else(default_points),
            active_handle: None,
            cursor_pos: None,
            was_minimized: false,
            angle_locked: saved.map(|state| state.angle_locked).unwrap_or(false),
            locked_signed_angle: saved
                .map(|state| state.locked_signed_angle)
                .unwrap_or(0.0),
            restored_window_pos: saved.map(|state| (state.window_x, state.window_y)),
        }
    }

    fn current_signed_angle(&self) -> f32 {
        let vertex = self.points[1];
        normalize_signed_angle(
            vector_angle(vertex, self.points[2]) - vector_angle(vertex, self.points[0]),
        )
    }

    fn toggle_angle_lock(&mut self) {
        self.angle_locked = !self.angle_locked;
        if self.angle_locked {
            self.locked_signed_angle = self.current_signed_angle();
        }
        self.save_state();
        self.request_redraw();
    }

    fn save_state(&self) {
        let (window_x, window_y) = self
            .window
            .as_ref()
            .and_then(|window| window.outer_position().ok())
            .map(|position| (position.x, position.y))
            .unwrap_or((0, 0));
        let p = self.points;
        let text = format!(
            "1\n{} {}\n{} {}\n{} {}\n{}\n{}\n{} {}\n",
            p[0].x,
            p[0].y,
            p[1].x,
            p[1].y,
            p[2].x,
            p[2].y,
            u8::from(self.angle_locked),
            self.locked_signed_angle,
            window_x,
            window_y,
        );
        let path = settings_path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(path, text);
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
        let mut pixmap = draw::render_angle_measure(width, height, self.points);
        draw_dashed_bisector(&mut pixmap, self.points);
        draw_lock_icon(&mut pixmap, self.points[1], self.angle_locked);

        let pos = window
            .outer_position()
            .unwrap_or(PhysicalPosition::new(0, 0));
        unsafe {
            present_pixmap(hwnd, &pixmap, pos.x, pos.y);
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
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
                .expect("create window"),
        );
        if let Some((x, y)) = self.restored_window_pos.take() {
            window.set_outer_position(PhysicalPosition::new(x, y));
        }
        let hwnd = hwnd_from_window(&window);
        unsafe {
            configure_overlay(hwnd);
        }
        self.window = Some(window);
        self.fit_window_to_content();
        self.redraw();
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
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
                let Some(window) = &self.window else {
                    return;
                };
                if button == MouseButton::Right && state == ElementState::Pressed {
                    let hwnd = hwnd_from_window(window);
                    if let Some(command) = unsafe { show_context_menu(hwnd) } {
                        match command {
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
                if button != MouseButton::Left {
                    return;
                }
                match state {
                    ElementState::Pressed => {
                        if let Some(pos) = self.cursor_pos {
                            if in_lock_button(pos, self.points[1]) {
                                self.toggle_angle_lock();
                                return;
                            }
                            self.active_handle = self.hit_test(pos.x, pos.y);
                            if self.active_handle.is_none() {
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

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
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
            if self.angle_locked {
                self.locked_signed_angle = self.current_signed_angle();
            }
            self.fit_window_to_content();
        }
    }

    fn fit_window_to_content(&mut self) {
        let Some(window) = &self.window else {
            return;
        };
        let bounds = content_bounds(self.points);
        let lock = lock_center(self.points[1]);
        let min_x = bounds.min_x.min(lock.x - LOCK_SIZE);
        let min_y = bounds.min_y.min(lock.y - LOCK_SIZE);
        let max_x = bounds.max_x.max(lock.x + LOCK_SIZE);
        let max_y = bounds.max_y.max(lock.y + LOCK_SIZE);

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
            if let Ok(outer) = window.outer_position() {
                window.set_outer_position(PhysicalPosition::new(
                    outer.x - shift_x.round() as i32,
                    outer.y - shift_y.round() as i32,
                ));
            }
        }

        let bounds = content_bounds(self.points);
        let lock = lock_center(self.points[1]);
        let new_w = (bounds.max_x.max(lock.x + LOCK_SIZE) + CONTENT_PAD)
            .ceil()
            .max(MIN_WINDOW)
            .min(8192.0) as u32;
        let new_h = (bounds.max_y.max(lock.y + LOCK_SIZE) + CONTENT_PAD)
            .ceil()
            .max(MIN_WINDOW)
            .min(8192.0) as u32;
        let size = window.inner_size();
        if new_w != size.width || new_h != size.height {
            let _ = window.request_inner_size(PhysicalSize::new(new_w, new_h));
        }
    }

    fn move_handle(&mut self, index: usize, target: Point) {
        if index == 1 {
            // Переміщення вершини завжди переносить увесь кут без зміни геометрії.
            let old_vertex = self.points[1];
            let a_off = (
                self.points[0].x - old_vertex.x,
                self.points[0].y - old_vertex.y,
            );
            let b_off = (
                self.points[2].x - old_vertex.x,
                self.points[2].y - old_vertex.y,
            );
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
            return;
        }

        let vertex = self.points[1];
        let moved = clamp_line_length(vertex, target);
        if !self.angle_locked {
            self.points[index] = moved;
            return;
        }

        let moved_radius = vector_length(vertex, moved);
        if moved_radius < EPSILON {
            return;
        }

        if index == 0 {
            let other_radius = vector_length(vertex, self.points[2]);
            let moved_angle = vector_angle(vertex, moved);
            self.points[0] = moved;
            self.points[2] = clamp_line_length(
                vertex,
                point_from_polar(
                    vertex,
                    moved_angle + self.locked_signed_angle,
                    other_radius,
                ),
            );
        } else {
            let other_radius = vector_length(vertex, self.points[0]);
            let moved_angle = vector_angle(vertex, moved);
            self.points[2] = moved;
            self.points[0] = clamp_line_length(
                vertex,
                point_from_polar(
                    vertex,
                    moved_angle - self.locked_signed_angle,
                    other_radius,
                ),
            );
        }
    }

    fn hit_test(&self, x: f32, y: f32) -> Option<usize> {
        let hit_radius = HANDLE_RADIUS + 6.0;
        self.points
            .iter()
            .enumerate()
            .find(|(_, point)| {
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
