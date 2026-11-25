use crate::prelude::*;
#[cfg(test)]
use std::time::Instant;

const LIMIT: f64 = 8_000_000.0;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    fn bounded(self) -> Self {
        Self {
            x: self.x.clamp(-LIMIT, LIMIT),
            y: self.y.clamp(-LIMIT, LIMIT),
        }
    }
}

pub struct Canvas {
    pub workspace: (*mut Monitor, usize),
    pub position: Point,
    pub scale: f64,
    pub projection: crate::canvas_scene::Projection,
    pan_button: Option<u32>,
    pan_axis: bool,
    swipe: Option<(Point, Point, bool, bool)>,
    pinch: Option<(Point, f64, Point)>,
    hold: bool,
}

impl Default for Canvas {
    fn default() -> Self {
        Self {
            workspace: (null_mut(), 0),
            position: Point::default(),
            scale: 1.0,
            projection: Default::default(),
            pan_button: None,
            pan_axis: false,
            swipe: None,
            pinch: None,
            hold: false,
        }
    }
}

fn centered(window: wlr_box, viewport: wlr_box, scale: f64) -> Point {
    Point {
        x: window.x as f64 + window.width as f64 / 2.0
            - (viewport.x as f64 + viewport.width as f64 / 2.0) / scale,
        y: window.y as f64 + window.height as f64 / 2.0
            - (viewport.y as f64 + viewport.height as f64 / 2.0) / scale,
    }
    .bounded()
}

fn neighbor(origin: Point, windows: &[Point], direction: (i32, i32)) -> Option<usize> {
    windows
        .iter()
        .enumerate()
        .filter_map(|(index, target)| {
            let horizontal = target.x - origin.x;
            let vertical = target.y - origin.y;
            let projection = horizontal * direction.0 as f64 + vertical * direction.1 as f64;
            let distance = horizontal.hypot(vertical);
            (projection > 0.0 && distance > 1.0).then_some((index, projection / distance, distance))
        })
        .max_by(|left, right| {
            left.1
                .total_cmp(&right.1)
                .then_with(|| right.2.total_cmp(&left.2))
        })
        .map(|candidate| candidate.0)
}

pub unsafe fn offset(c: *mut Client) -> Point {
    if !c.is_null()
        && ((*c).isfullscreen
            || (!(*c).scene.is_null() && (*(*c).scene).node.parent == g!().layers[LYR_FS]))
    {
        Point::default()
    } else {
        crate::workspace::camera(if c.is_null() { g!().selmon } else { (*c).mon }).0
    }
}

pub unsafe fn scale(c: *mut Client) -> f64 {
    if !c.is_null() && ((*c).isfullscreen || (*(*c).scene).node.parent == g!().layers[LYR_FS]) {
        1.0
    } else {
        crate::workspace::camera(if c.is_null() { g!().selmon } else { (*c).mon }).1
    }
}

pub unsafe fn world(c: *mut Client, x: f64, y: f64) -> Point {
    let camera = offset(c);
    let scale = scale(c);
    Point {
        x: x / scale + camera.x,
        y: y / scale + camera.y,
    }
}

pub unsafe fn screen(c: *mut Client, x: f64, y: f64) -> Point {
    let camera = offset(c);
    let scale = scale(c);
    Point {
        x: (x - camera.x) * scale,
        y: (y - camera.y) * scale,
    }
}

pub unsafe fn origin(c: *mut Client) -> (i32, i32) {
    let mut horizontal = 0;
    let mut vertical = 0;
    wlr_scene_node_coords(&mut (*(*c).scene).node, &mut horizontal, &mut vertical);
    let position = screen(c, horizontal as f64, vertical as f64);
    (position.x.round() as i32, position.y.round() as i32)
}

unsafe fn schedule() {
    for monitor in g!().mons.clone() {
        if (*(*monitor).wlr_output).enabled && !(*monitor).asleep {
            wlr_output_schedule_frame((*monitor).wlr_output);
        }
    }
}

unsafe fn apply() {
    crate::canvas_scene::sync();
    schedule();
    motionnotify0();
    printstatus();
}

pub unsafe fn allowed() -> bool {
    let focused = g!().focused_client;
    !g!().locked
        && !g!().selmon.is_null()
        && (focused.is_null() || !(*focused).isfullscreen)
        && g!().active_constraint.is_null()
        && (*g!().seat).drag.is_null()
        && !matches!(g!().cursor_mode, CursorMode::Move | CursorMode::Resize)
}

pub unsafe fn zoom_to(target: f64, anchor: Point) {
    if !crate::workspace::floating(g!().selmon) || !allowed()
        || !target.is_finite()
        || target <= 0.0
        || !anchor.x.is_finite()
        || !anchor.y.is_finite()
    {
        return;
    }
    let target = target.clamp(0.25, 3.0);
    let before = world(null_mut(), anchor.x, anchor.y);
    g!().canvas.scale = target;
    g!().canvas.position = Point {
        x: before.x - anchor.x / target,
        y: before.y - anchor.y / target,
    }
    .bounded();
    apply();
}

pub unsafe fn zoom_by(step: i32) {
    let cursor = g!().cursor;
    let target = if step == 0 {
        1.0
    } else {
        g!().canvas.scale * 1.2_f64.powi(step)
    };
    zoom_to(
        target,
        Point {
            x: (*cursor).x,
            y: (*cursor).y,
        },
    );
}

pub unsafe fn pinch_begin(event: &wlr_pointer_pinch_begin_event) {
    let modifiers = cleanmask(keyboard_mods());
    let empty = xytonode((*g!().cursor).x, (*g!().cursor).y).c.is_null();
    let zoom = match (modifiers, event.fingers) {
        (0, 2) => empty,
        (0, 3) | (MODKEY, 2) => true,
        (WLR_MODIFIER_ALT, 2 | 3) => empty || event.fingers == 2,
        _ => false,
    };
    g!().canvas.pinch = if zoom && crate::workspace::floating(g!().selmon) && allowed() {
        Some((
            g!().canvas.position,
            g!().canvas.scale,
            Point {
                x: (*g!().cursor).x,
                y: (*g!().cursor).y,
            },
        ))
    } else {
        None
    };
}

