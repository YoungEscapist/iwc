use crate::infinity::Point;
use crate::prelude::*;

#[derive(Clone, Copy)]
pub struct Workspace {
    pub layout: usize,
    position: Point,
    scale: f64,
    scroll: i32,
    scrolling: bool,
    swipe_y: f64,
}

impl Default for Workspace {
    fn default() -> Self {
        Self {
            layout: 0,
            position: Point::default(),
            scale: 1.0,
            scroll: 0,
            scrolling: false,
            swipe_y: 0.0,
        }
    }
}

pub unsafe fn index(monitor: *mut Monitor) -> usize {
    ((*monitor).tagset[(*monitor).seltags].trailing_zeros() as usize).min(TAGCOUNT as usize - 1)
}

pub unsafe fn layout(monitor: *mut Monitor) -> usize {
    if monitor.is_null() {
        0
    } else {
        (*monitor).workspaces[index(monitor)].layout
    }
}

pub unsafe fn floating(monitor: *mut Monitor) -> bool {
    !monitor.is_null() && layout(monitor) == 1
}

unsafe fn save() {
    let (monitor, tag) = g!().canvas.workspace;
    if !monitor.is_null() && (*monitor).workspaces[tag].layout == 1 {
        (*monitor).workspaces[tag].position = g!().canvas.position;
        (*monitor).workspaces[tag].scale = g!().canvas.scale;
    }
}

unsafe fn load(monitor: *mut Monitor) {
    crate::desktop::cancel_gestures();
    crate::infinity::cancel_gestures();
    let (previous, tag) = g!().canvas.workspace;
    if !previous.is_null() {
        (*previous).workspaces[tag].scrolling = false;
        (*previous).workspaces[tag].swipe_y = 0.0;
    }
    g!().canvas.workspace = (monitor, if monitor.is_null() { 0 } else { index(monitor) });
    let state = if monitor.is_null() {
        Workspace::default()
    } else {
        (*monitor).workspaces[index(monitor)]
    };
    g!().canvas.position = if state.layout == 1 {
        state.position
    } else {
        Point::default()
    };
    g!().canvas.scale = if state.layout == 1 { state.scale } else { 1.0 };
}

pub unsafe fn activate(monitor: *mut Monitor) {
    let tag = if monitor.is_null() { 0 } else { index(monitor) };
    if g!().canvas.workspace != (monitor, tag) {
        save();
        load(monitor);
    }
}

pub unsafe fn forget(monitor: *mut Monitor) {
    if g!().canvas.workspace.0 == monitor {
        load(null_mut());
    }
}

pub unsafe fn camera(monitor: *mut Monitor) -> (Point, f64) {
    if monitor.is_null() || g!().canvas.workspace == (monitor, index(monitor)) {
        (g!().canvas.position, g!().canvas.scale)
    } else {
        let state = (*monitor).workspaces[index(monitor)];
        if state.layout == 1 {
            (state.position, state.scale)
        } else {
            (Point::default(), 1.0)
        }
    }
}

pub unsafe fn set_layout(target: Option<usize>) {
    let monitor = g!().selmon;
    if monitor.is_null() || g!().locked || g!().cursor_mode != CursorMode::Normal
        || !g!().active_constraint.is_null() || !(*g!().seat).drag.is_null() {
        return;
    }
    crate::desktop::cancel_gestures();
    activate(monitor);
    let previous = layout(monitor);
    let target = target.unwrap_or(previous ^ 1);
    if target >= LAYOUTS.len() || target == previous {
        return;
    }
    save();
    let windows: Vec<_> = g!()
        .clients
        .clone()
        .into_iter()
        .filter(|client| visibleon(*client, monitor))
        .collect();
    for &client in &windows {
        if previous == 1 {
            (*client).floating_geom = if (*client).isfullscreen {
                (*client).prev
            } else {
                (*client).geom
            };
        }
    }
    (*monitor).workspaces[index(monitor)].layout = target;
    load(monitor);
    for (index, &client) in windows.iter().enumerate() {
        if target == 0 && !(*client).float_pinned {
            (*client).isfloating = false;
        }
        if target == 1 {
            if (*client).floating_geom.width == 0 {
                let viewport = (*monitor).w;
                let scale = g!().canvas.scale;
                let position = g!().canvas.position;
                let angle = std::f64::consts::TAU * index as f64 / windows.len() as f64;
                let mut geometry = if (*client).isfullscreen { (*client).prev } else { (*client).geom };
                let fit = (viewport.width as f64 * 0.7 / scale / geometry.width.max(1) as f64)
                    .min(viewport.height as f64 * 0.7 / scale / geometry.height.max(1) as f64).min(1.0);
                geometry.width = (geometry.width as f64 * fit).round() as i32;
                geometry.height = (geometry.height as f64 * fit).round() as i32;
                let radius = if windows.len() > 1 { viewport.width.min(viewport.height) as f64 * 0.15 / scale } else { 0.0 };
                geometry.x = (position.x + (viewport.x as f64 + viewport.width as f64 / 2.0) / scale + angle.cos() * radius - geometry.width as f64 / 2.0).round() as i32;
                geometry.y = (position.y + (viewport.y as f64 + viewport.height as f64 / 2.0) / scale + angle.sin() * radius - geometry.height as f64 / 2.0).round() as i32;
                (*client).floating_geom = geometry;
            }
            if (*client).isfullscreen {
                (*client).prev = (*client).floating_geom;
            } else {
                resize(client, (*client).floating_geom, false);
            }
        }
    }
    arrange(monitor);
    crate::canvas_scene::sync();
    printstatus();
}

