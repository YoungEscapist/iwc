//! Small desktop integration: local control socket, touch and touchpad swipes.
use crate::prelude::*;
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixDatagram;
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub struct Desktop {
    socket: UnixDatagram,
    source: *mut wl_event_source,
    path: PathBuf,
    pub status: String,
    swipe: Option<WindowGesture>,
    scroll_drag: Option<WindowGesture>,
    scroll_at: Instant,
    pinch: Option<(*mut Client, bool)>,
    pub pointer_touch: Option<i32>,
    pub touches: Vec<(*mut wlr_input_device, Listener)>,
}

pub fn runtime_path(suffix: &str) -> PathBuf {
    let display = std::env::var("WAYLAND_DISPLAY").unwrap_or_default();
    PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").unwrap())
        .join(format!("iwc-{}.{suffix}", display.replace('/', "_")))
}

impl Desktop {
    pub unsafe fn new() -> std::io::Result<Self> {
        let path = runtime_path("sock");
        let socket = UnixDatagram::bind(&path)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        socket.set_nonblocking(true)?;
        let source = wl_event_loop_add_fd(
            g!().event_loop,
            socket.as_raw_fd(),
            WL_EVENT_READABLE,
            Some(control),
            null_mut(),
        );
        if source.is_null() {
            let _ = std::fs::remove_file(&path);
            return Err(std::io::Error::other("cannot watch control socket"));
        }
        Ok(Self {
            socket,
            source,
            path,
            status: String::new(),
            swipe: None,
            scroll_drag: None,
            scroll_at: Instant::now(),
            pinch: None,
            pointer_touch: None,
            touches: Vec::new(),
        })
    }
}

impl Drop for Desktop {
    fn drop(&mut self) {
        unsafe {
            wl_event_source_remove(self.source);
        }
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_file(runtime_path("status"));
    }
}

unsafe extern "C" fn control(_: i32, _: u32, _: *mut c_void) -> i32 {
    // Read one datagram at a time, without holding a reference while actions
    // re-enter the compositor and publish their new state.
    for _ in 0..32 {
        let mut buf = [0u8; 128];
        let result = g!().desktop.as_ref().unwrap().socket.recv(&mut buf);
        let Ok(n) = result else { break };
        if g!().locked {
            continue;
        }
        let Ok(command) = std::str::from_utf8(&buf[..n]) else {
            continue;
        };
        if let Some(action) = parse_command(command) {
            run_action(action);
        }
    }
    0
}

pub fn parse_command(command: &str) -> Option<Action> {
    let mut args = command.split_whitespace();
    let cmd = args.next()?;
    let arg = args.next();
    if args.next().is_some() {
        return None;
    }
    match (cmd, arg) {
        ("view", Some(n)) | ("tag", Some(n)) => {
            let n: u32 = n.parse().ok()?;
            if !(1..=TAGCOUNT).contains(&n) {
                return None;
            }
            Some(if cmd == "view" {
                Action::View(1 << (n - 1))
            } else {
                Action::Tag(1 << (n - 1))
            })
        }
        ("next", None) => Some(Action::StepTag(1)),
        ("prev", None) => Some(Action::StepTag(-1)),
        ("focus-next", None) => Some(Action::FocusStack(1)),
        ("focus-prev", None) => Some(Action::FocusStack(-1)),
        ("layout", None) => Some(Action::CycleLayout),
        ("tiling", None) => Some(Action::SetLayout(Some(0))),
        ("floating", None) => Some(Action::SetLayout(Some(1))),
        ("toggle-floating", None) => Some(Action::ToggleFloating),
        ("reload", None) => Some(Action::ReloadConfig),
        ("focus-left", None) => Some(Action::FocusDirection(-1, 0)),
        ("focus-right", None) => Some(Action::FocusDirection(1, 0)),
        ("focus-up", None) => Some(Action::FocusDirection(0, -1)),
        ("focus-down", None) => Some(Action::FocusDirection(0, 1)),
        ("pan-left", None) => Some(Action::Pan(-1, 0)),
        ("pan-right", None) => Some(Action::Pan(1, 0)),
        ("pan-up", None) => Some(Action::Pan(0, -1)),
        ("pan-down", None) => Some(Action::Pan(0, 1)),
        ("zoom-in", None) => Some(Action::CanvasZoom(1)),
        ("zoom-out", None) => Some(Action::CanvasZoom(-1)),
        ("zoom-reset", None) => Some(Action::CanvasZoom(0)),
        ("center", None) => Some(Action::CenterWindow),
        ("fullscreen", None) => Some(Action::ToggleFullscreen),
        ("close", None) => Some(Action::KillClient),
        ("launcher", None) => Some(Action::Spawn(MENUCMD)),
        ("terminal", None) => Some(Action::Spawn(TERMCMD)),
        _ => None,
    }
}

pub unsafe fn step_tag(step: i32) {
    let m = g!().selmon;
    if m.is_null() {
        return;
    }
    let current = (*m).tagset[(*m).seltags].trailing_zeros() as i32;
    let next = (current + step).rem_euclid(TAGCOUNT as i32);
    view(1 << next);
}