pub unsafe fn pinch_update(event: &wlr_pointer_pinch_update_event) {
    if let Some((_, scale, anchor)) = g!().canvas.pinch {
        zoom_to(scale * event.scale, anchor);
    }
}

pub unsafe fn pinch_end(cancelled: bool) {
    if let Some((position, scale, _)) = g!().canvas.pinch.take() {
        if cancelled && !g!().locked {
            g!().canvas.position = position;
            g!().canvas.scale = scale;
            apply();
        }
    }
}

pub unsafe fn panning() -> bool {
    g!().canvas.pan_button.is_some() || g!().canvas.pan_axis
        || g!().canvas.swipe.is_some_and(|(_, _, nearest, _)| !nearest)
}

pub unsafe fn end_axis() {
    if std::mem::take(&mut g!().canvas.pan_axis) {
        wlr_cursor_set_xcursor(g!().cursor, g!().cursor_mgr, c"default".as_ptr());
        motionnotify0();
    }
}

pub unsafe fn cancel_gestures() {
    if panning() { wlr_cursor_set_xcursor(g!().cursor, g!().cursor_mgr, c"default".as_ptr()); }
    g!().canvas.pan_button = None;
    g!().canvas.pan_axis = false;
    g!().canvas.swipe = None;
    g!().canvas.pinch = None;
    g!().canvas.hold = false;
}

pub unsafe fn pan(horizontal: f64, vertical: f64) {
    if !crate::workspace::floating(g!().selmon) { return; }
    if !allowed() || !horizontal.is_finite() || !vertical.is_finite() {
        return;
    }
    let position = g!().canvas.position;
    g!().canvas.position = Point {
        x: position.x + horizontal,
        y: position.y + vertical,
    }
    .bounded();
    apply();
}

pub unsafe fn step(horizontal: i32, vertical: i32) {
    let monitor = g!().selmon;
    if !monitor.is_null() {
        pan(
            horizontal as f64 * (*monitor).w.width as f64 * 0.75 / g!().canvas.scale,
            vertical as f64 * (*monitor).w.height as f64 * 0.75 / g!().canvas.scale,
        );
    }
}

pub unsafe fn center(c: *mut Client) {
    if !c.is_null() && !crate::workspace::floating((*c).mon) { crate::workspace::reveal(c); return; }
    if c.is_null() || (*c).mon.is_null() || (*c).isfullscreen || !allowed() {
        return;
    }
    g!().canvas.position = centered((*c).geom, (*(*c).mon).w, g!().canvas.scale);
    apply();
}

unsafe fn candidates() -> Vec<*mut Client> {
    let monitor = g!().selmon;
    g!().clients
        .iter()
        .copied()
        .filter(|client| visibleon(*client, monitor))
        .collect()
}

unsafe fn jump(c: *mut Client) {
    focusclient(c, true);
    center(c);
}

pub unsafe fn focus_direction(horizontal: i32, vertical: i32) {
    if !allowed() {
        return;
    }
    let focused = g!().focused_client;
    let windows = candidates();
    if focused.is_null() || !windows.contains(&focused) {
        cycle(1);
        return;
    }
    let middle = |client: *mut Client| {
        let geometry = (*client).geom;
        Point {
            x: geometry.x as f64 + geometry.width as f64 / 2.0,
            y: geometry.y as f64 + geometry.height as f64 / 2.0,
        }
    };
    let positions: Vec<_> = windows.iter().map(|client| middle(*client)).collect();
    if let Some(index) = neighbor(middle(focused), &positions, (horizontal, vertical)) {
        jump(windows[index]);
    }
}

pub unsafe fn cycle(direction: i32) {
    if !allowed() {
        return;
    }
    let windows = candidates();
    if windows.is_empty() {
        return;
    }
    let current = windows
        .iter()
        .position(|client| *client == g!().focused_client);
    let index = current.map_or(0, |index| {
        (index as i64 + direction as i64).rem_euclid(windows.len() as i64) as usize
    });
    jump(windows[index]);
}

pub unsafe fn place(client: *mut Client, monitor: *mut Monitor) {
    let viewport = (*monitor).w;
    let camera = offset(client);
    let parent = client_get_parent(client);
    let geometry = &mut (*client).geom;
    if !parent.is_null() {
        let parent_geometry = (*parent).geom;
        geometry.x = parent_geometry.x + (parent_geometry.width - geometry.width) / 2;
        geometry.y = parent_geometry.y + (parent_geometry.height - geometry.height) / 2;
    } else {
        geometry.x = (camera.x
            + (viewport.x as f64 + viewport.width as f64 / 2.0) / scale(client)
            - geometry.width as f64 / 2.0)
            .round() as i32;
        geometry.y = (camera.y
            + (viewport.y as f64 + viewport.height as f64 / 2.0) / scale(client)
            - geometry.height as f64 / 2.0)
            .round() as i32;
    }
    geometry.x = geometry.x.clamp(-LIMIT as i32, LIMIT as i32);
    geometry.y = geometry.y.clamp(-LIMIT as i32, LIMIT as i32);
}

pub unsafe fn button(event: &wlr_pointer_button_event) -> bool {
    if event.state == WL_POINTER_BUTTON_STATE_RELEASED
        && g!().canvas.pan_button == Some(event.button)
    {
        g!().canvas.pan_button = None;
        wlr_cursor_set_xcursor(g!().cursor, g!().cursor_mgr, c"default".as_ptr());
        motionnotify0();
        return true;
    }
    if event.state == WL_POINTER_BUTTON_STATE_PRESSED
        && event.button == BTN_LEFT
        && cleanmask(keyboard_mods()) == WLR_MODIFIER_ALT
        && crate::workspace::floating(g!().selmon)
        && allowed()
    {
        g!().canvas.pan_button = Some(event.button);
        wlr_seat_pointer_clear_focus(g!().seat);
        wlr_cursor_set_xcursor(g!().cursor, g!().cursor_mgr, c"grabbing".as_ptr());
        return true;
    }
    false
}

pub unsafe fn motion(horizontal: f64, vertical: f64) -> bool {
    if g!().locked {
        g!().canvas.pan_button = None;
        return false;
    }
    if g!().canvas.pan_button.is_none() {
        return false;
    }
    pan(
        -horizontal / g!().canvas.scale,
        -vertical / g!().canvas.scale,
    );
    true
}