pub unsafe fn tile(monitor: *mut Monitor) {
    if !g!().settings.niri {
        crate::dwl::tile(monitor);
        return;
    }
    let windows = tiled_windows(monitor);
    let viewport = (*monitor).w;
    let gap = g!()
        .settings
        .gaps
        .min((viewport.width / 4).max(0))
        .min((viewport.height / 4).max(0));
    let width = (((viewport.width - gap) as f64 * g!().settings.column_width).round() as i32 - gap)
        .max(2 * BORDERPX + 1);
    let focused = focustop(monitor);
    let mut scroll = (*monitor).workspaces[index(monitor)].scroll;
    if let Some(column) = windows.iter().position(|client| *client == focused)
        .filter(|_| !(*monitor).workspaces[index(monitor)].scrolling) {
        let left = column as i32 * (width + gap);
        let available = (viewport.width - 2 * gap).max(width);
        scroll = if g!().settings.center_focused {
            left - (available - width) / 2
        } else {
            scroll.clamp(left + width - available, left)
        };
    }
    (*monitor).workspaces[index(monitor)].scroll = scroll;
    for (column, client) in windows.into_iter().enumerate() {
        resize(
            client,
            wlr_box(
                viewport.x + gap + column as i32 * (width + gap) - scroll,
                viewport.y + gap,
                width,
                (viewport.height - 2 * gap).max(2 * BORDERPX + 1),
            ),
            false,
        );
    }
}

unsafe fn tiled_windows(monitor: *mut Monitor) -> Vec<*mut Client> {
    g!()
        .clients
        .iter()
        .rev()
        .copied()
        .filter(|client| {
            visibleon(*client, monitor) && !(*(*client)).isfloating && !(*(*client)).isfullscreen
        })
        .collect()
}

pub unsafe fn scroll(event: &wlr_pointer_axis_event) -> bool {
    let monitor = g!().selmon;
    if monitor.is_null() || floating(monitor) || !g!().settings.niri
        || cleanmask(keyboard_mods()) != WLR_MODIFIER_ALT || !crate::infinity::allowed() {
        return false;
    }
    if event.delta == 0.0 { end_scroll(); return true; }
    if !event.delta.is_finite() { return true; }
    let windows = tiled_windows(monitor);
    let state = &mut (*monitor).workspaces[index(monitor)];
    state.scrolling = true;
    if event.orientation == WL_POINTER_AXIS_HORIZONTAL_SCROLL {
        let maximum = windows.first().map_or(0, |client| ((**client).geom.width + g!().settings.gaps) * (windows.len() as i32 - 1));
        state.scroll = (state.scroll as f64 - event.delta * 4.0).round().clamp(0.0, maximum as f64) as i32;
        arrange(monitor);
        crate::canvas_scene::sync();
        printstatus();
    } else {
        state.swipe_y += event.delta * 2.5;
        if state.swipe_y.abs() >= 120.0 {
            let direction = if state.swipe_y > 0.0 { 1 } else { -1 };
            state.swipe_y = 0.0;
            crate::desktop::step_tag(direction);
        }
    }
    true
}

pub unsafe fn end_scroll() {
    let monitor = g!().selmon;
    if monitor.is_null() { return; }
    let state = &mut (*monitor).workspaces[index(monitor)];
    if !std::mem::take(&mut state.scrolling) { return; }
    state.swipe_y = 0.0;
    let target = tiled_windows(monitor).into_iter().min_by_key(|client| {
        ((*(*client)).geom.x - (*monitor).w.x - g!().settings.gaps).abs()
    });
    if let Some(client) = target {
        (*monitor).workspaces[index(monitor)].scroll += (*client).geom.x - (*monitor).w.x - g!().settings.gaps;
        focusclient(client, true);
    }
    arrange(monitor);
    printstatus();
}

pub unsafe fn reveal(client: *mut Client) {
    if !client.is_null()
        && !(*client).mon.is_null()
        && !floating((*client).mon)
        && g!().settings.niri
    {
        arrange((*client).mon);
    }
}
