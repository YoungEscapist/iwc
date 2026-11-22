//! The `#ifdef XWAYLAND` part of dwl.c.

use crate::prelude::*;

/// xcb_icccm.h: XCB_ICCCM_WM_HINT_X_URGENCY
const WM_HINT_X_URGENCY: u32 = 1 << 8;

pub unsafe fn setup_xwayland() {
    /* Initialise the XWayland X server.
     * It will be started when the first X client is started. */
    let xwayland = wlr_xwayland_create(g!().dpy, g!().compositor, true);
    g!().xwayland = xwayland;
    if xwayland.is_null() {
        eprintln!("failed to setup XWayland X server, continuing without it");
        return;
    }
    let l = Listener::new(&mut (*xwayland).events.ready, null_mut::<c_void>(), xwaylandready);
    g!().listeners.push(l);
    let l = Listener::new(&mut (*xwayland).events.new_surface, null_mut::<c_void>(), createnotifyx11);
    g!().listeners.push(l);

    std::env::set_var("DISPLAY", &*cstr((*xwayland).display_name));
}

pub unsafe fn activatex11(c: *mut Client, _: *mut c_void) {
    /* Only "managed" windows can be activated */
    if !client_is_unmanaged(c) {
        wlr_xwayland_surface_activate((*c).xwayland, true);
    }
}

pub unsafe fn associatex11(c: *mut Client, _: *mut c_void) {
    let surface = client_surface(c);
    (*c).map = Listener::new(&mut (*surface).events.map, c, mapnotify);
    (*c).unmap = Listener::new(&mut (*surface).events.unmap, c, unmapnotify);
}

pub unsafe fn configurex11(c: *mut Client, data: *mut c_void) {
    let event = data as *mut wlr_xwayland_surface_configure_event;
    if !client_mapped(c) || client_is_unmanaged(c) || (*c).mon.is_null() {
        wlr_xwayland_surface_configure(
            (*c).xwayland,
            (*event).x,
            (*event).y,
            (*event).width,
            (*event).height,
        );
        return;
    }
    let m = (*c).mon;
    if ((*c).isfloating && c != g!().grabc) || crate::workspace::floating(m) {
        let bw = (*c).bw;
        resize(
            c,
            wlr_box(
                (*event).x as i32 - bw,
                (*event).y as i32 - bw,
                (*event).width as i32 + bw * 2,
                (*event).height as i32 + bw * 2,
            ),
            false,
        );
    } else {
        arrange(m);
    }
}

pub unsafe fn createnotifyx11(_: *mut c_void, data: *mut c_void) {
    let xsurface = data as *mut wlr_xwayland_surface;

    /* Allocate a Client for this surface */
    let c = Box::into_raw(Box::new(Client::new(X11)));
    (*xsurface).data = c.cast();
    (*c).xwayland = xsurface;
    (*c).bw = if client_is_unmanaged(c) { 0 } else { BORDERPX };

    /* Listen to the various events it can emit */
    let ev = &mut (*xsurface).events;
    (*c).associate = Listener::new(&mut ev.associate, c, associatex11);
    (*c).destroy = Listener::new(&mut ev.destroy, c, destroynotify);
    (*c).dissociate = Listener::new(&mut ev.dissociate, c, dissociatex11);
    (*c).activate = Listener::new(&mut ev.request_activate, c, activatex11);
    (*c).configure = Listener::new(&mut ev.request_configure, c, configurex11);
    (*c).fullscreen = Listener::new(&mut ev.request_fullscreen, c, fullscreennotify);
    (*c).set_geometry = Listener::new(&mut ev.set_geometry, c, setgeometryx11);
    (*c).set_hints = Listener::new(&mut ev.set_hints, c, sethints);
    (*c).set_title = Listener::new(&mut ev.set_title, c, updatetitle);
}

pub unsafe fn dissociatex11(c: *mut Client, _: *mut c_void) {
    (*c).map.remove();
    (*c).unmap.remove();
}

pub unsafe fn setgeometryx11(c: *mut Client, _: *mut c_void) {
    if !client_is_unmanaged(c) || !client_mapped(c) || (*c).scene.is_null() {
        return;
    }
    (*c).geom = client_get_geometry(c);
    wlr_scene_node_set_position(&mut (*(*c).scene).node, (*c).geom.x, (*c).geom.y);
}

pub unsafe fn sethints(c: *mut Client, _: *mut c_void) {
    let hints = (*(*c).xwayland).hints;
    if c == focustop(g!().selmon) || hints.is_null() {
        return;
    }

    (*c).isurgent = ((*hints).flags as u32 & WM_HINT_X_URGENCY) != 0;
    printstatus();

    if (*c).isurgent && client_mapped(c) {
        client_set_border_color(c, &URGENTCOLOR);
    }
}

pub unsafe fn xwaylandready(_: *mut c_void, _: *mut c_void) {
    let xwayland = g!().xwayland;
    /* assign the one and only seat */
    wlr_xwayland_set_seat(xwayland, g!().seat);

    /* Set the default XWayland cursor to match the rest of dwl. */
    let xcursor = wlr_xcursor_manager_get_xcursor(g!().cursor_mgr, c"default".as_ptr(), 1.0);
    if !xcursor.is_null() {
        let image = *(*xcursor).images;
        wlr_xwayland_set_cursor(
            xwayland,
            wlr_xcursor_image_get_buffer(image),
            (*image).hotspot_x as i32,
            (*image).hotspot_y as i32,
        );
    }
}
