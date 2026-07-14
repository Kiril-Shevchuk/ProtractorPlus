use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use tiny_skia::Pixmap;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, POINT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC,
    SelectObject, DIB_RGB_COLORS, HBITMAP, HGDIOBJ,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, GetWindowLongW, IsIconic, PostMessageW,
    SetForegroundWindow, SetWindowLongW, SetWindowPos, ShowWindow, TrackPopupMenu,
    UpdateLayeredWindow, GWL_EXSTYLE, HMENU, HWND_TOPMOST, MF_CHECKED, MF_SEPARATOR,
    MF_STRING, SW_MINIMIZE, SW_RESTORE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    TPM_LEFTALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON, ULW_ALPHA, WM_NULL, WS_EX_LAYERED,
    WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
};
use winit::window::Window;

pub const MENU_MINIMIZE: u32 = 1;
pub const MENU_CLOSE: u32 = 2;
pub const MENU_BISECTOR: u32 = 10;
pub const MENU_PLUS: u32 = 11;
pub const MENU_PLUS_DEGREES: u32 = 12;
pub const MENU_INVERSION: u32 = 13;
pub const MENU_HYPOTENUSE: u32 = 14;
pub const MENU_FRONT_PLUS: u32 = 15;
pub const MENU_DISTANCE_PLUS: u32 = 16;

#[derive(Clone, Copy, Debug, Default)]
pub struct ContextMenuState {
    pub bisector: bool,
    pub plus: bool,
    pub plus_degrees: bool,
    pub inversion: bool,
    pub hypotenuse: bool,
    pub front_plus: bool,
    pub distance_plus: bool,
}

pub fn hwnd_from_window(window: &Window) -> HWND {
    match window.window_handle().expect("window handle").as_raw() {
        RawWindowHandle::Win32(handle) => HWND(handle.hwnd.get() as *mut core::ffi::c_void),
        _ => panic!("ProtractorPlus supports Windows only"),
    }
}

pub unsafe fn configure_overlay(hwnd: HWND) {
    let current = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
    let style = current | WS_EX_LAYERED.0 | WS_EX_TOOLWINDOW.0;
    SetWindowLongW(hwnd, GWL_EXSTYLE, style as i32);
    let _ = SetWindowPos(
        hwnd,
        HWND_TOPMOST,
        0,
        0,
        0,
        0,
        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
    );
}

pub unsafe fn ensure_topmost(hwnd: HWND) {
    let _ = SetWindowPos(
        hwnd,
        HWND_TOPMOST,
        0,
        0,
        0,
        0,
        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
    );
}

pub unsafe fn set_click_through(hwnd: HWND, enabled: bool) {
    let mut style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
    if enabled {
        style |= WS_EX_TRANSPARENT.0;
    } else {
        style &= !WS_EX_TRANSPARENT.0;
    }
    SetWindowLongW(hwnd, GWL_EXSTYLE, style as i32);
}

pub unsafe fn is_minimized(hwnd: HWND) -> bool {
    IsIconic(hwnd).as_bool()
}

pub unsafe fn minimize_window(hwnd: HWND) {
    let _ = ShowWindow(hwnd, SW_MINIMIZE);
}