pub unsafe fn publish(status: &str) {
    let Some(desktop) = g!().desktop.as_mut() else {
        return;
    };
    if desktop.status == status {
        return;
    }
    let path = runtime_path("status");
    let tmp = path.with_extension("status.tmp");
    if std::fs::write(&tmp, status).is_ok() {
        let _ = std::fs::rename(tmp, path);
        desktop.status = status.to_owned();
    }
}

#[derive(Clone, Copy)]
struct WindowGesture {
    client: *mut Client,
    original: wlr_box,
    pointer: crate::infinity::Point,
    floating: bool,
    resize: bool,
    left: bool,
    top: bool,
    dx: f64,
    dy: f64,
}

fn window_gesture_enabled(mods: u32, fingers: u32, resize: bool) -> bool {
    cleanmask(mods) == MODKEY && fingers == if resize { 3 } else { 2 }
}

fn pinch_crossed_threshold(scale: f64) -> bool {
    scale.is_finite()
        && scale > 0.0
        && (scale <= PINCH_IN_THRESHOLD || scale >= PINCH_OUT_THRESHOLD)
}

unsafe fn gesture_target() -> *mut Client {
    let cursor = g!().cursor;
    let c = xytonode((*cursor).x, (*cursor).y).c;
    if !g!().locked && g!().active_constraint.is_null() && (*g!().seat).drag.is_null()
        && g!().cursor_mode == CursorMode::Normal
        && !c.is_null() && g!().clients.contains(&c) && !client_is_unmanaged(c) {
        c
    } else {
        null_mut()
    }
}

unsafe fn begin_window_gesture(resize: bool) -> Option<WindowGesture> {
    let c = gesture_target();
    if c.is_null() || (*c).isfullscreen {
        return None;
    }
    let geom = (*c).geom;
    let origin = crate::infinity::origin(c);
    Some(WindowGesture {
        client: c,
        original: geom,
        pointer: crate::infinity::Point { x: (*g!().cursor).x, y: (*g!().cursor).y },
        floating: (*c).isfloating,
        resize,
        left: (*g!().cursor).x < origin.0 as f64 + geom.width as f64 * crate::infinity::scale(c) / 2.0,
        top: (*g!().cursor).y < origin.1 as f64 + geom.height as f64 * crate::infinity::scale(c) / 2.0,
        dx: 0.0,
        dy: 0.0,
    })
}

unsafe fn apply_window_gesture(gesture: WindowGesture) -> bool {
    let c = gesture.client;
    if g!().locked || !g!().active_constraint.is_null() || !(*g!().seat).drag.is_null()
        || g!().cursor_mode != CursorMode::Normal
        || !g!().clients.contains(&c) || (*c).isfullscreen {
        return false;
    }
    if gesture.dx == 0.0 && gesture.dy == 0.0 {
        return true;
    }
    focusclient(c, true);
    if !(*c).isfloating && !crate::workspace::floating((*c).mon) { setfloating(c, true); }
    let mut geom = gesture.original;
    let gain = if gesture.resize {
        TOUCHPAD_RESIZE_GAIN
    } else {
        TOUCHPAD_MOVE_GAIN
    };
    let (dx, dy) = (
        (gesture.dx * gain / crate::infinity::scale(c)).round() as i32,
        (gesture.dy * gain / crate::infinity::scale(c)).round() as i32,
    );
    if gesture.resize {
        let min_size = 64.max(2 * (*c).bw + 1);
        if gesture.left {
            geom.width = (gesture.original.width - dx).max(min_size);
            geom.x += gesture.original.width - geom.width;
        } else {
            geom.width = (geom.width + dx).max(min_size);
        }
        if gesture.top {
            geom.height = (gesture.original.height - dy).max(min_size);
            geom.y += gesture.original.height - geom.height;
        } else {
            geom.height = (geom.height + dy).max(min_size);
        }
    } else {
        geom.x += dx;
        geom.y += dy;
    }
    resize(c, geom, true);
    let actual = (*c).geom;
    let scale = crate::infinity::scale(c);
    let dx = actual.x - gesture.original.x
        + if gesture.resize && !gesture.left { actual.width - gesture.original.width } else { 0 };
    let dy = actual.y - gesture.original.y
        + if gesture.resize && !gesture.top { actual.height - gesture.original.height } else { 0 };
    wlr_cursor_warp_closest(g!().cursor, null_mut(),
        gesture.pointer.x + dx as f64 * scale, gesture.pointer.y + dy as f64 * scale);
    motionnotify0();
    wlr_idle_notifier_v1_notify_activity(g!().idle_notifier, g!().seat);
    true
}

unsafe fn finish_window_gesture(gesture: WindowGesture, cancelled: bool) {
    let c = gesture.client;
    if g!().locked || !g!().clients.contains(&c) || (*c).isfullscreen {
        return;
    }
    if cancelled {
        resize(c, gesture.original, true);
        setfloating(c, gesture.floating);
        wlr_cursor_warp_closest(g!().cursor, null_mut(), gesture.pointer.x, gesture.pointer.y);
        motionnotify0();
    } else if gesture.dx != 0.0 || gesture.dy != 0.0 {
        let geom = (*c).geom;
        let point = crate::infinity::screen(c,
            (geom.x + geom.width / 2) as f64,
            (geom.y + geom.height / 2) as f64,
        );
        let monitor = xytomon(point.x, point.y);
        if !monitor.is_null() && monitor != (*c).mon {
            setmon(c, monitor, 0);
        }
    }
}