pub unsafe fn axis(event: &wlr_pointer_axis_event) -> bool {
    if !crate::workspace::floating(g!().selmon) { return false; }
    let modifiers = cleanmask(keyboard_mods());
    if event.source != WL_POINTER_AXIS_SOURCE_FINGER && modifiers == MODKEY && allowed() {
        if event.orientation == WL_POINTER_AXIS_VERTICAL_SCROLL {
            zoom_to(
                g!().canvas.scale * (-event.delta * 0.015).exp(),
                Point {
                    x: (*g!().cursor).x,
                    y: (*g!().cursor).y,
                },
            );
        }
        return true;
    }
    if ![WLR_MODIFIER_ALT, WLR_MODIFIER_ALT | WLR_MODIFIER_SHIFT].contains(&modifiers) || !allowed()
    {
        return false;
    }
    let horizontal = event.orientation == WL_POINTER_AXIS_HORIZONTAL_SCROLL
        || modifiers & WLR_MODIFIER_SHIFT != 0;
    let gain = if event.source == WL_POINTER_AXIS_SOURCE_FINGER { -TOUCHPAD_MOVE_GAIN } else { 3.0 };
    let distance = event.delta * gain / g!().canvas.scale;
    if event.source == WL_POINTER_AXIS_SOURCE_FINGER {
        if event.delta == 0.0 { end_axis(); return true; }
        g!().canvas.pan_axis = true;
    }
    pan(
        if horizontal { distance } else { 0.0 },
        if horizontal { 0.0 } else { distance },
    );
    true
}

pub unsafe fn swipe_begin(event: &wlr_pointer_swipe_begin_event) {
    let modifiers = cleanmask(keyboard_mods());
    let on_window = !xytonode((*g!().cursor).x, (*g!().cursor).y).c.is_null();
    let nearest = (event.fingers == 4 && modifiers == 0)
        || (event.fingers == 3 && modifiers == MODKEY && !on_window);
    let pan = event.fingers == 3 && [0, WLR_MODIFIER_ALT].contains(&modifiers)
        && crate::workspace::floating(g!().selmon);
    g!().canvas.swipe = if (nearest || pan) && allowed() {
        Some((g!().canvas.position, Point::default(), nearest, false))
    } else {
        None
    };
    if panning() { motionnotify0(); }
}

pub unsafe fn swipe_update(event: &wlr_pointer_swipe_update_event) -> bool {
    let Some((start, mut delta, nearest, fired)) = g!().canvas.swipe else { return false; };
    if !event.dx.is_finite() || !event.dy.is_finite() || !allowed() { return true; }
    delta.x += event.dx;
    delta.y += event.dy;
    let ready = delta.x.hypot(delta.y) >= 12.0
        && (delta.x.abs().max(delta.y.abs()) >= delta.x.abs().min(delta.y.abs()) * 2.0
            || delta.x.abs() + delta.y.abs() >= 36.0);
    g!().canvas.swipe = Some((start, delta, nearest, fired || ready));
    if nearest {
        if ready && !fired {
            if delta.x.abs() >= delta.y.abs() {
                focus_direction(if delta.x < 0.0 { 1 } else { -1 }, 0);
            } else {
                focus_direction(0, if delta.y < 0.0 { 1 } else { -1 });
            }
        }
        return true;
    }
    pan(
        -event.dx / g!().canvas.scale,
        -event.dy / g!().canvas.scale,
    );
    true
}

pub unsafe fn swipe_end(cancelled: bool) {
    let start = g!().canvas.swipe.take();
    if start.is_some_and(|(_, _, nearest, _)| !nearest) {
        wlr_cursor_set_xcursor(g!().cursor, g!().cursor_mgr, c"default".as_ptr());
        motionnotify0();
    }
    if cancelled && !g!().locked {
        if let Some((position, _, false, _)) = start {
            g!().canvas.position = position;
            apply();
        }
    }
}

pub unsafe fn hold_begin(_: *mut c_void, data: *mut c_void) {
    let event = &*(data as *mut wlr_pointer_hold_begin_event);
    crate::desktop::cancel_gestures();
    cancel_gestures();
    g!().canvas.hold = allowed() && matches!((cleanmask(keyboard_mods()), event.fingers), (0, 4) | (MODKEY, 3));
}