pub unsafe fn restore_window(hwnd: HWND) {
    let _ = ShowWindow(hwnd, SW_RESTORE);
    ensure_topmost(hwnd);
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe fn append_toggle_item(
    menu: HMENU,
    id: u32,
    text: &str,
    checked: bool,
) {
    let label = wide(text);
    let flags = if checked {
        MF_STRING | MF_CHECKED
    } else {
        MF_STRING
    };
    let _ = AppendMenuW(menu, flags, id as usize, PCWSTR(label.as_ptr()));
}

pub unsafe fn show_context_menu(hwnd: HWND, state: ContextMenuState) -> Option<u32> {
    let menu: HMENU = CreatePopupMenu().ok()?;

    append_toggle_item(menu, MENU_BISECTOR, "Бісектриса", state.bisector);
    append_toggle_item(menu, MENU_PLUS, "Плюс", state.plus);
    append_toggle_item(
        menu,
        MENU_PLUS_DEGREES,
        "+ градуси",
        state.plus_degrees,
    );
    append_toggle_item(menu, MENU_INVERSION, "Інверсія", state.inversion);
    append_toggle_item(
        menu,
        MENU_HYPOTENUSE,
        "Гіпотенуза",
        state.hypotenuse,
    );
    append_toggle_item(menu, MENU_FRONT_PLUS, "Фронт +", state.front_plus);
    append_toggle_item(
        menu,
        MENU_DISTANCE_PLUS,
        "Дистанція +",
        state.distance_plus,
    );

    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR(std::ptr::null()));

    let minimize = wide("Згорнути");
    let close = wide("Закрити");
    let _ = AppendMenuW(
        menu,
        MF_STRING,
        MENU_MINIMIZE as usize,
        PCWSTR(minimize.as_ptr()),
    );
    let _ = AppendMenuW(
        menu,
        MF_STRING,
        MENU_CLOSE as usize,
        PCWSTR(close.as_ptr()),
    );

    let mut point = POINT::default();
    if windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut point).is_err() {
        let _ = DestroyMenu(menu);
        return None;
    }
    let _ = SetForegroundWindow(hwnd);
    let command = TrackPopupMenu(
        menu,
        TPM_LEFTALIGN | TPM_RIGHTBUTTON | TPM_RETURNCMD,
        point.x,
        point.y,
        0,
        hwnd,
        None,
    );
    let _ = PostMessageW(hwnd, WM_NULL, WPARAM(0), LPARAM(0));
    let _ = DestroyMenu(menu);
    if command.0 == 0 {
        None
    } else {
        Some(command.0 as u32)
    }
}

pub unsafe fn present_pixmap(hwnd: HWND, pixmap: &Pixmap, x: i32, y: i32) {
    let width = pixmap.width() as i32;
    let height = pixmap.height() as i32;
    if width <= 0 || height <= 0 {
        return;
    }

    let screen_dc = GetDC(HWND::default());
    if screen_dc.0.is_null() {
        return;
    }
    let memory_dc = CreateCompatibleDC(screen_dc);
    if memory_dc.0.is_null() {
        let _ = ReleaseDC(HWND::default(), screen_dc);
        return;
    }

    let mut bmi = BITMAPINFO::default();
    bmi.bmiHeader = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: width,
        biHeight: -height,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
    };
    let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
    let bitmap: HBITMAP = CreateDIBSection(
        memory_dc,
        &bmi,
        DIB_RGB_COLORS,
        &mut bits,
        None,
        0,
    )
    .unwrap_or_default();
    if bitmap.0.is_null() || bits.is_null() {
        let _ = DeleteDC(memory_dc);
        let _ = ReleaseDC(HWND::default(), screen_dc);
        return;
    }

    // tiny-skia stores premultiplied RGBA; Windows DIB expects premultiplied BGRA.
    let dst = std::slice::from_raw_parts_mut(bits as *mut u8, (width * height * 4) as usize);
    for (src, out) in pixmap.data().chunks_exact(4).zip(dst.chunks_exact_mut(4)) {
        out[0] = src[2];
        out[1] = src[1];
        out[2] = src[0];
        out[3] = src[3];
    }

    let old = SelectObject(memory_dc, HGDIOBJ(bitmap.0));
    let destination = POINT { x, y };
    let source = POINT { x: 0, y: 0 };
    let size = SIZE {
        cx: width,
        cy: height,
    };
    let blend = BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: 255,
        AlphaFormat: AC_SRC_ALPHA as u8,
    };
    let _ = UpdateLayeredWindow(
        hwnd,
        screen_dc,
        Some(&destination),
        Some(&size),
        memory_dc,
        Some(&source),
        COLORREF(0),
        Some(&blend),
        ULW_ALPHA,
    );

    let _ = SelectObject(memory_dc, old);
    let _ = DeleteObject(HGDIOBJ(bitmap.0));
    let _ = DeleteDC(memory_dc);
    let _ = ReleaseDC(HWND::default(), screen_dc);
}