pub unsafe fn cancel_gestures() {
    let gestures = g!().desktop.as_mut().map(|desktop| {
        desktop.pinch = None;
        [desktop.swipe.take(), desktop.scroll_drag.take()]
    }).unwrap_or_default();
    for gesture in gestures.into_iter().flatten() {
        finish_window_gesture(gesture, true);
    }
}

pub unsafe fn modifiers_changed() {
    if cleanmask(keyboard_mods()) != WLR_MODIFIER_ALT { crate::workspace::end_scroll(); crate::infinity::end_axis(); }
    if cleanmask(keyboard_mods()) != MODKEY {
        let gesture = g!().desktop.as_mut().and_then(|desktop| desktop.scroll_drag.take());
        if let Some(gesture) = gesture { finish_window_gesture(gesture, false); }
    }
}

pub unsafe fn touchpad_axis(event: &wlr_pointer_axis_event) -> bool {
    if event.source != WL_POINTER_AXIS_SOURCE_FINGER {
        return false;
    }
    if crate::workspace::scroll(event) { return true; }
    let Some(d) = g!().desktop.as_mut() else {
        return false;
    };
    if !window_gesture_enabled(keyboard_mods(), 2, false) || g!().locked {
        d.scroll_drag = None;
        return false;
    }
    if event.delta == 0.0 {
        if let Some(gesture) = d.scroll_drag.take() {
            finish_window_gesture(gesture, false);
        }
        return true;
    }
    let previous = d.scroll_drag.take().filter(|_| d.scroll_at.elapsed() < Duration::from_millis(200));
    d.scroll_at = Instant::now();
    let Some(mut gesture) = previous.or_else(|| begin_window_gesture(false)) else {
        return false;
    };
    if event.orientation == WL_POINTER_AXIS_HORIZONTAL_SCROLL {
        gesture.dx += event.delta;
    } else {
        gesture.dy += event.delta;
    }
    if apply_window_gesture(gesture) {
        g!().desktop.as_mut().unwrap().scroll_drag = Some(gesture);
    }
    true
}

pub unsafe fn swipe_begin(_: *mut c_void, data: *mut c_void) {
    let e = &*(data as *mut wlr_pointer_swipe_begin_event);
    cancel_gestures();
    crate::infinity::cancel_gestures();
    crate::infinity::swipe_begin(e);
    let gesture = if window_gesture_enabled(keyboard_mods(), e.fingers, true) {
        begin_window_gesture(true)
    } else {
        None
    };
    if let Some(d) = g!().desktop.as_mut() {
        d.swipe = gesture;
    }
}

pub unsafe fn swipe_update(_: *mut c_void, data: *mut c_void) {
    let e = &*(data as *mut wlr_pointer_swipe_update_event);
    if crate::infinity::swipe_update(e) {
        return;
    }
    let gesture = g!().desktop.as_mut().and_then(|d| d.swipe.take());
    if let Some(mut gesture) = gesture {
        gesture.dx += e.dx;
        gesture.dy += e.dy;
        if apply_window_gesture(gesture) {
            g!().desktop.as_mut().unwrap().swipe = Some(gesture);
        }
    }
}

pub unsafe fn swipe_end(_: *mut c_void, data: *mut c_void) {
    let e = &*(data as *mut wlr_pointer_swipe_end_event);
    crate::infinity::swipe_end(e.cancelled);
    let gesture = g!().desktop.as_mut().and_then(|d| d.swipe.take());
    if let Some(gesture) = gesture {
        finish_window_gesture(gesture, e.cancelled);
    }
}

pub unsafe fn pinch_begin(_: *mut c_void, data: *mut c_void) {
    let e = &*(data as *mut wlr_pointer_pinch_begin_event);
    cancel_gestures();
    crate::infinity::cancel_gestures();
    crate::infinity::pinch_begin(e);
    let c = if cleanmask(keyboard_mods()) == WLR_MODIFIER_ALT && e.fingers == 3 {
        gesture_target()
    } else {
        null_mut()
    };
    if let Some(d) = g!().desktop.as_mut() {
        d.pinch = if c.is_null() { None } else { Some((c, false)) };
    }
}

pub unsafe fn pinch_update(_: *mut c_void, data: *mut c_void) {
    let e = &*(data as *mut wlr_pointer_pinch_update_event);
    crate::infinity::pinch_update(e);
    if let Some((client, false)) = g!().desktop.as_ref().and_then(|desktop| desktop.pinch) {
        if !g!().locked && pinch_crossed_threshold(e.scale) && g!().clients.contains(&client) {
            g!().desktop.as_mut().unwrap().pinch = Some((client, true));
            focusclient(client, true);
            setfullscreen(client, !(*client).isfullscreen);
        }
    }
}