pub unsafe fn hold_end(_: *mut c_void, data: *mut c_void) {
    let event = &*(data as *mut wlr_pointer_hold_end_event);
    if std::mem::take(&mut g!().canvas.hold) && !event.cancelled && allowed() {
        center(g!().focused_client);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_centers_window_in_usable_output_including_negative_origins() {
        let window = wlr_box(-3000, 2400, 800, 600);
        let viewport = wlr_box(-1920, 36, 1920, 1044);
        let camera = centered(window, viewport, 1.0);
        assert_eq!(window.x as f64 - camera.x + 400.0, -960.0);
        assert_eq!(window.y as f64 - camera.y + 300.0, 558.0);
    }

    #[test]
    fn directional_jump_uses_alignment_then_nearest_distance() {
        let windows = [
            Point { x: 3000.0, y: 0.0 },
            Point { x: 20.0, y: 20.0 },
            Point { x: -10.0, y: 0.0 },
            Point { x: 600.0, y: 0.0 },
        ];
        assert_eq!(neighbor(Point::default(), &windows, (1, 0)), Some(3));
        assert_eq!(neighbor(Point::default(), &windows, (-1, 0)), Some(2));
        assert_eq!(neighbor(Point::default(), &windows, (0, 1)), Some(1));
        assert_eq!(neighbor(Point::default(), &windows, (0, -1)), None);
    }

    #[test]
    fn coincident_windows_and_empty_direction_do_not_jump() {
        assert_eq!(
            neighbor(Point::default(), &[Point::default()], (1, 0)),
            None
        );
        assert_eq!(
            neighbor(Point::default(), &[Point { x: 20.0, y: 0.0 }], (0, 0)),
            None
        );
        assert_eq!(neighbor(Point::default(), &[], (1, 0)), None);
    }

    #[test]
    fn zoomed_center_and_removed_keyboard_move_bindings() {
        let window = wlr_box(-3000, 2400, 800, 600);
        let viewport = wlr_box(-1920, 36, 1920, 1044);
        for scale in [0.25, 0.5, 1.0, 2.0, 3.0] {
            let camera = centered(window, viewport, scale);
            assert_eq!((window.x as f64 + 400.0 - camera.x) * scale, -960.0);
            assert_eq!((window.y as f64 + 300.0 - camera.y) * scale, 558.0);
        }
        for binding in keys() {
            assert!(
                !(binding.modifiers == MODKEY | WLR_MODIFIER_SHIFT
                    && [XKB_KEY_Left, XKB_KEY_Right, XKB_KEY_Up, XKB_KEY_Down]
                        .contains(&binding.keysym))
            );
        }
        assert!(keys().iter().any(|binding| binding.modifiers == MODKEY && binding.keysym == XKB_KEY_d && matches!(binding.action, Action::CycleLayout)));
        assert!(keys().iter().any(|binding| binding.modifiers == MODKEY && binding.keysym == XKB_KEY_t && matches!(binding.action, Action::SetLayout(Some(0)))));
    }

    #[test]
    fn extreme_camera_positions_remain_safe_for_wlroots() {
        assert_eq!(
            Point {
                x: f64::MAX,
                y: -f64::MAX
            }
            .bounded(),
            Point {
                x: LIMIT,
                y: -LIMIT
            }
        );
    }

    #[test]
    #[ignore = "requires headless wlroots, pixman, foot and Xwayland"]
    fn infinite_canvas_headless() {
        struct Children(Vec<std::process::Child>);
        impl Drop for Children {
            fn drop(&mut self) {
                for child in &mut self.0 {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
        }
        unsafe fn pump() {
            let deadline = Instant::now() + std::time::Duration::from_millis(350);
            while Instant::now() < deadline {
                wl_display_flush_clients(g!().dpy);
                wl_event_loop_dispatch(g!().event_loop, 10);
            }
        }
        unsafe {
            let directory =
                std::env::temp_dir().join(format!("iwc-canvas-test-{}", std::process::id()));
            std::fs::create_dir_all(&directory).unwrap();
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
            std::env::set_var("XDG_RUNTIME_DIR", &directory);
            std::env::set_var("IWC_CONFIG", concat!(env!("CARGO_MANIFEST_DIR"), "/config.lua"));
            std::env::set_var("WLR_BACKENDS", "headless");
            std::env::set_var("WLR_RENDERER", "pixman");
            std::env::set_var("WLR_HEADLESS_OUTPUTS", "2");
            setup();
            let display = wl_display_add_socket_auto(g!().dpy);
            assert!(!display.is_null());
            std::env::set_var("WAYLAND_DISPLAY", &*cstr(display));
            g!().desktop = Some(crate::desktop::Desktop::new().unwrap());
            assert!(wlr_backend_start(g!().backend));
            g!().selmon = *g!().mons.iter().find(|monitor| (***monitor).m.x == 0).unwrap();
            crate::workspace::activate(g!().selmon);
            assert_eq!(crate::workspace::layout(g!().selmon), 0);
            assert!(!g!().settings.niri);
            view(2);
            assert_eq!(crate::workspace::layout(g!().selmon), 0);
            view(1);
            setlayout(Some(1));
            let mut children = Children(Vec::new());
            let mut windows = Vec::new();
            for (application, scale) in [("iwc-one", 0.5), ("iwc-two", 2.0), ("iwc-three", 1.0)] {
                let viewport = (*g!().selmon).w;
                let middle = Point { x: (viewport.x + viewport.width / 2) as f64,
                    y: (viewport.y + viewport.height / 2) as f64 };
                zoom_to(scale, middle);
                pan(2000.0, 0.0);
                let camera = g!().canvas.position;
                children.0.push(
                    std::process::Command::new("foot")
                        .args(["-a", application, "-e", "sleep", "60"])
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn()
                        .unwrap(),
                );
                for _ in 0..15 {
                    pump();
                    if g!()
                        .clients
                        .iter()
                        .any(|client| client_get_appid(*client) == application)
                    {
                        break;
                    }
                }
                let client = *g!()
                    .clients
                    .iter()
                    .find(|client| client_get_appid(**client) == application)
                    .unwrap();
                windows.push(client);
                pump();
                assert_eq!(g!().focused_client, client);
                let geometry = (*client).geom;
                let point = screen(client, (geometry.x + geometry.width / 2) as f64,
                    (geometry.y + geometry.height / 2) as f64);
                assert!((point.x - middle.x).abs() <= scale && (point.y - middle.y).abs() <= scale,
                    "new windows must appear in the current camera, not beside the old off-screen focus");
                assert!((g!().canvas.position.x - camera.x).abs() <= 1.0);
                assert!((g!().canvas.position.y - camera.y).abs() <= 1.0);
            }
            let [first, second, third] = windows.as_slice() else {
                unreachable!()
            };
            let (first, second, third) = (*first, *second, *third);
            assert!((*second).geom.x >= (*first).geom.x + (*first).geom.width);
            assert!((*third).geom.x >= (*second).geom.x + (*second).geom.width);
            let original: Vec<_> = windows.iter().map(|client| (**client).geom).collect();
            let anchor = Point { x: 600.0, y: 320.0 };
            let anchored = world(third, anchor.x, anchor.y);
            for target in [0.5, 2.0, 0.25, 3.0] {
                zoom_to(target, anchor);
                pump();
                let point = world(third, anchor.x, anchor.y);
                assert!((point.x - anchored.x).abs() < 0.001);
                assert!((point.y - anchored.y).abs() < 0.001);
                assert_eq!(g!().canvas.scale, target);
                let geometry = (*third).geom;
                let point = anchor;
                let hit = xytonode(point.x, point.y);
                assert_eq!(hit.c, third);
                assert!((hit.nx - (anchored.x - geometry.x as f64 - (*third).bw as f64)).abs() < 0.01);
                assert!((hit.ny - (anchored.y - geometry.y as f64 - (*third).bw as f64)).abs() < 0.01);
                for (client, geometry) in windows.iter().zip(&original) {
                    assert_eq!(
                        (
                            (**client).geom.x,
                            (**client).geom.y,
                            (**client).geom.width,
                            (**client).geom.height
                        ),
                        (geometry.x, geometry.y, geometry.width, geometry.height)
                    );
                }
            }
            let scale_before = g!().canvas.scale;
            zoom_to(f64::NAN, anchor);
            zoom_to(0.0, anchor);
            assert_eq!(g!().canvas.scale, scale_before);
            zoom_to(1.0, anchor);
            let point = screen(third, ((*third).geom.x + 100) as f64, ((*third).geom.y + 100) as f64);
            wlr_cursor_warp(g!().cursor, null_mut(), point.x, point.y);
            assert_eq!(xytonode(point.x, point.y).c, third);
            pinch_begin(&wlr_pointer_pinch_begin_event { fingers: 2, ..Default::default() });
            pinch_update(&wlr_pointer_pinch_update_event { scale: 0.6, fingers: 2, ..Default::default() });
            assert_eq!(g!().canvas.scale, 1.0, "bare two-finger pinch over a window must not zoom the canvas");
            pinch_end(false);
            let pinch = wlr_pointer_pinch_begin_event {
                fingers: 3,
                ..Default::default()
            };
            pinch_begin(&pinch);
            pinch_update(&wlr_pointer_pinch_update_event {
                scale: 0.6,
                fingers: 3,
                ..Default::default()
            });
            assert_eq!(g!().canvas.scale, 0.6);
            pinch_end(true);
            assert_eq!(g!().canvas.scale, 1.0);
            for target in [0.5, 2.0, 1.0] {
                zoom_to(target, anchor);
                center(third);
                pump();
                children.0.push(
                    std::process::Command::new("grim")
                        .args([
                            "-o",
                            "HEADLESS-1",
                            &format!("/tmp/iwc-zoom-{target:.1}.png"),
                        ])
                        .spawn()
                        .unwrap(),
                );
                pump();
            }
            pan(4000.0, -2500.0);
            for (client, geometry) in windows.iter().zip(&original) {
                assert_eq!(
                    (
                        (**client).geom.x,
                        (**client).geom.y,
                        (**client).geom.width,
                        (**client).geom.height
                    ),
                    (geometry.x, geometry.y, geometry.width, geometry.height)
                );
            }
            center(third);
            pump();
            let monitor = g!().selmon;
            let viewport = (*monitor).w;
            let middle = (
                viewport.x + viewport.width / 2,
                viewport.y + viewport.height / 2,
            );
            assert_eq!(xytonode(middle.0 as f64, middle.1 as f64).c, third);
            focus_direction(-1, 0);
            pump();
            assert_eq!(g!().focused_client, second);
            assert_eq!(xytonode(middle.0 as f64, middle.1 as f64).c, second);
            focus_direction(-1, 0);
            pump();
            assert_eq!(g!().focused_client, first);
            focus_direction(-1, 0);
            assert_eq!(g!().focused_client, first);
            cycle(-1);
            pump();
            assert_eq!(g!().focused_client, second);

            let before = g!().canvas.position;
            let begin = wlr_pointer_swipe_begin_event {
                fingers: 3,
                ..Default::default()
            };
            swipe_begin(&begin);
            assert!(panning());
            assert!((*g!().seat).pointer_state.focused_surface.is_null());
            assert!(swipe_update(&wlr_pointer_swipe_update_event {
                fingers: 3,
                dx: 100.0,
                dy: -50.0,
                ..Default::default()
            }));
            assert_eq!(
                g!().canvas.position,
                Point {
                    x: before.x - 100.0,
                    y: before.y + 50.0
                }
            );
            swipe_end(true);
            assert!(!panning());
            assert_eq!(g!().canvas.position, before);
            focusclient(second, true);
            let directional = wlr_pointer_swipe_begin_event { fingers: 4, ..Default::default() };
            swipe_begin(&directional);
            assert!(swipe_update(&wlr_pointer_swipe_update_event { fingers: 4, dx: -5.0, ..Default::default() }));
            assert_eq!(g!().focused_client, second);
            assert!(swipe_update(&wlr_pointer_swipe_update_event { fingers: 4, dx: -15.0, ..Default::default() }));
            assert_eq!(g!().focused_client, third, "fingers left jump to the window on the right");
            assert!(swipe_update(&wlr_pointer_swipe_update_event { fingers: 4, dx: 90.0, ..Default::default() }));
            assert_eq!(g!().focused_client, third, "at most one jump per swipe");
            swipe_end(false);
            pan(200.0, 100.0);
            let before_hold = g!().canvas.position;
            let mut hold = wlr_pointer_hold_begin_event { fingers: 4, ..Default::default() };
            hold_begin(null_mut(), (&mut hold as *mut wlr_pointer_hold_begin_event).cast());
            let mut held = wlr_pointer_hold_end_event { cancelled: true, ..Default::default() };
            hold_end(null_mut(), (&mut held as *mut wlr_pointer_hold_end_event).cast());
            assert_eq!(g!().canvas.position, before_hold);
            hold_begin(null_mut(), (&mut hold as *mut wlr_pointer_hold_begin_event).cast());
            held.cancelled = false;
            hold_end(null_mut(), (&mut held as *mut wlr_pointer_hold_end_event).cast());
            pump();
            assert_ne!(g!().canvas.position, before_hold);
            pan(before.x - g!().canvas.position.x, before.y - g!().canvas.position.y);
            focusclient(second, true);
            g!().locked = true;
            pan(1000.0, 2000.0);
            cycle(1);
            assert_eq!(g!().canvas.position, before);
            assert_eq!(g!().focused_client, second);
            g!().locked = false;
            setfullscreen(second, true);
            pan(500.0, 500.0);
            assert_eq!(g!().canvas.position, before);
            assert_eq!(origin(second), ((*monitor).m.x, (*monitor).m.y));
            setfullscreen(second, false);
            center(second);
            pump();
            assert_eq!(xytonode(middle.0 as f64, middle.1 as f64).c, second);

            let socket = std::os::unix::net::UnixDatagram::unbound().unwrap();
            socket
                .send_to(b"focus-right", crate::desktop::runtime_path("sock"))
                .unwrap();
            pump();
            assert_eq!(g!().focused_client, third);
            let status = std::fs::read_to_string(crate::desktop::runtime_path("status")).unwrap();
            assert!(status.starts_with("canvas "));
            assert!(status.contains("window iwc-three "));
            socket
                .send_to(b"view 2", crate::desktop::runtime_path("sock"))
                .unwrap();
            pump();
            assert!(g!().focused_client.is_null());
            socket
                .send_to(b"view 1", crate::desktop::runtime_path("sock"))
                .unwrap();
            pump();
            assert_eq!(g!().focused_client, third);

            children.0.push(
                std::process::Command::new("wtype")
                    .args(["-M", "logo", "-k", "Left", "-m", "logo"])
                    .spawn()
                    .unwrap(),
            );
            pump();
            assert_eq!(g!().focused_client, second);
            focus_direction(1, 0);
            pump();
            assert_eq!(g!().focused_client, third);

            for target in [0.5, 2.0] {
                zoom_to(
                    target,
                    Point {
                        x: middle.0 as f64,
                        y: middle.1 as f64,
                    },
                );
                center(third);
                pump();
                wlr_cursor_warp(g!().cursor, null_mut(), middle.0 as f64, middle.1 as f64);
                let geometry = (*third).geom;
                moveresize(CursorMode::Move);
                motionnotify(1200, null_mut(), 400.0, 300.0, 400.0, 300.0);
                assert_eq!((*third).geom.x - geometry.x, (400.0 / target) as i32);
                assert_eq!((*third).geom.y - geometry.y, (300.0 / target) as i32);
                g!().grabc = null_mut();
                g!().cursor_mode = CursorMode::Normal;
                resize(third, geometry, true);
            }
            let geometry_before_edges = (*third).geom;
            resize(third, wlr_box(geometry_before_edges.x, geometry_before_edges.y, 320, 240), true);
            pump();
            for target in [0.5, 1.0, 2.0] {
                zoom_to(target, Point { x: middle.0 as f64, y: middle.1 as f64 });
                center(third);
                pump();
                let geometry = (*third).geom;
                let viewport = (*g!().selmon).w;
                let top_left = world(third, viewport.x as f64, viewport.y as f64);
                let bottom_right = world(third, (viewport.x + viewport.width) as f64,
                    (viewport.y + viewport.height) as f64);
                for offset in [-8, 8] {
                    for (expected_x, expected_y) in [
                        (top_left.x.round() as i32 + offset, geometry.y),
                        (bottom_right.x.round() as i32 - geometry.width + offset, geometry.y),
                        (geometry.x, top_left.y.round() as i32 + offset),
                        (geometry.x, bottom_right.y.round() as i32 - geometry.height + offset),
                    ] {
                        resize(third, geometry, true);
                        let pointer = screen(third, (geometry.x + geometry.width / 2) as f64,
                            (geometry.y + geometry.height / 2) as f64);
                        wlr_cursor_warp(g!().cursor, null_mut(), pointer.x, pointer.y);
                        moveresize(CursorMode::Move);
                        assert_eq!(g!().grabc, third);
                        let destination = screen(third, (expected_x + g!().grabcx) as f64,
                            (expected_y + g!().grabcy) as f64);
                        let dx = destination.x - (*g!().cursor).x;
                        let dy = destination.y - (*g!().cursor).y;
                        motionnotify(1210, null_mut(), dx, dy, dx, dy);
                        assert_eq!(((*third).geom.x, (*third).geom.y), (expected_x, expected_y),
                            "dragging near or past any screen edge must not snap at scale {target}");
                        let pointer = screen(third, ((*third).geom.x + g!().grabcx) as f64,
                            ((*third).geom.y + g!().grabcy) as f64);
                        assert_eq!(((*g!().cursor).x, (*g!().cursor).y), (pointer.x, pointer.y));
                        g!().grabc = null_mut();
                        g!().cursor_mode = CursorMode::Normal;
                    }
                }
                resize(third, geometry, true);
            }
            resize(third, geometry_before_edges, true);
            zoom_to(
                0.5,
                Point {
                    x: middle.0 as f64,
                    y: middle.1 as f64,
                },
            );
            center(third);
            pump();
            wlr_cursor_warp(g!().cursor, null_mut(), middle.0 as f64, middle.1 as f64);
            let geometry = (*third).geom;
            moveresize(CursorMode::Resize);
            motionnotify(1220, null_mut(), 20.0, 10.0, 20.0, 10.0);
            assert_eq!((*third).geom.width, geometry.width + 40);
            assert_eq!((*third).geom.height, geometry.height + 20);
            let corner = screen(third, ((*third).geom.x + (*third).geom.width) as f64,
                ((*third).geom.y + (*third).geom.height) as f64);
            assert_eq!(((*g!().cursor).x, (*g!().cursor).y), (corner.x, corner.y));
            motionnotify(1221, null_mut(), -5000.0, -5000.0, -5000.0, -5000.0);
            let corner = screen(third, ((*third).geom.x + (*third).geom.width) as f64,
                ((*third).geom.y + (*third).geom.height) as f64);
            assert_eq!(((*g!().cursor).x, (*g!().cursor).y), (corner.x, corner.y));
            g!().grabc = null_mut();
            g!().cursor_mode = CursorMode::Normal;
            resize(third, geometry, true);
            zoom_to(
                1.0,
                Point {
                    x: middle.0 as f64,
                    y: middle.1 as f64,
                },
            );
            center(third);
            pump();
            let origin_before = origin(third);
            wlr_cursor_warp(g!().cursor, null_mut(), middle.0 as f64, middle.1 as f64);
            moveresize(CursorMode::Move);
            motionnotify(1234, null_mut(), 40.0, 30.0, 40.0, 30.0);
            assert_eq!(origin(third), (origin_before.0 + 40, origin_before.1 + 30));
            g!().grabc = null_mut();
            g!().cursor_mode = CursorMode::Normal;
            let keyboard = std::ptr::addr_of_mut!((*(*g!().kb_group).wlr_group).keyboard);
            wlr_seat_set_keyboard(g!().seat, keyboard);
            let modifiers = (*keyboard).modifiers;
            (*keyboard).modifiers.depressed = 1 << (*keyboard).mod_indexes[3];
            assert_eq!(keyboard_mods(), WLR_MODIFIER_ALT);
            let before_scroll = g!().canvas.position;
            assert!(axis(&wlr_pointer_axis_event {
                source: WL_POINTER_AXIS_SOURCE_FINGER,
                orientation: WL_POINTER_AXIS_HORIZONTAL_SCROLL,
                delta: 10.0,
                ..Default::default()
            }));
            assert_eq!(g!().canvas.position.x, before_scroll.x - 25.0 / g!().canvas.scale);
            assert_eq!(g!().canvas.position.y, before_scroll.y);
            assert!(panning());
            assert!((*g!().seat).pointer_state.focused_surface.is_null());
            assert!(axis(&wlr_pointer_axis_event { source: WL_POINTER_AXIS_SOURCE_FINGER,
                orientation: WL_POINTER_AXIS_HORIZONTAL_SCROLL, ..Default::default() }));
            assert!(!panning());
            assert!(axis(&wlr_pointer_axis_event { source: WL_POINTER_AXIS_SOURCE_FINGER,
                orientation: WL_POINTER_AXIS_HORIZONTAL_SCROLL, delta: 1.0, ..Default::default() }));
            (*keyboard).modifiers = modifiers;
            crate::desktop::modifiers_changed();
            assert!(!panning(), "releasing Alt restores the cursor even without axis_stop");
            (*keyboard).modifiers.depressed = 1 << (*keyboard).mod_indexes[3];
            pan(before_scroll.x - g!().canvas.position.x, 0.0);
            let button_event = wlr_pointer_button_event {
                button: BTN_LEFT,
                state: WL_POINTER_BUTTON_STATE_PRESSED,
                ..Default::default()
            };
            assert!(button(&button_event));
            assert!(panning());
            let pointer = ((*g!().cursor).x, (*g!().cursor).y);
            let before = g!().canvas.position;
            motionnotify(1250, null_mut(), 80.0, -25.0, 80.0, -25.0);
            assert!((*g!().seat).pointer_state.focused_surface.is_null());
            children.0.push(std::process::Command::new("grim").args(["-c", "/tmp/iwc-pan-cursor.png"]).spawn().unwrap());
            pump();
            assert_eq!(((*g!().cursor).x, (*g!().cursor).y), pointer);
            assert_eq!(
                g!().canvas.position,
                Point {
                    x: before.x - 80.0,
                    y: before.y + 25.0
                }
            );
            assert!(button(&wlr_pointer_button_event {
                state: WL_POINTER_BUTTON_STATE_RELEASED,
                ..button_event
            }));
            assert!(!panning());
            let before = g!().canvas.position;
            assert!(axis(&wlr_pointer_axis_event {
                orientation: WL_POINTER_AXIS_HORIZONTAL_SCROLL,
                delta: 20.0,
                ..Default::default()
            }));
            assert_eq!(g!().canvas.position.x, before.x + 60.0);
            (*keyboard).modifiers = modifiers;
            #[cfg(feature = "xwayland")]
            {
                children.0.push(
                    std::process::Command::new("xeyes")
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn()
                        .unwrap(),
                );
                for _ in 0..20 {
                    pump();
                    if g!().clients.iter().any(|client| client_is_x11(*client)) {
                        break;
                    }
                }
                let client = *g!()
                    .clients
                    .iter()
                    .find(|client| client_is_x11(**client))
                    .unwrap();
                assert_eq!(g!().focused_client, client);
                center(client);
                pump();
                assert_eq!(xytonode(middle.0 as f64, middle.1 as f64).c, client);
                zoom_to(
                    0.5,
                    Point {
                        x: middle.0 as f64,
                        y: middle.1 as f64,
                    },
                );
                pump();
                assert_eq!(xytonode(middle.0 as f64, middle.1 as f64).c, client);
                zoom_to(
                    1.0,
                    Point {
                        x: middle.0 as f64,
                        y: middle.1 as f64,
                    },
                );
                let geometry = (*client).geom;
                pan(-2000.0, 1500.0);
                assert_eq!(
                    ((*client).geom.x, (*client).geom.y),
                    (geometry.x, geometry.y)
                );
                center(client);
                pump();
                assert_eq!(xytonode(middle.0 as f64, middle.1 as f64).c, client);
            }
            let floating: Vec<_> = windows.iter().map(|client| (**client).geom).collect();
            zoom_to(0.5, Point { x: 600.0, y: 320.0 });
            pan(3000.0, -2000.0);
            let camera = (g!().canvas.position, g!().canvas.scale);
            g!().settings.niri = true;
            setlayout(Some(0));
            pump();
            assert_eq!(g!().canvas.position, Point::default());
            assert_eq!(g!().canvas.scale, 1.0);
            pan(2000.0, 2000.0);
            zoom_to(2.0, Point::default());
            assert_eq!(g!().canvas.position, Point::default());
            assert_eq!(g!().canvas.scale, 1.0);
            for client in &windows {
                assert!(!(**client).isfloating);
                assert_eq!((**client).geom.width, ((viewport.width - 16) as f64 * 0.5).round() as i32 - 16);
                assert_eq!((**client).geom.height, viewport.height - 32);
            }
            focusclient(first, true);
            pump();
            assert_eq!((*first).geom.x, viewport.x + 16);
            focus_direction(1, 0);
            assert_eq!(g!().focused_client, second);
            focus_direction(1, 0);
            assert_eq!(g!().focused_client, third);
            pump();
            assert!((*third).geom.x >= viewport.x + 16);
            assert!((*third).geom.x + (*third).geom.width <= viewport.x + viewport.width - 16);
            let point = ((*third).geom.x + 50, (*third).geom.y + 50);
            assert_eq!(xytonode(point.0 as f64, point.1 as f64).c, third);
            let keyboard = wlr_seat_get_keyboard(g!().seat);
            let saved_modifiers = (*keyboard).modifiers;
            (*keyboard).modifiers.depressed = 1 << (*keyboard).mod_indexes[3];
            let mut scroll = wlr_pointer_axis_event {
                source: WL_POINTER_AXIS_SOURCE_FINGER,
                orientation: WL_POINTER_AXIS_HORIZONTAL_SCROLL,
                delta: 1000.0,
                ..Default::default()
            };
            assert!(crate::desktop::touchpad_axis(&scroll));
            assert_eq!((*first).geom.x, viewport.x + 16);
            assert_eq!(g!().canvas.position, Point::default());
            scroll.delta = 0.0;
            assert!(crate::desktop::touchpad_axis(&scroll));
            assert_eq!(g!().focused_client, first);
            scroll.delta = -(((*first).geom.width + 16) * 2) as f64 / 4.0;
            assert!(crate::desktop::touchpad_axis(&scroll));
            crate::workspace::end_scroll();
            assert_eq!(g!().focused_client, third);
            scroll.orientation = WL_POINTER_AXIS_VERTICAL_SCROLL;
            scroll.delta = 50.0;
            assert!(crate::desktop::touchpad_axis(&scroll));
            assert_eq!((*monitor).tagset[(*monitor).seltags], 2);
            view(1);
            (*keyboard).modifiers = saved_modifiers;
            setlayout(Some(1));
            pump();
            assert_eq!((g!().canvas.position, g!().canvas.scale), camera);
            for (client, geometry) in windows.iter().zip(&floating) {
                let actual = (**client).geom;
                assert_eq!((actual.x, actual.y, actual.width, actual.height), (geometry.x, geometry.y, geometry.width, geometry.height));
            }
            setfullscreen(third, true);
            setlayout(Some(0));
            assert!((*third).isfullscreen);
            assert_eq!((*third).geom.width, (*monitor).m.width);
            setlayout(Some(1));
            setfullscreen(third, false);
            assert_eq!(((*third).geom.x, (*third).geom.width), (floating[2].x, floating[2].width));
            view(2);
            assert_eq!(crate::workspace::layout(monitor), 0);
            assert_eq!(g!().canvas.scale, 1.0);
            setlayout(Some(1));
            pan(-9000.0, 4000.0);
            view(1);
            assert_eq!((g!().canvas.position, g!().canvas.scale), camera);
            view(4);
            assert_eq!(crate::workspace::layout(monitor), 0);
            view(1);
            setlayout(Some(0));
            g!().settings.niri = false;
            arrange(monitor);
            pump();
            let master: Vec<_> = windows.iter().map(|client| (**client).geom).collect();
            assert_ne!(master[0].height, master[2].height, "master/stack, not columns");
            let toplevel = xdg_toplevel(first);
            let size = ((*toplevel).current.width, (*toplevel).current.height);
            wlr_xdg_toplevel_set_size(toplevel, size.0 + 80, size.1 + 40);
            client_set_size(first, size.0 as u32, size.1 as u32);
            assert_eq!(((*toplevel).scheduled.width, (*toplevel).scheduled.height), size,
                "returning to the acknowledged size must replace a stale scheduled configure");
            for _ in 0..5 {
                setlayout(Some(1));
                setlayout(Some(0));
                for (client, expected) in windows.iter().zip(&master) {
                    focusclient(*client, true);
                    wlr_cursor_warp(g!().cursor, null_mut(), (expected.x + 20) as f64, (expected.y + 20) as f64);
                    motionnotify(1500, null_mut(), 0.0, 0.0, 0.0, 0.0);
                    let actual = (**client).geom;
                    assert_eq!((actual.x, actual.y, actual.width, actual.height),
                        (expected.x, expected.y, expected.width, expected.height));
                    assert_eq!(((*(**client).scene).node.x, (*(**client).scene).node.y), (actual.x, actual.y));
                }
                pump();
                for client in &windows {
                    let toplevel = xdg_toplevel(*client);
                    assert_eq!(((*toplevel).current.width, (*toplevel).current.height),
                        ((**client).geom.width - 2 * (**client).bw, (**client).geom.height - 2 * (**client).bw));
                }
            }
            for client in &windows {
                let actual = (**client).geom;
                assert!(actual.x >= viewport.x && actual.y >= viewport.y);
                assert!(actual.x + actual.width <= viewport.x + viewport.width);
                assert!(actual.y + actual.height <= viewport.y + viewport.height);
            }
            setlayout(Some(1));
            focusclient(first, true);
            center(first);
            assert_eq!(g!().canvas.position, centered((*first).geom, (*monitor).w, g!().canvas.scale));
            pump();
            let first_camera = crate::workspace::camera(monitor);
            let point = screen(first, ((*first).geom.x + (*first).geom.width / 2) as f64,
                ((*first).geom.y + (*first).geom.height / 2) as f64);
            let other = *g!().mons.iter().find(|candidate| **candidate != monitor).unwrap();
            g!().selmon = other;
            focusclient(focustop(other), true);
            assert_eq!(crate::workspace::layout(other), 0);
            setlayout(Some(1));
            pan(-2000.0, 1500.0);
            zoom_to(2.0, Point { x: (*other).w.x as f64 + 100.0, y: 100.0 });
            assert_eq!(crate::workspace::camera(monitor), first_camera);
            assert_eq!(xytonode(point.x, point.y).c, first);
            printstatus();
            let status = std::fs::read_to_string(crate::desktop::runtime_path("status")).unwrap();
            let name = cstr((*(*monitor).wlr_output).name);
            assert!(status.contains(&format!("{name} zoom {:.3}", first_camera.1)));
            let name = cstr((*(*other).wlr_output).name);
            assert!(status.contains(&format!("{name} zoom 2.000")));
            setlayout(Some(0));
            setmon(second, other, 1);
            focusclient(second, true);
            pump();
            let geometry = (*second).geom;
            assert!(geometry.x >= (*other).w.x);
            assert_eq!(xytonode((geometry.x + 50) as f64, (geometry.y + 50) as f64).c, second);
            assert_eq!(crate::workspace::camera(monitor), first_camera);
            crate::canvas_scene::sync();
            for output in g!().mons.clone() { wlr_scene_output_commit((*output).scene_output, null()); }
            crate::canvas_scene::sync();
            for output in g!().mons.clone() {
                assert!(!wlr_scene_output_needs_frame((*output).scene_output), "unchanged projection must not keep damaging the output");
            }
            drop(children);
            pump();
            assert!(g!().clients.is_empty());
            cleanup();
            let _ = std::fs::remove_dir_all(directory);
        }
    }
}