pub unsafe fn pinch_end(_: *mut c_void, data: *mut c_void) {
    let e = &*(data as *mut wlr_pointer_pinch_end_event);
    crate::infinity::pinch_end(e.cancelled);
    if let Some(desktop) = g!().desktop.as_mut() { desktop.pinch = None; }
}

pub unsafe fn update_caps() {
    let mut caps = WL_SEAT_CAPABILITY_POINTER;
    if !wl_list_is_empty(&(*(*g!().kb_group).wlr_group).devices) {
        caps |= WL_SEAT_CAPABILITY_KEYBOARD;
    }
    if g!().desktop.as_ref().is_some_and(|d| !d.touches.is_empty()) {
        caps |= WL_SEAT_CAPABILITY_TOUCH;
    }
    wlr_seat_set_capabilities(g!().seat, caps);
}

unsafe fn destroy_touch(_: *mut c_void, data: *mut c_void) {
    let device = data as *mut wlr_input_device;
    cancel_touches();
    if let Some(d) = g!().desktop.as_mut() {
        d.touches.retain(|(p, _)| *p != device);
    }
    update_caps();
}

pub unsafe fn create_touch(device: *mut wlr_input_device) {
    wlr_cursor_attach_input_device(g!().cursor, device);
    // The built-in ELAN panel belongs to the laptop display, even when an
    // external monitor is connected. Other touch devices retain auto mapping.
    if cstr((*device).name).starts_with("ELAN901C") {
        for &m in &g!().mons {
            if cstr((*(*m).wlr_output).name).starts_with("eDP-") {
                wlr_cursor_map_input_to_output(g!().cursor, device, (*m).wlr_output);
                break;
            }
        }
    }
    let destroy = Listener::new(
        &mut (*device).events.destroy,
        null_mut::<c_void>(),
        destroy_touch,
    );
    if let Some(d) = g!().desktop.as_mut() {
        d.touches.push((device, destroy));
    }
}

unsafe fn touch_coords(touch: *mut wlr_touch, x: f64, y: f64) -> (f64, f64) {
    let (mut lx, mut ly) = (0.0, 0.0);
    wlr_cursor_absolute_to_layout_coords(g!().cursor, &mut (*touch).base, x, y, &mut lx, &mut ly);
    (lx, ly)
}

pub unsafe fn touch_down(_: *mut c_void, data: *mut c_void) {
    let e = &*(data as *mut wlr_touch_down_event);
    wlr_idle_notifier_v1_notify_activity(g!().idle_notifier, g!().seat);
    let (x, y) = touch_coords(e.touch, e.x, e.y);
    let hit = xytonode(x, y);
    if hit.surface.is_null() {
        return;
    }
    if g!().locked {
        let m = xytomon(x, y);
        if m.is_null()
            || (*m).lock_surface.is_null()
            || wlr_surface_get_root_surface(hit.surface) != (*(*m).lock_surface).surface
        {
            return;
        }
    }
    if !g!().locked {
        g!().selmon = xytomon(x, y);
        if !hit.c.is_null() {
            focusclient(hit.c, true);
        } else if !hit.l.is_null() && (*(*hit.l).layer_surface).current.keyboard_interactive != 0 {
            focusclient(null_mut(), false);
            client_notify_enter(hit.surface, wlr_seat_get_keyboard(g!().seat));
        }
    }
    if wlr_surface_accepts_touch(hit.surface, g!().seat) {
        wlr_seat_touch_notify_down(
            g!().seat,
            hit.surface,
            e.time_msec,
            e.touch_id,
            hit.nx,
            hit.ny,
        );
    } else if g!()
        .desktop
        .as_ref()
        .is_some_and(|d| d.pointer_touch.is_none())
    {
        // Xwayland and pointer-only clients still get tap and drag.
        g!().desktop.as_mut().unwrap().pointer_touch = Some(e.touch_id);
        wlr_cursor_warp(g!().cursor, null_mut(), x, y);
        wlr_seat_pointer_notify_enter(g!().seat, hit.surface, hit.nx, hit.ny);
        wlr_seat_pointer_notify_button(
            g!().seat,
            e.time_msec,
            BTN_LEFT,
            WL_POINTER_BUTTON_STATE_PRESSED,
        );
        wlr_seat_pointer_notify_frame(g!().seat);
    }
}

pub unsafe fn touch_motion(_: *mut c_void, data: *mut c_void) {
    let e = &*(data as *mut wlr_touch_motion_event);
    wlr_idle_notifier_v1_notify_activity(g!().idle_notifier, g!().seat);
    let (x, y) = touch_coords(e.touch, e.x, e.y);
    if g!()
        .desktop
        .as_ref()
        .is_some_and(|d| d.pointer_touch == Some(e.touch_id))
    {
        wlr_cursor_warp(g!().cursor, null_mut(), x, y);
        let surface = (*g!().seat).pointer_state.focused_surface;
        if let Some((sx, sy)) = surface_coords(surface, x, y) {
            wlr_seat_pointer_notify_motion(g!().seat, e.time_msec, sx, sy);
            wlr_seat_pointer_notify_frame(g!().seat);
        }
        return;
    }
    let point = wlr_seat_touch_get_point(g!().seat, e.touch_id);
    if !point.is_null() {
        if let Some((sx, sy)) = surface_coords((*point).surface, x, y) {
            wlr_seat_touch_notify_motion(g!().seat, e.time_msec, e.touch_id, sx, sy);
        }
    }
}

// Track the original surface during a drag, including popups/subsurfaces.
unsafe fn surface_coords(surface: *mut wlr_surface, x: f64, y: f64) -> Option<(f64, f64)> {
    if surface.is_null() {
        return None;
    }
    struct Search {
        surface: *mut wlr_surface,
        origin: Option<(i32, i32)>,
    }
    unsafe extern "C" fn find(buffer: *mut wlr_scene_buffer, sx: i32, sy: i32, data: *mut c_void) {
        let search = &mut *(data as *mut Search);
        let ss = wlr_scene_surface_try_from_buffer(buffer);
        if !ss.is_null() && (*ss).surface == search.surface {
            search.origin = Some((sx, sy));
        }
    }
    let mut search = Search {
        surface,
        origin: None,
    };
    wlr_scene_node_for_each_buffer(
        &mut (*g!().scene).tree.node,
        Some(find),
        (&mut search as *mut Search).cast(),
    );
    let (sx, sy) = search.origin?;
    Some((x - sx as f64, y - sy as f64))
}

pub unsafe fn touch_up(_: *mut c_void, data: *mut c_void) {
    let e = &*(data as *mut wlr_touch_up_event);
    if g!()
        .desktop
        .as_ref()
        .is_some_and(|d| d.pointer_touch == Some(e.touch_id))
    {
        g!().desktop.as_mut().unwrap().pointer_touch = None;
        wlr_seat_pointer_notify_button(
            g!().seat,
            e.time_msec,
            BTN_LEFT,
            WL_POINTER_BUTTON_STATE_RELEASED,
        );
        wlr_seat_pointer_notify_frame(g!().seat);
    } else {
        wlr_seat_touch_notify_up(g!().seat, e.time_msec, e.touch_id);
    }
}

pub unsafe fn touch_cancel(_: *mut c_void, data: *mut c_void) {
    let e = &*(data as *mut wlr_touch_cancel_event);
    let point = wlr_seat_touch_get_point(g!().seat, e.touch_id);
    if !point.is_null() {
        wlr_seat_touch_notify_cancel(g!().seat, (*point).client);
    }
    let mut up = wlr_touch_up_event {
        touch: e.touch,
        time_msec: e.time_msec,
        touch_id: e.touch_id,
    };
    touch_up(null_mut(), (&mut up as *mut wlr_touch_up_event).cast());
}

pub unsafe fn touch_frame(_: *mut c_void, _: *mut c_void) {
    wlr_seat_touch_notify_frame(g!().seat);
}

pub unsafe fn cancel_touches() {
    crate::infinity::cancel_gestures();
    if let Some(d) = g!().desktop.as_mut() {
        d.swipe = None;
        d.scroll_drag = None;
        d.pinch = None;
    }
    let pointer = g!().desktop.as_mut().and_then(|d| d.pointer_touch.take());
    if pointer.is_some() {
        wlr_seat_pointer_notify_button(
            g!().seat,
            now_msec(),
            BTN_LEFT,
            WL_POINTER_BUTTON_STATE_RELEASED,
        );
        wlr_seat_pointer_notify_frame(g!().seat);
    }
    let mut clients = Vec::new();
    for point in wl_list_items!(
        &mut (*g!().seat).touch_state.touch_points,
        wlr_touch_point,
        link
    ) {
        if !clients.contains(&(*point).client) {
            clients.push((*point).client);
        }
    }
    for client in clients {
        wlr_seat_touch_notify_cancel(g!().seat, client);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn commands_validate_tag_range_and_arguments() {
        assert!(matches!(parse_command("view 2"), Some(Action::View(2))));
        assert!(matches!(parse_command("tag 9"), Some(Action::Tag(256))));
        assert!(matches!(parse_command("zoom-in"), Some(Action::CanvasZoom(1))));
        assert!(matches!(parse_command("zoom-out"), Some(Action::CanvasZoom(-1))));
        assert!(matches!(parse_command("zoom-reset"), Some(Action::CanvasZoom(0))));
        assert!(matches!(parse_command("layout"), Some(Action::CycleLayout)));
        assert!(matches!(parse_command("tiling"), Some(Action::SetLayout(Some(0)))));
        assert!(matches!(parse_command("floating"), Some(Action::SetLayout(Some(1)))));
        assert!(matches!(parse_command("reload"), Some(Action::ReloadConfig)));
        for cmd in [
            "view 0",
            "view 32",
            "tag -1",
            "close extra",
            "view 1 extra",
            "quit",
            "move-left",
            "move-right",
            "move-up",
            "move-down",
            "",
            "monocle",
            "tile",
            "pan-right extra",
            "focus-up 1",
        ] {
            assert!(parse_command(cmd).is_none(), "{cmd}");
        }
        assert!(matches!(parse_command("focus-left"), Some(Action::FocusDirection(-1, 0))));
        assert!(matches!(parse_command("pan-right"), Some(Action::Pan(1, 0))));
        assert!(matches!(parse_command("center"), Some(Action::CenterWindow)));
    }
    #[test]
    fn parallax_bindings_require_super_and_exact_finger_count() {
        assert!(window_gesture_enabled(MODKEY, 2, false));
        assert!(window_gesture_enabled(MODKEY, 3, true));
        assert!(!window_gesture_enabled(0, 3, true));
        assert!(!window_gesture_enabled(WLR_MODIFIER_ALT, 3, true));
        assert!(!window_gesture_enabled(
            WLR_MODIFIER_ALT | WLR_MODIFIER_CTRL,
            3,
            true
        ));
        assert!(!window_gesture_enabled(WLR_MODIFIER_ALT, 4, true));
        assert!(!pinch_crossed_threshold(1.03));
        assert!(pinch_crossed_threshold(0.80));
        assert!(pinch_crossed_threshold(1.20));
        assert!(!pinch_crossed_threshold(f64::NAN));
    }

    #[test]
    #[ignore = "requires headless wlroots, pixman and foot"]
    fn touch_and_gesture_headless() {
        unsafe extern "C" {
            fn wlr_touch_init(
                touch: *mut wlr_touch,
                implementation: *const c_void,
                name: *const c_char,
            );
            fn wlr_touch_finish(touch: *mut wlr_touch);
        }
        unsafe fn pump() {
            for _ in 0..30 {
                wl_display_flush_clients(g!().dpy);
                wl_event_loop_dispatch(g!().event_loop, 10);
            }
        }
        unsafe {
            let dir = std::env::temp_dir().join(format!("iwc-input-test-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
            std::env::set_var("XDG_RUNTIME_DIR", &dir);
            std::env::set_var("IWC_CONFIG", concat!(env!("CARGO_MANIFEST_DIR"), "/config.lua"));
            std::env::set_var("WLR_BACKENDS", "headless");
            std::env::set_var("WLR_RENDERER", "pixman");
            std::env::set_var("WLR_HEADLESS_OUTPUTS", "1");
            setup();
            let display = wl_display_add_socket_auto(g!().dpy);
            assert!(!display.is_null());
            std::env::set_var("WAYLAND_DISPLAY", &*cstr(display));
            g!().desktop = Some(Desktop::new().unwrap());
            assert!(wlr_backend_start(g!().backend));
            setlayout(Some(1));

            // Use wlroots' real device and cursor signals, without touching
            // /dev/input or injecting events into the user's active session.
            let mut touch = wlr_touch::default();
            let implementation = c"test-touch".as_ptr();
            wlr_touch_init(
                &mut touch,
                (&implementation as *const *const c_char).cast(),
                c"test-touch".as_ptr(),
            );
            create_touch(&mut touch.base);
            update_caps();
            assert_ne!((*g!().seat).capabilities & WL_SEAT_CAPABILITY_TOUCH, 0);

            let mut child = std::process::Command::new("foot")
                .args(["-a", "iwc-input-test", "-e", "sleep", "30"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap();
            for _ in 0..20 {
                pump();
                if !g!().clients.is_empty() {
                    break;
                }
            }
            assert!(!g!().clients.is_empty(), "foot must map a test surface");
            let mut down = wlr_touch_down_event {
                touch: &mut touch,
                time_msec: 100,
                touch_id: 7,
                x: 0.4,
                y: 0.4,
            };
            wl_signal_emit_mutable(
                &mut (*g!().cursor).events.touch_down,
                (&mut down as *mut wlr_touch_down_event).cast(),
            );
            let native = g!().desktop.as_ref().unwrap().pointer_touch.is_none();
            if native {
                assert_eq!(wlr_seat_touch_num_points(g!().seat), 1);
            } else {
                assert_eq!((*g!().seat).pointer_state.button_count, 1);
            }

            let mut motion = wlr_touch_motion_event {
                touch: &mut touch,
                time_msec: 110,
                touch_id: 7,
                x: 0.6,
                y: 0.6,
            };
            wl_signal_emit_mutable(
                &mut (*g!().cursor).events.touch_motion,
                (&mut motion as *mut wlr_touch_motion_event).cast(),
            );
            let mut up = wlr_touch_up_event {
                touch: &mut touch,
                time_msec: 120,
                touch_id: 7,
            };
            wl_signal_emit_mutable(
                &mut (*g!().cursor).events.touch_up,
                (&mut up as *mut wlr_touch_up_event).cast(),
            );
            assert_eq!(wlr_seat_touch_num_points(g!().seat), 0);
            assert_eq!((*g!().seat).pointer_state.button_count, 0);

            touch_down(null_mut(), (&mut down as *mut wlr_touch_down_event).cast());
            cancel_touches();
            assert_eq!(wlr_seat_touch_num_points(g!().seat), 0);
            assert_eq!((*g!().seat).pointer_state.button_count, 0);
            g!().locked = true;
            touch_down(null_mut(), (&mut down as *mut wlr_touch_down_event).cast());
            assert_eq!(wlr_seat_touch_num_points(g!().seat), 0);
            assert_eq!((*g!().seat).pointer_state.button_count, 0);
            g!().locked = false;
            touch_down(null_mut(), (&mut down as *mut wlr_touch_down_event).cast());
            let mut cancel = wlr_touch_cancel_event {
                touch: &mut touch,
                time_msec: 130,
                touch_id: 7,
            };
            touch_cancel(
                null_mut(),
                (&mut cancel as *mut wlr_touch_cancel_event).cast(),
            );
            touch_frame(null_mut(), null_mut());
            assert_eq!(wlr_seat_touch_num_points(g!().seat), 0);
            assert_eq!((*g!().seat).pointer_state.button_count, 0);

            let mut begin = wlr_pointer_swipe_begin_event {
                pointer: null_mut(),
                time_msec: 200,
                fingers: 3,
            };
            let mut update = wlr_pointer_swipe_update_event {
                pointer: null_mut(),
                time_msec: 210,
                fingers: 3,
                dx: -160.0,
                dy: 0.0,
            };
            let mut end = wlr_pointer_swipe_end_event {
                pointer: null_mut(),
                time_msec: 220,
                cancelled: true,
            };
            for (locked, cancelled, expected) in
                [(false, true, 1), (false, false, 1), (true, false, 1)]
            {
                g!().locked = locked;
                end.cancelled = cancelled;
                swipe_begin(
                    null_mut(),
                    (&mut begin as *mut wlr_pointer_swipe_begin_event).cast(),
                );
                swipe_update(
                    null_mut(),
                    (&mut update as *mut wlr_pointer_swipe_update_event).cast(),
                );
                swipe_end(
                    null_mut(),
                    (&mut end as *mut wlr_pointer_swipe_end_event).cast(),
                );
                let m = g!().selmon;
                assert_eq!((*m).tagset[(*m).seltags], expected);
            }
            g!().locked = false;

            let c = (&g!().clients)[0];
            let camera = g!().canvas.position;
            crate::infinity::pan(-camera.x, -camera.y);
            setfloating(c, true);
            resize(c, wlr_box(200, 180, 400, 250), true);
            wlr_cursor_warp(g!().cursor, null_mut(), 280.0, 240.0);
            let kb = wlr_seat_get_keyboard(g!().seat);
            let modifiers = (*kb).modifiers;
            (*kb).modifiers.depressed = 1 << (*kb).mod_indexes[6];
            assert_eq!(keyboard_mods(), MODKEY);
            let mut axis = wlr_pointer_axis_event {
                source: WL_POINTER_AXIS_SOURCE_FINGER,
                orientation: WL_POINTER_AXIS_HORIZONTAL_SCROLL,
                delta: 12.0,
                ..Default::default()
            };
            assert!(touchpad_axis(&axis));
            assert_eq!((*c).geom.x, 230);
            assert_eq!((*c).geom.width, 400);
            assert_eq!(((*g!().cursor).x, (*g!().cursor).y), (310.0, 240.0));
            axis.delta = 0.0;
            assert!(touchpad_axis(&axis));
            assert!(g!().desktop.as_ref().unwrap().scroll_drag.is_none());

            update.dx = -20.0;
            update.dy = -10.0;
            swipe_begin(
                null_mut(),
                (&mut begin as *mut wlr_pointer_swipe_begin_event).cast(),
            );
            swipe_update(
                null_mut(),
                (&mut update as *mut wlr_pointer_swipe_update_event).cast(),
            );
            assert_eq!((*c).geom.width, 460);
            assert_eq!((*c).geom.height, 280);
            assert_eq!(((*g!().cursor).x, (*g!().cursor).y), (250.0, 210.0));
            end.cancelled = true;
            swipe_end(
                null_mut(),
                (&mut end as *mut wlr_pointer_swipe_end_event).cast(),
            );
            assert_eq!((*c).geom.width, 400);
            assert_eq!((*c).geom.x, 230);
            assert_eq!(((*g!().cursor).x, (*g!().cursor).y), (310.0, 240.0));
            swipe_begin(
                null_mut(),
                (&mut begin as *mut wlr_pointer_swipe_begin_event).cast(),
            );
            swipe_update(
                null_mut(),
                (&mut update as *mut wlr_pointer_swipe_update_event).cast(),
            );
            end.cancelled = false;
            swipe_end(
                null_mut(),
                (&mut end as *mut wlr_pointer_swipe_end_event).cast(),
            );
            assert_eq!((*c).geom.width, 460);

            (*kb).modifiers.depressed = 1 << (*kb).mod_indexes[3];
            assert_eq!(keyboard_mods(), WLR_MODIFIER_ALT);
            let mut pbegin = wlr_pointer_pinch_begin_event {
                fingers: 3,
                ..Default::default()
            };
            let mut pupdate = wlr_pointer_pinch_update_event {
                fingers: 3,
                scale: 0.7,
                ..Default::default()
            };
            let mut pend = wlr_pointer_pinch_end_event::default();
            pinch_begin(
                null_mut(),
                (&mut pbegin as *mut wlr_pointer_pinch_begin_event).cast(),
            );
            pinch_update(
                null_mut(),
                (&mut pupdate as *mut wlr_pointer_pinch_update_event).cast(),
            );
            assert!(
                (*c).isfullscreen,
                "Parallax threshold fires immediately, once per gesture"
            );
            pinch_update(null_mut(), (&mut pupdate as *mut wlr_pointer_pinch_update_event).cast());
            assert!((*c).isfullscreen);
            pinch_end(
                null_mut(),
                (&mut pend as *mut wlr_pointer_pinch_end_event).cast(),
            );
            assert!((*c).isfullscreen);
            pupdate.scale = 1.3;
            pinch_begin(
                null_mut(),
                (&mut pbegin as *mut wlr_pointer_pinch_begin_event).cast(),
            );
            pinch_update(
                null_mut(),
                (&mut pupdate as *mut wlr_pointer_pinch_update_event).cast(),
            );
            pinch_end(
                null_mut(),
                (&mut pend as *mut wlr_pointer_pinch_end_event).cast(),
            );
            assert!(!(*c).isfullscreen);
            pend.cancelled = true;
            pupdate.scale = 1.03;
            pinch_begin(
                null_mut(),
                (&mut pbegin as *mut wlr_pointer_pinch_begin_event).cast(),
            );
            pinch_update(
                null_mut(),
                (&mut pupdate as *mut wlr_pointer_pinch_update_event).cast(),
            );
            pinch_end(
                null_mut(),
                (&mut pend as *mut wlr_pointer_pinch_end_event).cast(),
            );
            assert!(!(*c).isfullscreen);
            (*kb).modifiers = modifiers;

            setlayout(Some(0));
            assert!(!(*c).isfloating, "transient resizing must retile on mode switch");
            assert!(!(*c).float_pinned);
            let tiled = (*c).geom;
            wlr_cursor_warp(g!().cursor, null_mut(), (tiled.x + 100) as f64, (tiled.y + 100) as f64);
            (*kb).modifiers.depressed = 1 << (*kb).mod_indexes[6];
            axis.delta = 12.0;
            assert!(touchpad_axis(&axis));
            assert!((*c).isfloating);
            assert_eq!((*c).geom.x, tiled.x + 30);
            view(2);
            assert!(g!().desktop.as_ref().unwrap().scroll_drag.is_none());
            view(1);
            assert!(!(*c).isfloating, "workspace change cancels the in-flight drag");
            assert_eq!((*c).geom.x, tiled.x);
            togglefloating();
            assert!((*c).float_pinned);
            setlayout(Some(1));
            setlayout(Some(0));
            assert!((*c).isfloating && (*c).float_pinned, "explicit floating survives workspace mode changes");
            togglefloating();
            assert!(!(*c).float_pinned);
            (*c).floating_geom = wlr_box::default();
            let tiling_geometry = (*c).geom;
            setlayout(Some(1));
            assert!((*c).geom.height < tiling_geometry.height, "first floating entry scatters instead of retaining the tile");
            assert!(!(*c).isfloating);
            (*kb).modifiers = modifiers;

            (*kb).modifiers.depressed = 1 << (*kb).mod_indexes[6];
            for scale in [0.5, 2.0] {
                crate::infinity::zoom_to(scale, crate::infinity::Point::default());
                let camera = g!().canvas.position;
                crate::infinity::pan(-camera.x, -camera.y);
                for (left, top) in [(true, true), (true, false), (false, true), (false, false)] {
                    resize(c, wlr_box(160, 100, 280, 180), true);
                    let point = crate::infinity::screen(c,
                        (160 + if left { 20 } else { 240 }) as f64,
                        (100 + if top { 20 } else { 140 }) as f64);
                    wlr_cursor_warp(g!().cursor, null_mut(), point.x, point.y);
                    let mut gesture = begin_window_gesture(true).unwrap();
                    gesture.dx = if left { -10.0 } else { 10.0 };
                    gesture.dy = if top { -10.0 } else { 10.0 };
                    assert!(apply_window_gesture(gesture));
                    assert_eq!(((*g!().cursor).x, (*g!().cursor).y),
                        (point.x + gesture.dx * TOUCHPAD_RESIZE_GAIN, point.y + gesture.dy * TOUCHPAD_RESIZE_GAIN));
                    finish_window_gesture(gesture, true);
                    assert_eq!(((*g!().cursor).x, (*g!().cursor).y), (point.x, point.y));
                    let mut gesture = begin_window_gesture(false).unwrap();
                    gesture.dx = 12.0;
                    gesture.dy = 8.0;
                    assert!(apply_window_gesture(gesture));
                    assert_eq!(((*g!().cursor).x, (*g!().cursor).y), (point.x + 30.0, point.y + 20.0));
                    finish_window_gesture(gesture, true);
                }
            }
            (*kb).modifiers = modifiers;

            wlr_touch_finish(&mut touch);
            assert_eq!((*g!().seat).capabilities & WL_SEAT_CAPABILITY_TOUCH, 0);
            let _ = child.kill();
            let _ = child.wait();
            pump();
            cleanup();
            std::fs::remove_dir_all(dir).unwrap();
        }
    }
}
