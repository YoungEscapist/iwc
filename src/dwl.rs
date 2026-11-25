//! Port of dwl.c.  Function names follow dwl so the two can be read side by
//! side.

use crate::prelude::*;
use std::fmt::Write as _;
use std::os::unix::process::CommandExt;

pub const TAGMASK: u32 = (1u32 << TAGCOUNT) - 1;

#[inline]
pub const fn cleanmask(mask: u32) -> u32 {
    mask & !WLR_MODIFIER_CAPS
}

#[inline]
pub unsafe fn visibleon(c: *mut Client, m: *mut Monitor) -> bool {
    !m.is_null() && (*c).mon == m && ((*c).tags & (*m).tagset[(*m).seltags]) != 0
}

#[inline]
unsafe fn node(tree: *mut wlr_scene_tree) -> *mut wlr_scene_node {
    &mut (*tree).node
}

#[inline]
unsafe fn rect_node(rect: *mut wlr_scene_rect) -> *mut wlr_scene_node {
    &mut (*rect).node
}

unsafe fn seat_keyboard() -> *mut wlr_keyboard {
    wlr_seat_get_keyboard(g!().seat)
}

pub(crate) unsafe fn keyboard_mods() -> u32 {
    let kb = seat_keyboard();
    if kb.is_null() {
        0
    } else {
        wlr_keyboard_get_modifiers(kb)
    }
}

unsafe fn listen_global(signal: *mut wl_signal, f: unsafe fn(*mut c_void, *mut c_void)) {
    let l = Listener::new(signal, null_mut::<c_void>(), f);
    g!().listeners.push(l);
}

pub unsafe fn motionnotify0() {
    motionnotify(0, null_mut(), 0.0, 0.0, 0.0, 0.0);
}

unsafe fn layer_link(l: *mut LayerSurface, m: *mut Monitor, layer: usize) {
    layer_unlink(l);
    if m.is_null() {
        return;
    }
    (*m).layers[layer].insert(0, l);
    (*l).list_mon = m;
    (*l).list_layer = layer;
}

unsafe fn layer_unlink(l: *mut LayerSurface) {
    let m = (*l).list_mon;
    if !m.is_null() {
        remove_ptr(&mut (*m).layers[(*l).list_layer], l);
        (*l).list_mon = null_mut();
    }
}

/* ---------------------------------------------------------------------- */
/* actions (dwl's `void (*func)(const Arg *)`)                            */

pub unsafe fn run_action(a: Action) {
    if !g!().locked { crate::workspace::activate(g!().selmon); }
    match a {
        Action::Spawn(argv) => spawn(argv),
        Action::FocusStack(i) => crate::infinity::cycle(i),
        Action::FocusDirection(horizontal, vertical) => crate::infinity::focus_direction(horizontal, vertical),
        Action::Pan(horizontal, vertical) => crate::infinity::step(horizontal, vertical),
        Action::CanvasZoom(step) => crate::infinity::zoom_by(step),
        Action::CenterWindow => crate::infinity::center(g!().focused_client),
        Action::IncNMaster(i) => incnmaster(i),
        Action::SetMFact(f) => setmfact(f),
        Action::Zoom => zoom(),
        Action::View(ui) => view(ui),
        Action::StepTag(step) => crate::desktop::step_tag(step),
        Action::ToggleView(ui) => toggleview(ui),
        Action::Tag(ui) => { tag(ui); crate::infinity::center(focustop(g!().selmon)); },
        Action::ToggleTag(ui) => { toggletag(ui); crate::infinity::center(focustop(g!().selmon)); },
        Action::KillClient => killclient(),
        Action::SetLayout(lt) => setlayout(lt),
        Action::CycleLayout => setlayout(None),
        Action::ReloadConfig => crate::lua_config::reload(),
        Action::ToggleFloating => togglefloating(),
        Action::ToggleFullscreen => togglefullscreen(),
        Action::FocusMon(dir) => { focusmon(dir); crate::infinity::center(focustop(g!().selmon)); },
        Action::TagMon(dir) => tagmon(dir),
        Action::MoveResize(mode) => moveresize(mode),
        Action::Chvt(n) => chvt(n),
        Action::Quit => quit(),
    }
}

/* ---------------------------------------------------------------------- */

pub unsafe fn applybounds(c: *mut Client, bbox: &wlr_box) {
    let bw = (*c).bw;
    /* set minimum client size to 1 */
    (*c).geom.width = (*c).geom.width.max(1 + 2 * bw);
    (*c).geom.height = (*c).geom.height.max(1 + 2 * bw);

    if !(*c).isfullscreen && ((*c).isfloating || crate::workspace::floating((*c).mon)) {
        let (max, min) = client_get_size_hints(c);
        (*c).geom.width = (*c).geom.width.max(min.width + 2 * bw);
        (*c).geom.height = (*c).geom.height.max(min.height + 2 * bw);
        /* Some clients set their max size to INT_MAX, which does not violate
         * the protocol but it's unnecessary, as they can set their max size
         * to zero. */
        if max.width > 0 && 2 * bw <= i32::MAX - max.width {
            (*c).geom.width = (*c).geom.width.min(max.width + 2 * bw);
        }
        if max.height > 0 && 2 * bw <= i32::MAX - max.height {
            (*c).geom.height = (*c).geom.height.min(max.height + 2 * bw);
        }
    }

    if !(*c).isfullscreen {
        return;
    }
    let gm = &mut (*c).geom;
    if gm.x >= bbox.x + bbox.width {
        gm.x = bbox.x + bbox.width - gm.width;
    }
    if gm.y >= bbox.y + bbox.height {
        gm.y = bbox.y + bbox.height - gm.height;
    }
    if gm.x + gm.width <= bbox.x {
        gm.x = bbox.x;
    }
    if gm.y + gm.height <= bbox.y {
        gm.y = bbox.y;
    }
}

pub unsafe fn applyrules(c: *mut Client) {
    /* rule matching */
    let appid = client_get_appid(c);
    let title = client_get_title(c);
    let mut newtags = 0u32;
    let mut mon = g!().selmon;

    for r in RULES {
        if r.title.map_or(true, |t| title.contains(t)) && r.id.map_or(true, |id| appid.contains(id))
        {
            (*c).isfloating = r.isfloating;
            (*c).float_pinned = r.isfloating;
            newtags |= r.tags;
            for (i, &m) in g!().mons.iter().enumerate() {
                if r.monitor == i as i32 {
                    mon = m;
                }
            }
        }
    }

    setmon(c, mon, newtags);
}

pub unsafe fn arrange(m: *mut Monitor) {
    if !(*(*m).wlr_output).enabled {
        return;
    }

    for c in g!().clients.clone() {
        if (*c).mon == m {
            let visible = visibleon(c, m);
            wlr_scene_node_set_enabled(node((*c).scene), visible);
            let src = (*c).image_capture_source;
            client_set_suspended(
                c,
                !visible && (src.is_null() || wl_list_is_empty(&(*src).resources)),
            );
        }
    }

    let top = focustop(m);
    wlr_scene_node_set_enabled(
        rect_node((*m).fullscreen_bg),
        !top.is_null() && (*top).isfullscreen,
    );

    let lt = &LAYOUTS[crate::workspace::layout(m)];
    (*m).ltsymbol = if lt.arrange.is_some() && g!().settings.niri { "|||" } else { lt.symbol }.to_string();

    /* We move all clients (except fullscreen and unmanaged) to LyrTile while
     * in floating layout to avoid "real" floating clients be always on top */
    for c in g!().clients.clone() {
        if (*c).mon != m || (*(*c).scene).node.parent == g!().layers[LYR_FS] {
            continue;
        }
        let parent = if lt.arrange.is_none() || !(*c).isfloating {
            g!().layers[LYR_TILE]
        } else {
            g!().layers[LYR_FLOAT]
        };
        wlr_scene_node_reparent(node((*c).scene), parent);
        client_set_tiled(c, if lt.arrange.is_some() && !(*c).isfloating {
            WLR_EDGE_TOP | WLR_EDGE_BOTTOM | WLR_EDGE_LEFT | WLR_EDGE_RIGHT
        } else { WLR_EDGE_NONE });
    }

    if let Some(f) = lt.arrange {
        f(m);
    }
    motionnotify0();
    checkidleinhibitor(null_mut());
}

pub unsafe fn arrangelayer(m: *mut Monitor, layer: usize, usable_area: &mut wlr_box, exclusive: bool) {
    let full_area = (*m).m;
    for l in (*m).layers[layer].clone() {
        let layer_surface = (*l).layer_surface;
        if !(*layer_surface).initialized {
            continue;
        }
        if exclusive != ((*layer_surface).current.exclusive_zone > 0) {
            continue;
        }
        wlr_scene_layer_surface_v1_configure((*l).scene_layer, &full_area, usable_area);
        wlr_scene_node_set_position(
            node((*l).popups),
            (*(*l).scene).node.x,
            (*(*l).scene).node.y,
        );
    }
}

pub unsafe fn arrangelayers(m: *mut Monitor) {
    let mut usable_area = (*m).m;
    if !(*(*m).wlr_output).enabled {
        return;
    }

    /* Arrange exclusive surfaces from top->bottom */
    for i in (0..4).rev() {
        arrangelayer(m, i, &mut usable_area, true);
    }

    if !wlr_box_equal(&usable_area, &(*m).w) {
        (*m).w = usable_area;
        arrange(m);
    }

    /* Arrange non-exclusive surfaces from top->bottom */
    for i in (0..4).rev() {
        arrangelayer(m, i, &mut usable_area, false);
    }

    /* Find topmost keyboard interactive layer that has indicated it wants
     * exclusive access to the keyboard, if such a layer exists */
    for layer in [ZWLR_LAYER_SHELL_V1_LAYER_OVERLAY, ZWLR_LAYER_SHELL_V1_LAYER_TOP] {
        for &l in (*m).layers[layer as usize].clone().iter().rev() {
            if g!().locked
                || (*(*l).layer_surface).current.keyboard_interactive
                    != ZWLR_LAYER_SURFACE_V1_KEYBOARD_INTERACTIVITY_EXCLUSIVE
                || !(*l).mapped
            {
                continue;
            }
            /* Deactivate the focused client. */
            focusclient(null_mut(), false);
            g!().exclusive_focus = Exclusive::Layer(l);
            client_notify_enter((*(*l).layer_surface).surface, seat_keyboard());
            return;
        }
    }
}

pub unsafe fn axisnotify(_: *mut c_void, data: *mut c_void) {
    /* This event is forwarded by the cursor when a pointer emits an axis
     * event, for example when you move the scroll wheel. */
    let event = data as *mut wlr_pointer_axis_event;
    wlr_idle_notifier_v1_notify_activity(g!().idle_notifier, g!().seat);
    if crate::infinity::axis(&*event) {
        return;
    }
    if crate::desktop::touchpad_axis(&*event) {
        return;
    }

    let adir = if (*event).orientation == WL_POINTER_AXIS_VERTICAL_SCROLL {
        if (*event).delta > 0.0 { AxisDir::Down } else { AxisDir::Up }
    } else if (*event).delta > 0.0 {
        AxisDir::Right
    } else {
        AxisDir::Left
    };
    let mods = keyboard_mods();
    for a in AXES {
        if cleanmask(mods) == cleanmask(a.modifiers) && adir == a.dir {
            run_action(a.action);
            return;
        }
    }

    /* Notify the client with pointer focus of the axis event. */
    wlr_seat_pointer_notify_axis(
        g!().seat,
        (*event).time_msec,
        (*event).orientation,
        (*event).delta,
        (*event).delta_discrete,
        (*event).source,
        (*event).relative_direction,
    );
}

pub unsafe fn buttonpress(_: *mut c_void, data: *mut c_void) {
    let event = data as *mut wlr_pointer_button_event;
    let cursor = g!().cursor;
    wlr_idle_notifier_v1_notify_activity(g!().idle_notifier, g!().seat);
    if crate::infinity::button(&*event) {
        return;
    }

    match (*event).state {
        WL_POINTER_BUTTON_STATE_PRESSED => {
            g!().cursor_mode = CursorMode::Pressed;
            g!().selmon = xytomon((*cursor).x, (*cursor).y);
            if !g!().locked {
                /* Change focus if the button was _pressed_ over a client
                 * or a layer surface with on-demand keyboard interactivity */
                let hit = xytonode((*cursor).x, (*cursor).y);
                let (c, l) = (hit.c, hit.l);
                if !c.is_null() && (!client_is_unmanaged(c) || client_wants_focus(c)) {
                    focusclient(c, true);
                } else if !l.is_null() && (*(*l).layer_surface).current.keyboard_interactive != 0 {
                    focusclient(null_mut(), false);
                    client_notify_enter((*(*l).layer_surface).surface, seat_keyboard());
                }

                let mods = keyboard_mods();
                for b in BUTTONS {
                    if cleanmask(mods) == cleanmask(b.modifiers) && (*event).button == b.button {
                        run_action(b.action);
                        return;
                    }
                }
            }
        }
        WL_POINTER_BUTTON_STATE_RELEASED => {
            /* If you released any buttons, we exit interactive move/resize mode. */
            let mode = g!().cursor_mode;
            if !g!().locked && mode != CursorMode::Normal && mode != CursorMode::Pressed {
                g!().cursor_mode = CursorMode::Normal;
                /* Update the cursor */
                wlr_seat_pointer_clear_focus(g!().seat);
                motionnotify0();
                /* Drop the window off on its new monitor */
                g!().selmon = xytomon((*cursor).x, (*cursor).y);
                let grabc = g!().grabc;
                if !grabc.is_null() {
                    setmon(grabc, g!().selmon, 0);
                }
                g!().grabc = null_mut();
                return;
            }
            g!().cursor_mode = CursorMode::Normal;
        }
        _ => {}
    }
    /* If the event wasn't handled by the compositor, notify the client with
     * pointer focus that a button press has occurred */
    wlr_seat_pointer_notify_button(g!().seat, (*event).time_msec, (*event).button, (*event).state);
}

pub unsafe fn capturerequest(_: *mut c_void, data: *mut c_void) {
    let request = data as *mut wlr_ext_foreign_toplevel_image_capture_source_manager_v1_request;
    let view = (*(*request).toplevel_handle).data as *mut Client;
    if view.is_null() {
        return;
    }

    if (*view).image_capture_source.is_null() {
        (*view).image_capture_source = wlr_ext_image_capture_source_v1_create_with_scene_node(
            &mut (*(*view).image_capture_scene).tree.node,
            g!().event_loop,
            g!().alloc,
            g!().drw,
        );
        if (*view).image_capture_source.is_null() {
            return;
        }
    }

    wlr_ext_foreign_toplevel_image_capture_source_manager_v1_request_accept(
        request,
        (*view).image_capture_source,
    );
    client_set_suspended(view, false);
}

pub unsafe fn chvt(n: u32) {
    if !g!().session.is_null() {
        wlr_session_change_vt(g!().session, n);
    }
}

pub unsafe fn checkidleinhibitor(exclude: *mut wlr_surface) {
    let mut inhibited = false;
    let (mut unused_lx, mut unused_ly) = (0, 0);
    for inhibitor in
        wl_list_items!(&mut (*g!().idle_inhibit_mgr).inhibitors, wlr_idle_inhibitor_v1, link)
    {
        let surface = wlr_surface_get_root_surface((*inhibitor).surface);
        let tree = (*surface).data as *mut wlr_scene_tree;
        if exclude != surface
            && (BYPASS_SURFACE_VISIBILITY
                || tree.is_null()
                || wlr_scene_node_coords(node(tree), &mut unused_lx, &mut unused_ly))
        {
            inhibited = true;
            break;
        }
    }
    wlr_idle_notifier_v1_set_inhibited(g!().idle_notifier, inhibited);
}

pub unsafe fn cleanup() {
    g!().desktop.take();
    g!().listeners.clear(); // cleanuplisteners()
    g!().gpu_reset.remove();
    #[cfg(feature = "xwayland")]
    {
        if !g!().xwayland.is_null() {
            wlr_xwayland_destroy(g!().xwayland);
        }
        g!().xwayland = null_mut();
    }
    wl_display_destroy_clients(g!().dpy);
    if g!().child_pid > 0 {
        libc::kill(-g!().child_pid, libc::SIGTERM);
        libc::waitpid(g!().child_pid, null_mut(), 0);
    }
    wlr_xcursor_manager_destroy(g!().cursor_mgr);

    destroykeyboardgroup(g!().kb_group, null_mut());

    input_method_relay_finish(g!().input_method_relay);
    g!().input_method_relay = null_mut();

    /* If it's not destroyed manually, it will cause a use-after-free of
     * wlr_seat.  Destroy it until it's fixed on the wlroots side */
    wlr_backend_destroy(g!().backend);

    wl_display_destroy(g!().dpy);
    /* Destroy after the wayland display (when the monitors are already
     * destroyed) to avoid destroying them with an invalid scene output. */
    crate::canvas_scene::finish();
    wlr_scene_node_destroy(&mut (*g!().scene).tree.node);
}

pub unsafe fn cleanupmon(m: *mut Monitor, _: *mut c_void) {
    crate::workspace::forget(m);
    for i in 0..4 {
        for l in (*m).layers[i].clone() {
            wlr_layer_surface_v1_destroy((*l).layer_surface);
        }
    }

    (*m).destroy.remove();
    (*m).frame.remove();
    remove_ptr(&mut g!().mons, m);
    (*m).request_state.remove();
    if !(*m).lock_surface.is_null() {
        destroylocksurface(m, null_mut());
    }
    (*(*m).wlr_output).data = null_mut();
    wlr_output_layout_remove(g!().output_layout, (*m).wlr_output);
    wlr_scene_output_destroy((*m).scene_output);

    closemon(m);
    wlr_scene_node_destroy(rect_node((*m).fullscreen_bg));
    drop(Box::from_raw(m));
}

pub unsafe fn closemon(m: *mut Monitor) {
    /* update selmon if needed and
     * move closed monitor's clients to the focused one */
    if g!().mons.is_empty() {
        g!().selmon = null_mut();
    } else if m == g!().selmon {
        /* don't switch to disabled mons */
        let first = (&g!().mons)[0];
        g!().selmon = if (*(*first).wlr_output).enabled { first } else { null_mut() };
    }

    for c in g!().clients.clone() {
        if (*c).mon == m {
            setmon(c, g!().selmon, (*c).tags);
        }
    }
    focusclient(focustop(g!().selmon), true);
    printstatus();
}

pub unsafe fn commitlayersurfacenotify(l: *mut LayerSurface, _: *mut c_void) {
    let layer_surface = (*l).layer_surface;
    let scene_layer = g!().layers[LAYERMAP[(*layer_surface).current.layer as usize]];

    if (*layer_surface).initial_commit {
        if (*l).mon.is_null() {
            return;
        }
        client_set_scale((*layer_surface).surface, (*(*(*l).mon).wlr_output).scale);

        /* Temporarily set the layer's current state to pending
         * so that we can easily arrange it */
        let old_state = (*layer_surface).current;
        (*layer_surface).current = (*layer_surface).pending;
        arrangelayers((*l).mon);
        (*layer_surface).current = old_state;
        return;
    }

    if g!().exclusive_focus == Exclusive::Layer(l)
        && (*layer_surface).current.keyboard_interactive
            != ZWLR_LAYER_SURFACE_V1_KEYBOARD_INTERACTIVITY_EXCLUSIVE
    {
        g!().exclusive_focus = Exclusive::None;
    }

    if (*layer_surface).current.committed == 0 && (*l).mapped == (*(*layer_surface).surface).mapped {
        return;
    }
    (*l).mapped = (*(*layer_surface).surface).mapped;

    if scene_layer != (*(*l).scene).node.parent {
        wlr_scene_node_reparent(node((*l).scene), scene_layer);
        layer_link(l, (*l).mon, (*layer_surface).current.layer as usize);
        wlr_scene_node_reparent(
            node((*l).popups),
            if (*layer_surface).current.layer < ZWLR_LAYER_SHELL_V1_LAYER_TOP {
                g!().layers[LYR_TOP]
            } else {
                scene_layer
            },
        );
    }

    if !(*l).mon.is_null() {
        arrangelayers((*l).mon);
    }
}

pub unsafe fn commitnotify(c: *mut Client, _: *mut c_void) {
    if (*(*c).xdg).initial_commit {
        /*
         * Get the monitor this client will be rendered on
         * Note that if the user set a rule in which the client is placed on
         * a different monitor based on its title, this will likely select
         * a wrong monitor.
         */
        applyrules(c);
        if !(*c).mon.is_null() {
            client_set_scale(client_surface(c), (*(*(*c).mon).wlr_output).scale);
        }
        setmon(c, null_mut(), 0); /* Make sure to reapply rules in mapnotify() */

        wlr_xdg_toplevel_set_wm_capabilities(
            xdg_toplevel(c),
            WLR_XDG_TOPLEVEL_WM_CAPABILITIES_FULLSCREEN,
        );
        if !(*c).decoration.is_null() {
            requestdecorationmode(c, (*c).decoration.cast());
        }
        wlr_xdg_toplevel_set_size(xdg_toplevel(c), 0, 0);
        if !(*c).isfloating {
            client_set_tiled(c, WLR_EDGE_TOP | WLR_EDGE_BOTTOM | WLR_EDGE_LEFT | WLR_EDGE_RIGHT);
        }
        return;
    }

    resize(c, (*c).geom, (*c).isfloating && !(*c).isfullscreen);
}

pub unsafe fn commitpopup(p: *mut Popup, data: *mut c_void) {
    let surface = data as *mut wlr_surface;
    let popup = wlr_xdg_popup_try_from_wlr_surface(surface);
    if popup.is_null() || !(*(*popup).base).initial_commit {
        return;
    }

    let (ty, c, l) = toplevel_from_wlr_surface((*(*popup).base).surface);
    if (*popup).parent.is_null() || ty < 0 {
        return;
    }
    let parent_tree = (*(*popup).parent).data as *mut wlr_scene_tree;
    if parent_tree.is_null() {
        return;
    }
    (*(*(*popup).base).surface).data =
        wlr_scene_xdg_surface_create(parent_tree, (*popup).base).cast();
    if (!l.is_null() && (*l).mon.is_null()) || (!c.is_null() && (*c).mon.is_null()) {
        // Our destroy listener frees `p` synchronously.
        wlr_xdg_popup_destroy(popup);
        return;
    }
    let mut bx = if ty == LAYER_SHELL { (*(*l).mon).m } else { (*(*c).mon).w };
    let origin = if ty == LAYER_SHELL {
        ((*(*l).scene).node.x, (*(*l).scene).node.y)
    } else {
        crate::infinity::origin(c)
    };
    let scale = if ty == LAYER_SHELL { 1.0 } else { crate::infinity::scale(c) };
    bx.x = ((bx.x - origin.0) as f64 / scale).round() as i32;
    bx.y = ((bx.y - origin.1) as f64 / scale).round() as i32;
    bx.width = (bx.width as f64 / scale).round() as i32;
    bx.height = (bx.height as f64 / scale).round() as i32;
    wlr_xdg_popup_unconstrain_from_box(popup, &bx);
    (*p).commit.remove();
}

pub unsafe fn destroypopup(p: *mut Popup, _: *mut c_void) {
    drop(Box::from_raw(p));
}

pub unsafe fn createdecoration(_: *mut c_void, data: *mut c_void) {
    let deco = data as *mut wlr_xdg_toplevel_decoration_v1;
    let c = (*(*(*deco).toplevel).base).data as *mut Client;
    (*c).decoration = deco;

    (*c).set_decoration_mode = Listener::new(&mut (*deco).events.request_mode, c, requestdecorationmode);
    (*c).destroy_decoration = Listener::new(&mut (*deco).events.destroy, c, destroydecoration);

    requestdecorationmode(c, deco.cast());
}

pub unsafe fn createidleinhibitor(_: *mut c_void, data: *mut c_void) {
    let idle_inhibitor = data as *mut wlr_idle_inhibitor_v1;
    listen_static(&mut (*idle_inhibitor).events.destroy, destroyidleinhibitor);
    checkidleinhibitor(null_mut());
}

pub unsafe fn createkeyboard(keyboard: *mut wlr_keyboard) {
    let group = (*g!().kb_group).wlr_group;
    /* Set the keymap to match the group keymap */
    wlr_keyboard_set_keymap(keyboard, (*group).keyboard.keymap);
    /* Add the new keyboard to the group */
    wlr_keyboard_group_add_keyboard(group, keyboard);
}

pub unsafe fn createkeyboardgroup() -> *mut KeyboardGroup {
    let group = Box::into_raw(Box::new(KeyboardGroup {
        wlr_group: wlr_keyboard_group_create(),
        virtual_keyboard: null_mut(),
        keysyms: [0; 2],
        mods: 0,
        key_repeat_source: null_mut(),
        modifiers: Listener::none(),
        key: Listener::none(),
        destroy: Listener::none(),
    }));
    (*(*group).wlr_group).data = group.cast();
    let kb: *mut wlr_keyboard = &mut (*(*group).wlr_group).keyboard;

    /* Prepare an XKB keymap and assign it to the keyboard group. */
    let opt = |s: Option<&str>| s.map(|s| CString::new(s).unwrap());
    let (rules, model, layout, variant, options) = (
        opt(XKB_RULES.rules),
        opt(XKB_RULES.model),
        opt(XKB_RULES.layout),
        opt(XKB_RULES.variant),
        opt(XKB_RULES.options),
    );
    let ptr = |s: &Option<CString>| s.as_ref().map_or(null(), |s| s.as_ptr());
    let names = xkb_rule_names {
        rules: ptr(&rules),
        model: ptr(&model),
        layout: ptr(&layout),
        variant: ptr(&variant),
        options: ptr(&options),
    };
    let context = xkb_context_new(XKB_CONTEXT_NO_FLAGS);
    let keymap = xkb_keymap_new_from_names(context, &names, XKB_KEYMAP_COMPILE_NO_FLAGS);
    if keymap.is_null() {
        die("failed to compile keymap");
    }

    wlr_keyboard_set_keymap(kb, keymap);
    xkb_keymap_unref(keymap);
    xkb_context_unref(context);

    wlr_keyboard_set_repeat_info(kb, REPEAT_RATE, REPEAT_DELAY);

    /* Set up listeners for keyboard events */
    (*group).key = Listener::new(&mut (*kb).events.key, group, keypress);
    (*group).modifiers = Listener::new(&mut (*kb).events.modifiers, group, keypressmod);

    (*group).key_repeat_source = wl_event_loop_add_timer(g!().event_loop, Some(keyrepeat), group.cast());

    /* A seat can only have one keyboard, but this is a limitation of the
     * Wayland protocol - not wlroots. We assign all connected keyboards to the
     * same wlr_keyboard_group, which provides a single wlr_keyboard interface
     * for all of them. Set this combined wlr_keyboard as the seat keyboard. */
    wlr_seat_set_keyboard(g!().seat, kb);
    group
}

pub unsafe fn createlayersurface(_: *mut c_void, data: *mut c_void) {
    let layer_surface = data as *mut wlr_layer_surface_v1;
    let surface = (*layer_surface).surface;
    let scene_layer = g!().layers[LAYERMAP[(*layer_surface).pending.layer as usize]];

    if (*layer_surface).output.is_null() {
        if g!().selmon.is_null() {
            wlr_layer_surface_v1_destroy(layer_surface);
            return;
        }
        (*layer_surface).output = (*g!().selmon).wlr_output;
    }

    let l = Box::into_raw(Box::new(LayerSurface {
        mon: (*(*layer_surface).output).data.cast(),
        scene: null_mut(),
        popups: null_mut(),
        scene_layer: null_mut(),
        list_mon: null_mut(),
        list_layer: 0,
        mapped: false,
        layer_surface,
        destroy: Listener::none(),
        unmap: Listener::none(),
        surface_commit: Listener::none(),
    }));
    (*layer_surface).data = l.cast();
    (*l).surface_commit = Listener::new(&mut (*surface).events.commit, l, commitlayersurfacenotify);
    (*l).unmap = Listener::new(&mut (*surface).events.unmap, l, unmaplayersurfacenotify);
    (*l).destroy = Listener::new(&mut (*layer_surface).events.destroy, l, destroylayersurfacenotify);

    (*l).scene_layer = wlr_scene_layer_surface_v1_create(scene_layer, layer_surface);
    (*l).scene = (*(*l).scene_layer).tree;
    (*l).popups = wlr_scene_tree_create(
        if (*layer_surface).current.layer < ZWLR_LAYER_SHELL_V1_LAYER_TOP {
            g!().layers[LYR_TOP]
        } else {
            scene_layer
        },
    );
    (*surface).data = (*l).popups.cast();
    (*(*l).scene).node.data = l.cast();
    (*(*l).popups).node.data = l.cast();

    layer_link(l, (*l).mon, (*layer_surface).pending.layer as usize);
    wlr_surface_send_enter(surface, (*layer_surface).output);
}

pub unsafe fn createlocksurface(lock: *mut SessionLock, data: *mut c_void) {
    let lock_surface = data as *mut wlr_session_lock_surface_v1;
    let m = (*(*lock_surface).output).data as *mut Monitor;
    let scene_tree = wlr_scene_subsurface_tree_create((*lock).scene, (*lock_surface).surface);
    (*(*lock_surface).surface).data = scene_tree.cast();
    if m.is_null() {
        return;
    }
    (*m).lock_surface = lock_surface;

    wlr_scene_node_set_position(node(scene_tree), (*m).m.x, (*m).m.y);
    wlr_session_lock_surface_v1_configure(lock_surface, (*m).m.width as u32, (*m).m.height as u32);

    (*m).destroy_lock_surface =
        Listener::new(&mut (*lock_surface).events.destroy, m, destroylocksurface);

    if m == g!().selmon {
        client_notify_enter((*lock_surface).surface, seat_keyboard());
    }
}

pub unsafe fn createmon(_: *mut c_void, data: *mut c_void) {
    /* This event is raised by the backend when a new output (aka a display or
     * monitor) becomes available. */
    let wlr_output = data as *mut wlr_output;

    if !wlr_output_init_render(wlr_output, g!().alloc, g!().drw) {
        return;
    }

    let m = Box::into_raw(Box::new(Monitor {
        wlr_output,
        scene_output: null_mut(),
        fullscreen_bg: null_mut(),
        frame: Listener::none(),
        destroy: Listener::none(),
        request_state: Listener::none(),
        destroy_lock_surface: Listener::none(),
        lock_surface: null_mut(),
        m: wlr_box::default(),
        w: wlr_box::default(),
        layers: Default::default(),
        workspaces: [Default::default(); TAGCOUNT as usize],
        seltags: 0,
        tagset: [1, 1],
        mfact: 0.0,
        gamma_lut_changed: false,
        nmaster: 0,
        ltsymbol: String::new(),
        asleep: false,
    }));
    (*wlr_output).data = m.cast();

    let mut state: wlr_output_state = std::mem::zeroed();
    wlr_output_state_init(&mut state);
    /* Initialize monitor state using configured rules */
    let name = cstr((*wlr_output).name).into_owned();
    for r in MONRULES {
        if r.name.map_or(true, |n| name.contains(n)) {
            (*m).m.x = r.x;
            (*m).m.y = r.y;
            (*m).mfact = r.mfact;
            (*m).nmaster = r.nmaster;
            (*m).ltsymbol = LAYOUTS[0].symbol.to_string();
            wlr_output_state_set_scale(&mut state, r.scale);
            wlr_output_state_set_transform(&mut state, r.rr);
            break;
        }
    }

    /* The mode is a tuple of (width, height, refresh rate), and each monitor
     * supports only a specific set of modes. We just pick the monitor's
     * preferred mode; a more sophisticated compositor would let the user
     * configure it. */
    let mode = wlr_output_preferred_mode(wlr_output);
    if !mode.is_null() {
        wlr_output_state_set_mode(&mut state, mode);
    }

    /* Set up event listeners */
    (*m).frame = Listener::new(&mut (*wlr_output).events.frame, m, rendermon);
    (*m).destroy = Listener::new(&mut (*wlr_output).events.destroy, m, cleanupmon);
    (*m).request_state = Listener::new(&mut (*wlr_output).events.request_state, m, requestmonstate);

    wlr_output_state_set_enabled(&mut state, true);
    wlr_output_commit_state(wlr_output, &state);
    wlr_output_state_finish(&mut state);

    g!().mons.insert(0, m);
    printstatus();

    /* The xdg-protocol specifies:
     *
     * If the fullscreened surface is not opaque, the compositor must make
     * sure that other screen content not part of the same surface tree (made
     * up of subsurfaces, popups or similarly coupled surfaces) are not
     * visible below the fullscreened surface.
     */
    /* updatemons() will resize and set correct position */
    (*m).fullscreen_bg = wlr_scene_rect_create(g!().layers[LYR_FS], 0, 0, FULLSCREEN_BG.as_ptr());
    wlr_scene_node_set_enabled(rect_node((*m).fullscreen_bg), false);

    /* Adds this to the output layout in the order it was configured.
     *
     * The output layout utility automatically adds a wl_output global to the
     * display, which Wayland clients can see to find out information about
     * the output (such as DPI, scale factor, manufacturer, etc). */
    (*m).scene_output = wlr_scene_output_create(g!().scene, wlr_output);
    if (*m).m.x == -1 && (*m).m.y == -1 {
        wlr_output_layout_add_auto(g!().output_layout, wlr_output);
    } else {
        wlr_output_layout_add(g!().output_layout, wlr_output, (*m).m.x, (*m).m.y);
    }
}

pub unsafe fn createnotify(_: *mut c_void, data: *mut c_void) {
    /* This event is raised when a client creates a new toplevel (application
     * window). */
    let toplevel = data as *mut wlr_xdg_toplevel;
    let base = (*toplevel).base;

    /* Allocate a Client for this surface */
    let c = Box::into_raw(Box::new(Client::new(XDG_SHELL)));
    (*base).data = c.cast();
    (*c).xdg = base;
    (*c).bw = BORDERPX;

    let surface = (*base).surface;
    (*c).commit = Listener::new(&mut (*surface).events.commit, c, commitnotify);
    (*c).map = Listener::new(&mut (*surface).events.map, c, mapnotify);
    (*c).unmap = Listener::new(&mut (*surface).events.unmap, c, unmapnotify);
    (*c).destroy = Listener::new(&mut (*toplevel).events.destroy, c, destroynotify);
    (*c).fullscreen = Listener::new(&mut (*toplevel).events.request_fullscreen, c, fullscreennotify);
    (*c).maximize = Listener::new(&mut (*toplevel).events.request_maximize, c, maximizenotify);
    (*c).set_title = Listener::new(&mut (*toplevel).events.set_title, c, updatetitle);
}

pub unsafe fn createpointer(pointer: *mut wlr_pointer) {
    let base: *mut wlr_input_device = &mut (*pointer).base;
    if wlr_input_device_is_libinput(base) {
        let device = wlr_libinput_get_device_handle(base);
        if !device.is_null() {
            let on = |b: bool| if b { 1 } else { 0 };
            if libinput_device_config_tap_get_finger_count(device) != 0 {
                libinput_device_config_tap_set_enabled(device, on(TAP_TO_CLICK));
                libinput_device_config_tap_set_drag_enabled(device, on(TAP_AND_DRAG));
                libinput_device_config_tap_set_drag_lock_enabled(device, on(DRAG_LOCK));
                libinput_device_config_tap_set_button_map(device, BUTTON_MAP);
            }

            if libinput_device_config_scroll_has_natural_scroll(device) != 0 {
                libinput_device_config_scroll_set_natural_scroll_enabled(
                    device,
                    NATURAL_SCROLLING as i32,
                );
            }

            if libinput_device_config_dwt_is_available(device) != 0 {
                libinput_device_config_dwt_set_enabled(device, on(DISABLE_WHILE_TYPING));
            }

            if libinput_device_config_left_handed_is_available(device) != 0 {
                libinput_device_config_left_handed_set(device, LEFT_HANDED as i32);
            }

            if libinput_device_config_middle_emulation_is_available(device) != 0 {
                libinput_device_config_middle_emulation_set_enabled(
                    device,
                    on(MIDDLE_BUTTON_EMULATION),
                );
            }

            if libinput_device_config_scroll_get_methods(device) != LIBINPUT_CONFIG_SCROLL_NO_SCROLL {
                libinput_device_config_scroll_set_method(device, SCROLL_METHOD);
            }

            if libinput_device_config_click_get_methods(device)
                != LIBINPUT_CONFIG_CLICK_METHOD_NONE
            {
                libinput_device_config_click_set_method(device, CLICK_METHOD);
            }

            if libinput_device_config_send_events_get_modes(device) != 0 {
                libinput_device_config_send_events_set_mode(device, SEND_EVENTS_MODE);
            }

            if libinput_device_config_accel_is_available(device) != 0 {
                libinput_device_config_accel_set_profile(device, ACCEL_PROFILE);
                libinput_device_config_accel_set_speed(device, ACCEL_SPEED);
            }
        }
    }

    wlr_cursor_attach_input_device(g!().cursor, base);
}

pub unsafe fn createpointerconstraint(_: *mut c_void, data: *mut c_void) {
    let pc = Box::into_raw(Box::new(PointerConstraint {
        constraint: data.cast(),
        destroy: Listener::none(),
    }));
    (*pc).destroy = Listener::new(&mut (*(*pc).constraint).events.destroy, pc, destroypointerconstraint);
}

pub unsafe fn createpopup(_: *mut c_void, data: *mut c_void) {
    /* This event is raised when a client (either xdg-shell or layer-shell)
     * creates a new popup. */
    let popup = data as *mut wlr_xdg_popup;
    let p = Box::into_raw(Box::new(Popup {
        commit: Listener::none(),
        destroy: Listener::none(),
    }));
    (*p).commit = Listener::new(&mut (*(*(*popup).base).surface).events.commit, p, commitpopup);
    (*p).destroy = Listener::new(&mut (*popup).events.destroy, p, destroypopup);
}

pub unsafe fn cursorconstrain(constraint: *mut wlr_pointer_constraint_v1) {
    if g!().active_constraint == constraint {
        return;
    }
    if !g!().active_constraint.is_null() {
        wlr_pointer_constraint_v1_send_deactivated(g!().active_constraint);
    }
    g!().active_constraint = constraint;
    wlr_pointer_constraint_v1_send_activated(constraint);
}

pub unsafe fn cursorframe(_: *mut c_void, _: *mut c_void) {
    /* This event is forwarded by the cursor when a pointer emits a frame
     * event. Frame events are sent after regular pointer events to group
     * multiple events together. For instance, two axis events may happen at
     * the same time, in which case a frame event won't be sent in between.
     * Notify the client with pointer focus of the frame event. */
    wlr_seat_pointer_notify_frame(g!().seat);
}

pub unsafe fn cursorwarptohint() {
    let ac = g!().active_constraint;
    let sx = (*ac).current.cursor_hint.x;
    let sy = (*ac).current.cursor_hint.y;

    let c = toplevel_from_wlr_surface((*ac).surface).1;
    if !c.is_null() && (*ac).current.cursor_hint.enabled {
        wlr_cursor_warp(
            g!().cursor,
            null_mut(),
            crate::infinity::origin(c).0 as f64 + (sx + (*c).bw as f64) * crate::infinity::scale(c),
            crate::infinity::origin(c).1 as f64 + (sy + (*c).bw as f64) * crate::infinity::scale(c),
        );
        wlr_seat_pointer_warp((*ac).seat, sx, sy);
    }
}

pub unsafe fn destroydecoration(c: *mut Client, _: *mut c_void) {
    (*c).decoration = null_mut();
    (*c).destroy_decoration.remove();
    (*c).set_decoration_mode.remove();
}

pub unsafe fn destroydragicon(_: *mut c_void) -> bool {
    /* Focus enter isn't sent during drag, so refocus the focused node. */
    focusclient(focustop(g!().selmon), true);
    motionnotify0();
    true
}

pub unsafe fn destroyidleinhibitor(data: *mut c_void) -> bool {
    /* `data` is the wlr_surface of the idle inhibitor being destroyed,
     * at this point the idle inhibitor is still in the list of the manager */
    checkidleinhibitor(wlr_surface_get_root_surface(data.cast()));
    true
}

pub unsafe fn destroylayersurfacenotify(l: *mut LayerSurface, _: *mut c_void) {
    if g!().exclusive_focus == Exclusive::Layer(l) {
        g!().exclusive_focus = Exclusive::None;
    }
    layer_unlink(l);
    (*l).destroy.remove();
    (*l).unmap.remove();
    (*l).surface_commit.remove();
    wlr_scene_node_destroy(node((*l).scene));
    wlr_scene_node_destroy(node((*l).popups));
    drop(Box::from_raw(l));
}

pub unsafe fn destroylock(lock: *mut SessionLock, unlock: bool) {
    wlr_seat_keyboard_notify_clear_focus(g!().seat);
    g!().locked = !unlock;
    if unlock {
        wlr_scene_node_set_enabled(rect_node(g!().locked_bg), false);
        focusclient(focustop(g!().selmon), false);
        motionnotify0();
    }

    wlr_scene_node_destroy(node((*lock).scene));
    g!().cur_lock = null_mut();
    drop(Box::from_raw(lock)); // disconnects new_surface, unlock and destroy
}

pub unsafe fn destroylocksurface(m: *mut Monitor, _: *mut c_void) {
    let lock_surface = (*m).lock_surface;
    (*m).lock_surface = null_mut();
    (*m).destroy_lock_surface.remove();

    if lock_surface.is_null()
        || (*lock_surface).surface != (*g!().seat).keyboard_state.focused_surface
    {
        return;
    }

    let cur_lock = g!().cur_lock;
    if g!().locked && !cur_lock.is_null() && !wl_list_is_empty(&(*cur_lock).surfaces) {
        let surface = container_of!((*cur_lock).surfaces.next, wlr_session_lock_surface_v1, link);
        client_notify_enter((*surface).surface, seat_keyboard());
    } else if !g!().locked {
        focusclient(focustop(g!().selmon), true);
    } else {
        wlr_seat_keyboard_clear_focus(g!().seat);
    }
}

pub unsafe fn destroynotify(c: *mut Client, _: *mut c_void) {
    /* Called when the xdg_toplevel is destroyed.  Dropping the Client
     * disconnects every listener it still has. */
    if g!().focused_client == c {
        g!().focused_client = null_mut();
    }
    if g!().grabc == c {
        g!().grabc = null_mut();
        g!().cursor_mode = CursorMode::Normal;
    }
    if g!().exclusive_focus == Exclusive::Client(c) {
        g!().exclusive_focus = Exclusive::None;
    }
    drop(Box::from_raw(c));
}

pub unsafe fn destroypointerconstraint(pc: *mut PointerConstraint, _: *mut c_void) {
    if g!().active_constraint == (*pc).constraint {
        cursorwarptohint();
        g!().active_constraint = null_mut();
    }
    drop(Box::from_raw(pc));
}

pub unsafe fn destroysessionlock(lock: *mut SessionLock, _: *mut c_void) {
    destroylock(lock, false);
}

pub unsafe fn destroykeyboardgroup(group: *mut KeyboardGroup, _: *mut c_void) {
    wl_event_source_remove((*group).key_repeat_source);
    (*group).key.remove();
    (*group).modifiers.remove();
    (*group).destroy.remove();
    wlr_keyboard_group_destroy((*group).wlr_group);
    drop(Box::from_raw(group));
}

pub unsafe fn dirtomon(dir: wlr_direction) -> *mut Monitor {
    let selmon = g!().selmon;
    let ol = g!().output_layout;
    if selmon.is_null() || wlr_output_layout_get(ol, (*selmon).wlr_output).is_null() {
        return selmon;
    }
    let (x, y) = ((*selmon).m.x as f64, (*selmon).m.y as f64);
    let next = wlr_output_layout_adjacent_output(ol, dir, (*selmon).wlr_output, x, y);
    if !next.is_null() {
        return (*next).data.cast();
    }
    let next = wlr_output_layout_farthest_output(
        ol,
        dir ^ (WLR_DIRECTION_LEFT | WLR_DIRECTION_RIGHT | WLR_DIRECTION_UP | WLR_DIRECTION_DOWN),
        (*selmon).wlr_output,
        x,
        y,
    );
    if !next.is_null() {
        return (*next).data.cast();
    }
    selmon
}

pub unsafe fn focusclient(c: *mut Client, lift: bool) {
    let seat = g!().seat;
    let old = (*seat).keyboard_state.focused_surface;
    let (mut unused_lx, mut unused_ly) = (0, 0);

    if g!().locked {
        return;
    }

    /* Raise client in stacking order if requested */
    if c.is_null() { crate::workspace::activate(g!().selmon); }
    if !c.is_null() && lift {
        wlr_scene_node_raise_to_top(node((*c).scene));
    }

    if !c.is_null() && client_surface(c) == old {
        return;
    }

    let (old_client_type, old_c, old_l) = toplevel_from_wlr_surface(old);
    if old_client_type == XDG_SHELL {
        for popup in wl_list_items!(&mut (*(*old_c).xdg).popups, wlr_xdg_popup, link) {
            wlr_xdg_popup_destroy(popup);
        }
    }

    if !old_c.is_null() && g!().exclusive_focus == Exclusive::Client(old_c) && client_wants_focus(old_c)
    {
        g!().exclusive_focus = Exclusive::None;
    }

    /* Put the new client atop the focus stack and select its monitor */
    if !c.is_null() && !client_is_unmanaged(c) {
        remove_ptr(&mut g!().fstack, c);
        g!().fstack.insert(0, c);
        g!().selmon = (*c).mon;
        crate::workspace::activate(g!().selmon);
        crate::workspace::reveal(c);
        (*c).isurgent = false;

        /* Don't change border color if there is an exclusive focus or we are
         * handling a drag operation */
        if g!().exclusive_focus == Exclusive::None && (*seat).drag.is_null() {
            client_set_border_color(c, &FOCUSCOLOR);
        }
    }

    /* If an overlay is focused, don't focus or activate the client,
     * but only update its position in fstack to render its border with
     * focuscolor and focus it after the overlay is closed. */
    if !old.is_null()
        && (c.is_null() || client_surface(c) != old)
        && old_client_type == LAYER_SHELL
        && wlr_scene_node_coords(node((*old_l).scene), &mut unused_lx, &mut unused_ly)
        && (*(*old_l).layer_surface).current.layer >= ZWLR_LAYER_SHELL_V1_LAYER_TOP
        && (*(*old_l).layer_surface).current.keyboard_interactive
            == ZWLR_LAYER_SURFACE_V1_KEYBOARD_INTERACTIVITY_EXCLUSIVE
    {
        return;
    }

    let focused = g!().focused_client;
    if !focused.is_null() && focused != c && !(!c.is_null() && client_is_unmanaged(c)) {
        let s = client_surface(focused);
        if !c.is_null() {
            client_set_border_color(focused, &BORDERCOLOR);
        }
        if !s.is_null() && (*s).mapped {
            client_activate_surface(s, false);
        }
        g!().focused_client = null_mut();
    }
    printstatus();

    if c.is_null() {
        /* With no client, all we have left is to clear focus */
        input_method_relay_set_focus(g!().input_method_relay, null_mut());
        wlr_seat_keyboard_notify_clear_focus(seat);
        return;
    }

    /* Change cursor surface */
    motionnotify0();

    input_method_relay_set_focus(g!().input_method_relay, client_surface(c));

    /* Have a client, so focus its top-level wlr_surface */
    client_notify_enter(client_surface(c), seat_keyboard());

    /* Activate the new client */
    client_activate_surface(client_surface(c), true);

    if !client_is_unmanaged(c) {
        g!().focused_client = c;
    }
}

pub unsafe fn focusmon(dir: wlr_direction) {
    let nmons = g!().mons.len();
    if nmons > 0 {
        /* don't switch to disabled mons */
        let mut i = 0;
        loop {
            g!().selmon = dirtomon(dir);
            let sm = g!().selmon;
            if sm.is_null() || (*(*sm).wlr_output).enabled || i >= nmons {
                break;
            }
            i += 1;
        }
    }
    focusclient(focustop(g!().selmon), true);
}

pub unsafe fn focusstack(dir: i32) {
    /* Focus the next or previous client (in tiling order) on selmon */
    let selmon = g!().selmon;
    let sel = focustop(selmon);
    if sel.is_null() || ((*sel).isfullscreen && !client_has_children(sel)) {
        return;
    }
    let clients = g!().clients.clone();
    let n = clients.len();
    let Some(start) = clients.iter().position(|&c| c == sel) else {
        return;
    };
    let mut c = sel;
    for k in 1..=n {
        let i = if dir > 0 { (start + k) % n } else { (start + n * k - k) % n };
        if visibleon(clients[i], selmon) {
            c = clients[i];
            break;
        }
    }
    /* If only one client is visible on selmon, then c == sel */
    focusclient(c, true);
}

/// We probably should change the name of this: it sounds like it will focus
/// the topmost client of this mon, when actually will only return that client.
pub unsafe fn focustop(m: *mut Monitor) -> *mut Client {
    for &c in g!().fstack.iter() {
        if visibleon(c, m) {
            return c;
        }
    }
    null_mut()
}

pub unsafe fn fullscreennotify(c: *mut Client, _: *mut c_void) {
    setfullscreen(c, client_wants_fullscreen(c));
}

pub unsafe fn gpureset(_: *mut c_void, _: *mut c_void) {
    let old_drw = g!().drw;
    let old_alloc = g!().alloc;

    g!().drw = wlr_renderer_autocreate(g!().backend);
    if g!().drw.is_null() {
        die("couldn't recreate renderer");
    }
    g!().alloc = wlr_allocator_autocreate(g!().backend, g!().drw);
    if g!().alloc.is_null() {
        die("couldn't recreate allocator");
    }

    // Reassigning drops (disconnects) the listener we are running in.
    g!().gpu_reset = Listener::new(&mut (*g!().drw).events.lost, null_mut::<c_void>(), gpureset);

    wlr_compositor_set_renderer(g!().compositor, g!().drw);

    for &m in g!().mons.clone().iter() {
        wlr_output_init_render((*m).wlr_output, g!().alloc, g!().drw);
    }

    wlr_allocator_destroy(old_alloc);
    wlr_renderer_destroy(old_drw);
}

extern "C" fn handlesig(signo: libc::c_int) {
    unsafe {
        if signo == libc::SIGCHLD {
            while libc::waitpid(-1, null_mut(), libc::WNOHANG) > 0 {}
        } else if signo == libc::SIGINT || signo == libc::SIGTERM {
            quit();
        }
    }
}

pub unsafe fn incnmaster(i: i32) {
    let selmon = g!().selmon;
    if selmon.is_null() {
        return;
    }
    (*selmon).nmaster = ((*selmon).nmaster + i).max(0);
    arrange(selmon);
}

pub unsafe fn inputdevice(_: *mut c_void, data: *mut c_void) {
    /* This event is raised by the backend when a new input device becomes
     * available. */
    let device = data as *mut wlr_input_device;
    match (*device).type_ {
        WLR_INPUT_DEVICE_KEYBOARD => createkeyboard(wlr_keyboard_from_input_device(device)),
        WLR_INPUT_DEVICE_POINTER => createpointer(wlr_pointer_from_input_device(device)),
        WLR_INPUT_DEVICE_TOUCH => crate::desktop::create_touch(device),
        _ => {}
    }

    crate::desktop::update_caps();
}

pub unsafe fn keybinding(mods: u32, sym: xkb_keysym_t) -> Option<Action> {
    /* Here we handle compositor keybindings. This is when the compositor is
     * processing keys, rather than passing them on to the client for its own
     * processing. */
    g!().settings.keys
        .iter()
        .find(|k| cleanmask(mods) == cleanmask(k.modifiers) && sym == k.keysym)
        .map(|k| k.action)
}

pub unsafe fn keypress(group: *mut KeyboardGroup, data: *mut c_void) {
    /* This event is raised when a key is pressed or released. */
    let event = data as *mut wlr_keyboard_key_event;

    /* Translate libinput keycode -> xkbcommon */
    let keycode = (*event).keycode + 8;
    let kb: *mut wlr_keyboard = &mut (*(*group).wlr_group).keyboard;
    let layout = xkb_state_key_get_layout((*kb).xkb_state, keycode);
    let mut handled = false;

    // Get the keysyms for level 0 (normal) and level 1 (shifted).  Only one
    // keysym for each level: multiple keysyms per keycode don't really exist
    // in the real world.
    for i in 0..2 {
        (*group).keysyms[i] = keymap_get_one_sym_by_level((*kb).keymap, keycode, layout, i as u32);
    }
    (*group).mods = wlr_keyboard_get_modifiers(kb);

    wlr_idle_notifier_v1_notify_activity(g!().idle_notifier, g!().seat);

    let idx = (*event).keycode as usize;
    let in_range = idx < g!().consumed.len();

    /* On _press_ if there is no active screen locker,
     * attempt to process a compositor keybinding. */
    if !g!().locked && (*event).state == WL_KEYBOARD_KEY_STATE_PRESSED {
        for i in 0..2 {
            if let Some(action) = keybinding((*group).mods, (*group).keysyms[i]) {
                if in_range {
                    (&mut g!().consumed)[idx] = true;
                }
                run_action(action);
                handled = true;
                break;
            }
        }
    }

    let delay = (*kb).repeat_info.delay;
    wl_event_source_timer_update(
        (*group).key_repeat_source,
        if handled && delay > 0 { delay } else { 0 },
    );

    if handled || input_method_keyboard_grab_forward_key(group, event) {
        return;
    }

    if in_range && (&g!().consumed)[idx] {
        if (*event).state == WL_KEYBOARD_KEY_STATE_RELEASED {
            (&mut g!().consumed)[idx] = false;
        }
        return; /* Don't pass to the client the release event of a handled key-press */
    }
    wlr_seat_set_keyboard(g!().seat, kb);
    /* Pass unhandled keycodes along to the client. */
    wlr_seat_keyboard_notify_key(g!().seat, (*event).time_msec, (*event).keycode, (*event).state);
}

pub unsafe fn keypressmod(group: *mut KeyboardGroup, _: *mut c_void) {
    /* This event is raised when a modifier key, such as shift or alt, is
     * pressed. We simply communicate this to the client. */
    if input_method_keyboard_grab_forward_modifiers(group) {
        return;
    }
    let kb: *mut wlr_keyboard = &mut (*(*group).wlr_group).keyboard;
    wlr_seat_set_keyboard(g!().seat, kb);
    crate::desktop::modifiers_changed();
    /* Send modifiers to the client. */
    wlr_seat_keyboard_notify_modifiers(g!().seat, &mut (*kb).modifiers);
}

unsafe extern "C" fn keyrepeat(data: *mut c_void) -> libc::c_int {
    let group = data as *mut KeyboardGroup;
    let rate = (*(*group).wlr_group).keyboard.repeat_info.rate;
    if rate <= 0 {
        return 0;
    }

    wl_event_source_timer_update((*group).key_repeat_source, 1000 / rate);

    for i in 0..2 {
        if let Some(action) = keybinding((*group).mods, (*group).keysyms[i]) {
            run_action(action);
            break;
        }
    }
    0
}

pub unsafe fn killclient() {
    let sel = focustop(g!().selmon);
    if !sel.is_null() {
        client_send_close(sel);
    }
}

pub unsafe fn locksession(_: *mut c_void, data: *mut c_void) {
    let session_lock = data as *mut wlr_session_lock_v1;
    wlr_scene_node_set_enabled(rect_node(g!().locked_bg), true);
    if !g!().cur_lock.is_null() {
        wlr_session_lock_v1_destroy(session_lock);
        return;
    }
    let lock = Box::into_raw(Box::new(SessionLock {
        scene: null_mut(),
        lock: session_lock,
        new_surface: Listener::none(),
        unlock: Listener::none(),
        destroy: Listener::none(),
    }));
    (*session_lock).data = lock.cast();
    focusclient(null_mut(), false);

    (*lock).scene = wlr_scene_tree_create(g!().layers[LYR_BLOCK]);
    g!().cur_lock = session_lock;
    g!().locked = true;
    crate::desktop::cancel_touches();

    (*lock).new_surface = Listener::new(&mut (*session_lock).events.new_surface, lock, createlocksurface);
    (*lock).destroy = Listener::new(&mut (*session_lock).events.destroy, lock, destroysessionlock);
    (*lock).unlock = Listener::new(&mut (*session_lock).events.unlock, lock, unlocksession);

    wlr_session_lock_v1_send_locked(session_lock);
}

pub unsafe fn mapnotify(c: *mut Client, _: *mut c_void) {
    /* Called when the surface is mapped, or ready to display on-screen. */
    let mut p: *mut Client = null_mut();
    let layers = g!().layers;

    let foreign_toplevel_state = wlr_ext_foreign_toplevel_handle_v1_state {
        app_id: client_get_appid_ptr(c),
        title: client_get_title_ptr(c),
    };

    /* Create scene tree for this client and its border */
    (*c).scene = wlr_scene_tree_create(layers[LYR_TILE]);
    (*client_surface(c)).data = (*c).scene.cast();
    /* Enabled later by a call to arrange() */
    wlr_scene_node_set_enabled(node((*c).scene), client_is_unmanaged(c));
    (*c).scene_surface = if (*c).kind == XDG_SHELL {
        wlr_scene_xdg_surface_create((*c).scene, (*c).xdg)
    } else {
        wlr_scene_subsurface_tree_create((*c).scene, client_surface(c))
    };
    (*(*c).scene).node.data = c.cast();
    (*(*c).scene_surface).node.data = c.cast();

    (*c).geom = client_get_geometry(c);

    /* Handle unmanaged clients first so we can return prior create borders */
    if client_is_unmanaged(c) {
        /* Unmanaged clients always are floating */
        wlr_scene_node_reparent(node((*c).scene), layers[LYR_UNMANAGED]);
        wlr_scene_node_set_position(node((*c).scene), (*c).geom.x, (*c).geom.y);
        if client_wants_focus(c) {
            focusclient(c, true);
            g!().exclusive_focus = Exclusive::Client(c);
        }
    } else {
        let color = if (*c).isurgent { &URGENTCOLOR } else { &BORDERCOLOR };
        for i in 0..4 {
            (*c).border[i] = wlr_scene_rect_create((*c).scene, 0, 0, color.as_ptr());
            (*(*c).border[i]).node.data = c.cast();
        }

        /* Initialize client geometry with room for border */
        (*c).geom.width += 2 * (*c).bw;
        (*c).geom.height += 2 * (*c).bw;

        /* Insert this client into client lists. */
        g!().clients.insert(0, c);
        g!().fstack.insert(0, c);

        (*c).foreign_toplevel_handle = wlr_ext_foreign_toplevel_handle_v1_create(
            g!().foreign_toplevel_list,
            &foreign_toplevel_state,
        );
        if !(*c).foreign_toplevel_handle.is_null() {
            (*(*c).foreign_toplevel_handle).data = c.cast();
        }
        (*c).image_capture_scene = wlr_scene_create();
        if (*c).kind == XDG_SHELL {
            (*c).image_capture_tree =
                wlr_scene_xdg_surface_create(&mut (*(*c).image_capture_scene).tree, (*c).xdg);
        } else {
            (*c).image_capture_scene_surface =
                wlr_scene_surface_create(&mut (*(*c).image_capture_scene).tree, client_surface(c));
        }

        /* Set initial monitor, tags, floating status, and focus:
         * we always consider floating, clients that have parent and thus
         * we set the same tags and monitor as its parent.
         * If there is no parent, apply rules */
        p = client_get_parent(c);
        if !p.is_null() {
            (*c).isfloating = true;
            (*c).float_pinned = true;
            setmon(c, (*p).mon, (*p).tags);
        } else {
            applyrules(c);
        }
        printstatus();
    }

    /* unset_fullscreen: */
    let m = if !(*c).mon.is_null() {
        (*c).mon
    } else {
        xytomon((*c).geom.x as f64, (*c).geom.y as f64)
    };
    for w in g!().clients.clone() {
        if w != c && w != p && (*w).isfullscreen && m == (*w).mon && ((*w).tags & (*c).tags) != 0 {
            setfullscreen(w, false);
        }
    }
    if !client_is_unmanaged(c) && p.is_null() {
        crate::infinity::center(c);
    }
    crate::canvas_scene::sync();
}

pub unsafe fn maximizenotify(c: *mut Client, _: *mut c_void) {
    /* This event is raised when a client would like to maximize itself,
     * typically because the user clicked on the maximize button on
     * client-side decorations. dwl doesn't support maximization, but
     * to conform to xdg-shell protocol we still must send a configure.
     * Since xdg-shell protocol v5 we should ignore request of unsupported
     * capabilities, just schedule a empty configure when the client uses <5
     * protocol version.
     * wlr_xdg_surface_schedule_configure() is used to send an empty reply. */
    if (*(*c).xdg).initialized
        && wl_resource_get_version((*xdg_toplevel(c)).resource)
            < XDG_TOPLEVEL_WM_CAPABILITIES_SINCE_VERSION
    {
        wlr_xdg_surface_schedule_configure((*c).xdg);
    }
}

pub unsafe fn monocle(m: *mut Monitor) {
    let mut n = 0;
    for c in g!().clients.clone() {
        if !visibleon(c, m) || (*c).isfloating || (*c).isfullscreen {
            continue;
        }
        resize(c, (*m).w, false);
        n += 1;
    }
    if n > 0 {
        (*m).ltsymbol = format!("[{n}]");
    }
    let c = focustop(m);
    if !c.is_null() {
        wlr_scene_node_raise_to_top(node((*c).scene));
    }
}

pub unsafe fn motionabsolute(_: *mut c_void, data: *mut c_void) {
    /* This event is forwarded by the cursor when a pointer emits an _absolute_
     * motion event, from 0..1 on each axis. This happens, for example, when
     * wlroots is running under a Wayland window rather than KMS+DRM, and you
     * move the mouse over the window. You could enter the window from any
     * edge, so we have to warp the mouse there. Also, some hardware emits
     * these events. */
    let event = data as *mut wlr_pointer_motion_absolute_event;
    let cursor = g!().cursor;
    let device: *mut wlr_input_device = &mut (*(*event).pointer).base;
    let (mut lx, mut ly) = (0.0, 0.0);

    if (*event).time_msec == 0 {
        /* this is 0 with virtual pointers */
        wlr_cursor_warp_absolute(cursor, device, (*event).x, (*event).y);
    }

    wlr_cursor_absolute_to_layout_coords(cursor, device, (*event).x, (*event).y, &mut lx, &mut ly);
    let dx = lx - (*cursor).x;
    let dy = ly - (*cursor).y;
    motionnotify((*event).time_msec, device, dx, dy, dx, dy);
}

pub unsafe fn motionnotify(
    time: u32,
    device: *mut wlr_input_device,
    mut dx: f64,
    mut dy: f64,
    dx_unaccel: f64,
    dy_unaccel: f64,
) {
    let cursor = g!().cursor;
    let seat = g!().seat;
    if time != 0 && crate::infinity::motion(dx, dy) {
        wlr_idle_notifier_v1_notify_activity(g!().idle_notifier, seat);
        return;
    }

    /* Find the client under the pointer and send the event along. */
    let hit = xytonode((*cursor).x, (*cursor).y);
    let (mut surface, mut c, l, mut sx, mut sy) = (hit.surface, hit.c, hit.l, hit.nx, hit.ny);

    if g!().cursor_mode == CursorMode::Pressed
        && (*seat).drag.is_null()
        && surface != (*seat).pointer_state.focused_surface
    {
        let focused = (*seat).pointer_state.focused_surface;
        let (ty, w, focusedl) = toplevel_from_wlr_surface(focused);
        if ty >= 0 {
            c = w;
            surface = focused;
            sx = (*cursor).x
                - if !focusedl.is_null() { (*(*focusedl).scene).node.x } else { crate::infinity::origin(w).0 } as f64;
            sy = (*cursor).y
                - if !focusedl.is_null() { (*(*focusedl).scene).node.y } else { crate::infinity::origin(w).1 } as f64;
            if focusedl.is_null() {
                sx /= crate::infinity::scale(w);
                sy /= crate::infinity::scale(w);
            }
        }
    }

    /* time is 0 in internal calls meant to restore pointer focus. */
    if time != 0 {
        wlr_relative_pointer_manager_v1_send_relative_motion(
            g!().relative_pointer_mgr,
            seat,
            time as u64 * 1000,
            dx,
            dy,
            dx_unaccel,
            dy_unaccel,
        );

        for constraint in wl_list_items!(
            &mut (*g!().pointer_constraints).constraints,
            wlr_pointer_constraint_v1,
            link
        ) {
            cursorconstrain(constraint);
        }

        let ac = g!().active_constraint;
        let mode = g!().cursor_mode;
        if !ac.is_null() && mode != CursorMode::Resize && mode != CursorMode::Move {
            c = toplevel_from_wlr_surface((*ac).surface).1;
            if !c.is_null() && (*ac).surface == (*seat).pointer_state.focused_surface {
                let origin = crate::infinity::origin(c);
                let scale = crate::infinity::scale(c);
                sx = ((*cursor).x - origin.0 as f64) / scale - (*c).bw as f64;
                sy = ((*cursor).y - origin.1 as f64) / scale - (*c).bw as f64;
                let (mut sx_confined, mut sy_confined) = (0.0, 0.0);
                if wlr_region_confine(
                    &(*ac).region,
                    sx,
                    sy,
                    sx + dx / scale,
                    sy + dy / scale,
                    &mut sx_confined,
                    &mut sy_confined,
                ) {
                    dx = (sx_confined - sx) * scale;
                    dy = (sy_confined - sy) * scale;
                }

                if (*ac).type_ == WLR_POINTER_CONSTRAINT_V1_LOCKED {
                    return;
                }
            }
        }

        wlr_cursor_move(cursor, device, dx, dy);
        wlr_idle_notifier_v1_notify_activity(g!().idle_notifier, seat);

        /* Update selmon (even while dragging a window) */
        if SLOPPYFOCUS {
            g!().selmon = xytomon((*cursor).x, (*cursor).y);
            crate::workspace::activate(g!().selmon);
        }
    }

    /* Update drag icon's position */
    wlr_scene_node_set_position(
        node(g!().drag_icon),
        (*cursor).x.round() as i32,
        (*cursor).y.round() as i32,
    );

    /* If we are currently grabbing the mouse, handle and return */
    let grabc = g!().grabc;
    match g!().cursor_mode {
        CursorMode::Move if !grabc.is_null() => {
            /* Move the grabbed client to the new position. */
            let point = crate::infinity::world(grabc, (*cursor).x, (*cursor).y);
            let nx = point.x.round() as i32 - g!().grabcx;
            let ny = point.y.round() as i32 - g!().grabcy;
            let gw = (*grabc).geom;
            resize(grabc, wlr_box(nx, ny, gw.width, gw.height), true);
            let point = crate::infinity::screen(grabc, ((*grabc).geom.x + g!().grabcx) as f64, ((*grabc).geom.y + g!().grabcy) as f64);
            wlr_cursor_warp_closest(cursor, null_mut(), point.x, point.y);
            return;
        }
        CursorMode::Resize if !grabc.is_null() => {
            let gw = (*grabc).geom;
            let point = crate::infinity::world(grabc, (*cursor).x, (*cursor).y);
            resize(
                grabc,
                wlr_box(
                    gw.x,
                    gw.y,
                    point.x.round() as i32 - gw.x,
                    point.y.round() as i32 - gw.y,
                ),
                true,
            );
            let point = crate::infinity::screen(grabc, ((*grabc).geom.x + (*grabc).geom.width) as f64, ((*grabc).geom.y + (*grabc).geom.height) as f64);
            wlr_cursor_warp_closest(cursor, null_mut(), point.x, point.y);
            return;
        }
        _ => {}
    }

    if crate::infinity::panning() {
        wlr_seat_pointer_clear_focus(seat);
        wlr_cursor_set_xcursor(cursor, g!().cursor_mgr, c"grabbing".as_ptr());
        return;
    }

    /* If there's no client surface under the cursor, set the cursor image to
     * a default. This is what makes the cursor image appear when you move it
     * off of a client or over its border. */
    if surface.is_null() && (*seat).drag.is_null() {
        wlr_cursor_set_xcursor(cursor, g!().cursor_mgr, c"default".as_ptr());
    }

    pointerfocus(c, l, surface, sx, sy, time);
}

pub unsafe fn motionrelative(_: *mut c_void, data: *mut c_void) {
    /* This event is forwarded by the cursor when a pointer emits a _relative_
     * pointer motion event (i.e. a delta).  The cursor doesn't move unless we
     * tell it to. The cursor automatically handles constraining the motion to
     * the output layout, as well as any special configuration applied for the
     * specific input device which generated the event. You can pass NULL for
     * the device if you want to move the cursor around without any input. */
    let event = data as *mut wlr_pointer_motion_event;
    motionnotify(
        (*event).time_msec,
        &mut (*(*event).pointer).base,
        (*event).delta_x,
        (*event).delta_y,
        (*event).unaccel_dx,
        (*event).unaccel_dy,
    );
}

pub unsafe fn moveresize(mode: CursorMode) {
    let cur = g!().cursor_mode;
    if cur != CursorMode::Normal && cur != CursorMode::Pressed {
        return;
    }
    let cursor = g!().cursor;
    let grabc = xytonode((*cursor).x, (*cursor).y).c;
    g!().grabc = grabc;
    if grabc.is_null() || client_is_unmanaged(grabc) || (*grabc).isfullscreen {
        return;
    }

    /* Float the window and tell motionnotify to grab it */
    if !crate::workspace::floating((*grabc).mon) { setfloating(grabc, true); }
    g!().cursor_mode = mode;
    match mode {
        CursorMode::Move => {
            let point = crate::infinity::world(grabc, (*cursor).x, (*cursor).y);
            g!().grabcx = point.x.round() as i32 - (*grabc).geom.x;
            g!().grabcy = point.y.round() as i32 - (*grabc).geom.y;
            wlr_cursor_set_xcursor(cursor, g!().cursor_mgr, c"all-scroll".as_ptr());
        }
        CursorMode::Resize => {
            /* Doesn't work for X11 output - the next absolute motion event
             * returns the cursor to where it started */
            let point = crate::infinity::screen(grabc, ((*grabc).geom.x + (*grabc).geom.width) as f64, ((*grabc).geom.y + (*grabc).geom.height) as f64);
            wlr_cursor_warp_closest(
                cursor,
                null_mut(),
                point.x,
                point.y,
            );
            wlr_cursor_set_xcursor(cursor, g!().cursor_mgr, c"se-resize".as_ptr());
        }
        _ => {}
    }
}

pub unsafe fn outputmgrapply(_: *mut c_void, data: *mut c_void) {
    outputmgrapplyortest(data.cast(), false);
}

pub unsafe fn outputmgrapplyortest(config: *mut wlr_output_configuration_v1, test: bool) {
    /*
     * Called when a client such as wlr-randr requests a change in output
     * configuration. This is only one way that the layout can be changed,
     * so any Monitor information should be updated by updatemons() after an
     * output_layout.change event, not here.
     */
    let mut states_len = 0usize;
    let states = wlr_output_configuration_v1_build_state(config, &mut states_len);

    if states.is_null() {
        wlr_output_configuration_v1_send_failed(config);
        wlr_output_configuration_v1_destroy(config);
        return;
    }

    let mut swapchain_manager: wlr_output_swapchain_manager = std::mem::zeroed();
    wlr_output_swapchain_manager_init(&mut swapchain_manager, g!().backend);

    let mut ok = wlr_output_swapchain_manager_prepare(&mut swapchain_manager, states, states_len);
    if ok && !test {
        for i in 0..states_len {
            let st = states.add(i);
            let swapchain =
                wlr_output_swapchain_manager_get_swapchain(&mut swapchain_manager, (*st).output);
            if !swapchain.is_null() && !(*(*st).output).enabled {
                wlr_output_state_set_buffer(&mut (*st).base, wlr_swapchain_acquire(swapchain));
            }
        }

        ok = wlr_backend_commit(g!().backend, states, states_len);
        if ok {
            wlr_output_swapchain_manager_apply(&mut swapchain_manager);

            for head in wl_list_items!(
                &mut (*config).heads,
                wlr_output_configuration_head_v1,
                link
            ) {
                let wlr_output = (*head).state.output;
                let m = (*wlr_output).data as *mut Monitor;
                if m.is_null() {
                    continue;
                }

                /* Ensure displays previously disabled by
                 * wlr-output-power-management-v1 are properly handled */
                (*m).asleep = false;

                /* Don't move monitors if position wouldn't change. This avoids
                 * wlroots marking the output as manually configured.
                 * wlr_output_layout_add does not like disabled outputs */
                if (*head).state.enabled
                    && ((*m).m.x != (*head).state.x
                        || (*m).m.y != (*head).state.y
                        || wlr_output_layout_get(g!().output_layout, wlr_output).is_null())
                {
                    wlr_output_layout_add(
                        g!().output_layout,
                        wlr_output,
                        (*head).state.x,
                        (*head).state.y,
                    );
                }
            }
        }
    }

    wlr_output_swapchain_manager_finish(&mut swapchain_manager);
    for i in 0..states_len {
        wlr_output_state_finish(&mut (*states.add(i)).base);
    }
    libc::free(states.cast());

    if ok {
        wlr_output_configuration_v1_send_succeeded(config);
    } else {
        wlr_output_configuration_v1_send_failed(config);
    }
    wlr_output_configuration_v1_destroy(config);

    /* https://codeberg.org/dwl/dwl/issues/577 */
    updatemons(null_mut(), null_mut());
}

pub unsafe fn outputmgrtest(_: *mut c_void, data: *mut c_void) {
    outputmgrapplyortest(data.cast(), true);
}

pub unsafe fn pointerfocus(
    c: *mut Client,
    l: *mut LayerSurface,
    surface: *mut wlr_surface,
    sx: f64,
    sy: f64,
    time: u32,
) {
    let seat = g!().seat;
    if surface != (*seat).pointer_state.focused_surface && SLOPPYFOCUS && time != 0 {
        if !c.is_null() && (!client_is_unmanaged(c) || client_wants_focus(c)) {
            focusclient(c, false);
        } else if !l.is_null() && (*(*l).layer_surface).current.keyboard_interactive != 0 {
            focusclient(null_mut(), false);
            client_notify_enter((*(*l).layer_surface).surface, seat_keyboard());
        }
    }

    /* If surface is NULL, clear pointer focus */
    if surface.is_null() {
        wlr_seat_pointer_notify_clear_focus(seat);
        return;
    }

    let time = if time == 0 { now_msec() } else { time };

    /* Let the client know that the mouse cursor has entered one of its
     * surfaces, and make keyboard focus follow if desired.  wlroots makes
     * this a no-op if surface is already focused */
    wlr_seat_pointer_notify_enter(seat, surface, sx, sy);
    wlr_seat_pointer_notify_motion(seat, time, sx, sy);
}

pub unsafe fn printstatus() {
    let mut out = String::new();
    let camera = g!().canvas.position;
    let _ = writeln!(out, "canvas {:.0} {:.0} {:.3}", camera.x, camera.y, g!().canvas.scale);
    for client in g!().clients.clone() {
        let geometry = (*client).geom;
        let _ = writeln!(out, "window {} {} {} {} {} {}", client_get_appid(client), geometry.x, geometry.y, geometry.width, geometry.height, (g!().focused_client == client) as u8);
    }
    for &m in g!().mons.iter() {
        let name = cstr((*(*m).wlr_output).name);
        let (mut occ, mut urg) = (0u32, 0u32);
        for &c in g!().clients.iter() {
            if (*c).mon != m {
                continue;
            }
            occ |= (*c).tags;
            if (*c).isurgent {
                urg |= (*c).tags;
            }
        }
        let c = focustop(m);
        let sel = if !c.is_null() {
            let _ = writeln!(out, "{name} title {}", client_get_title(c));
            let _ = writeln!(out, "{name} appid {}", client_get_appid(c));
            let _ = writeln!(out, "{name} fullscreen {}", (*c).isfullscreen as i32);
            let _ = writeln!(out, "{name} floating {}", ((*c).isfloating || crate::workspace::floating(m)) as i32);
            (*c).tags
        } else {
            let _ = writeln!(out, "{name} title ");
            let _ = writeln!(out, "{name} appid ");
            let _ = writeln!(out, "{name} fullscreen ");
            let _ = writeln!(out, "{name} floating ");
            0
        };

        let _ = writeln!(out, "{name} selmon {}", (m == g!().selmon) as u32);
        let _ = writeln!(
            out,
            "{name} tags {occ} {} {sel} {urg}",
            (*m).tagset[(*m).seltags]
        );
        let _ = writeln!(out, "{name} layout {}", (*m).ltsymbol);
        let _ = writeln!(out, "{name} zoom {:.3}", crate::workspace::camera(m).1);
    }
    crate::desktop::publish(&out);
    write_stdout(&out);
}

pub unsafe fn powermgrsetmode(_: *mut c_void, data: *mut c_void) {
    let event = data as *mut wlr_output_power_v1_set_mode_event;
    let m = (*(*event).output).data as *mut Monitor;
    if m.is_null() {
        return;
    }

    (*m).gamma_lut_changed = true; /* Reapply gamma LUT when re-enabling the output */
    let mut state: wlr_output_state = std::mem::zeroed();
    wlr_output_state_init(&mut state);
    wlr_output_state_set_enabled(&mut state, (*event).mode != 0);
    wlr_output_commit_state((*m).wlr_output, &state);
    wlr_output_state_finish(&mut state);

    (*m).asleep = (*event).mode == 0;
    updatemons(null_mut(), null_mut());
}

pub unsafe fn quit() {
    if !G.is_null() && !g!().dpy.is_null() {
        wl_display_terminate(g!().dpy);
    }
}

pub unsafe fn rendermon(m: *mut Monitor, _: *mut c_void) {
    crate::canvas_scene::sync();
    /* This function is called every time an output is ready to display a
     * frame, generally at the output's refresh rate (e.g. 60Hz). */
    if !wlr_scene_output_needs_frame((*m).scene_output) {
        return;
    }
    wlr_scene_output_commit((*m).scene_output, null());
    let mut now = monotonic_now();
    wlr_scene_output_send_frame_done((*m).scene_output, &mut now as *mut _ as *mut timespec);
}

pub unsafe fn requestdecorationmode(c: *mut Client, _: *mut c_void) {
    if (*(*c).xdg).initialized && !(*c).decoration.is_null() {
        wlr_xdg_toplevel_decoration_v1_set_mode(
            (*c).decoration,
            WLR_XDG_TOPLEVEL_DECORATION_V1_MODE_SERVER_SIDE,
        );
    }
}

pub unsafe fn requeststartdrag(_: *mut c_void, data: *mut c_void) {
    let event = data as *mut wlr_seat_request_start_drag_event;
    if wlr_seat_validate_pointer_grab_serial(g!().seat, (*event).origin, (*event).serial) {
        wlr_seat_start_pointer_drag(g!().seat, (*event).drag, (*event).serial);
    } else {
        wlr_data_source_destroy((*(*event).drag).source);
    }
}

pub unsafe fn requestmonstate(_: *mut Monitor, data: *mut c_void) {
    let event = data as *mut wlr_output_event_request_state;
    wlr_output_commit_state((*event).output, (*event).state);
    updatemons(null_mut(), null_mut());
}

pub unsafe fn resize(c: *mut Client, geo: wlr_box, interact: bool) {
    if (*c).mon.is_null() || !client_mapped(c) || (*c).border[0].is_null() {
        return;
    }

    let bbox = if interact { g!().sgeom } else { (*(*c).mon).w };

    client_set_bounds(c, geo.width, geo.height);
    (*c).geom = geo;
    applybounds(c, &bbox);

    /* Update scene-graph, including borders */
    let (gm, bw) = ((*c).geom, (*c).bw);
    wlr_scene_node_set_position(node((*c).scene), gm.x, gm.y);
    wlr_scene_node_set_position(node((*c).scene_surface), bw, bw);
    wlr_scene_rect_set_size((*c).border[0], gm.width, bw);
    wlr_scene_rect_set_size((*c).border[1], gm.width, bw);
    wlr_scene_rect_set_size((*c).border[2], bw, gm.height - 2 * bw);
    wlr_scene_rect_set_size((*c).border[3], bw, gm.height - 2 * bw);
    wlr_scene_node_set_position(rect_node((*c).border[1]), 0, gm.height - bw);
    wlr_scene_node_set_position(rect_node((*c).border[2]), 0, bw);
    wlr_scene_node_set_position(rect_node((*c).border[3]), gm.width - bw, bw);

    client_set_size(c, (gm.width - 2 * bw) as u32, (gm.height - 2 * bw) as u32);
    let clip = client_get_clip(c);
    wlr_scene_subsurface_tree_set_clip(node((*c).scene_surface), &clip);
    crate::canvas_scene::sync();
}

pub unsafe fn run(startup_cmd: Option<&str>) {
    /* Add a Unix socket to the Wayland display. */
    let socket = wl_display_add_socket_auto(g!().dpy);
    if socket.is_null() {
        die("startup: display_add_socket_auto");
    }
    std::env::set_var("WAYLAND_DISPLAY", &*cstr(socket));
    g!().desktop = Some(crate::desktop::Desktop::new().unwrap_or_else(|e| die(&format!("desktop: {e}"))));

    /* Start the backend. This will enumerate outputs and inputs, become the
     * DRM master, etc */
    if !wlr_backend_start(g!().backend) {
        die("startup: backend_start");
    }

    /* Now that the socket exists and the backend is started, run the startup
     * command */
    if let Some(cmd) = startup_cmd {
        let cmd = CString::new(cmd).unwrap_or_else(|_| die("startup: bad command"));
        let mut piperw = [0i32; 2];
        if libc::pipe(piperw.as_mut_ptr()) < 0 {
            die("startup: pipe:");
        }
        let pid = libc::fork();
        if pid < 0 {
            die("startup: fork:");
        }
        if pid == 0 {
            libc::setsid();
            libc::dup2(piperw[0], libc::STDIN_FILENO);
            libc::close(piperw[0]);
            libc::close(piperw[1]);
            libc::execl(
                c"/bin/sh".as_ptr(),
                c"/bin/sh".as_ptr(),
                c"-c".as_ptr(),
                cmd.as_ptr(),
                null::<c_char>(),
            );
            die("startup: execl:");
        }
        g!().child_pid = pid;
        libc::dup2(piperw[1], libc::STDOUT_FILENO);
        libc::close(piperw[1]);
        libc::close(piperw[0]);
    }

    /* Mark stdout as non-blocking to avoid the startup script causing dwl to
     * freeze when a user neither closes stdin nor consumes standard input in
     * his startup script */
    if !fd_set_nonblock(libc::STDOUT_FILENO) {
        libc::close(libc::STDOUT_FILENO);
    }

    printstatus();

    /* At this point the outputs are initialized, choose initial selmon based
     * on cursor position, and set default cursor image */
    let cursor = g!().cursor;
    g!().selmon = xytomon((*cursor).x, (*cursor).y);

    /* TODO hack to get cursor to display in its initial location (100, 100)
     * instead of (0, 0) and then jumping. Still may not be fully
     * initialized, as the image/coordinates are not transformed for the
     * monitor when displayed here */
    wlr_cursor_warp_closest(cursor, null_mut(), (*cursor).x, (*cursor).y);
    wlr_cursor_set_xcursor(cursor, g!().cursor_mgr, c"default".as_ptr());

    /* Run the Wayland event loop. This does not return until you exit the
     * compositor. Starting the backend rigged up all of the necessary event
     * loop configuration to listen to libinput events, DRM events, generate
     * frame events at the refresh rate, and so on. */
    wl_display_run(g!().dpy);
}

pub unsafe fn setcursor(_: *mut c_void, data: *mut c_void) {
    /* This event is raised by the seat when a client provides a cursor image */
    let event = data as *mut wlr_seat_pointer_request_set_cursor_event;
    /* If we're "grabbing" the cursor, don't use the client's image, we will
     * restore it after "grabbing" sending a leave event, followed by a enter
     * event, which will result in the client requesting set the cursor
     * surface */
    let mode = g!().cursor_mode;
    if mode != CursorMode::Normal && mode != CursorMode::Pressed {
        return;
    }
    /* This can be sent by any client, so we check to make sure this one
     * actually has pointer focus first. If so, we can tell the cursor to use
     * the provided surface as the cursor image. It will set the hardware
     * cursor on the output that it's currently on and continue to do so as
     * the cursor moves between outputs. */
    if (*event).seat_client == (*g!().seat).pointer_state.focused_client {
        wlr_cursor_set_surface(g!().cursor, (*event).surface, (*event).hotspot_x, (*event).hotspot_y);
    }
}

pub unsafe fn setcursorshape(_: *mut c_void, data: *mut c_void) {
    let event = data as *mut wlr_cursor_shape_manager_v1_request_set_shape_event;
    let mode = g!().cursor_mode;
    if mode != CursorMode::Normal && mode != CursorMode::Pressed {
        return;
    }
    /* This can be sent by any client, so we check to make sure this one
     * actually has pointer focus first. If so, we can tell the cursor to use
     * the provided cursor shape. */
    if (*event).seat_client == (*g!().seat).pointer_state.focused_client {
        wlr_cursor_set_xcursor(g!().cursor, g!().cursor_mgr, wlr_cursor_shape_v1_name((*event).shape));
    }
}

pub unsafe fn setfloating(c: *mut Client, floating: bool) {
    let p = client_get_parent(c);
    (*c).isfloating = floating;
    /* If in floating layout do not change the client's layer */
    let m = (*c).mon;
    if m.is_null() || !client_mapped(c) || crate::workspace::floating(m) {
        return;
    }
    let layer = if (*c).isfullscreen || (!p.is_null() && (*p).isfullscreen) {
        LYR_FS
    } else if (*c).isfloating {
        LYR_FLOAT
    } else {
        LYR_TILE
    };
    wlr_scene_node_reparent(node((*c).scene), g!().layers[layer]);
    arrange(m);
    printstatus();
}

pub unsafe fn setfullscreen(c: *mut Client, fullscreen: bool) {
    (*c).isfullscreen = fullscreen;
    if (*c).mon.is_null() || !client_mapped(c) {
        return;
    }
    (*c).bw = if fullscreen { 0 } else { BORDERPX };
    client_set_fullscreen(c, fullscreen);
    let layer = if (*c).isfullscreen {
        LYR_FS
    } else if (*c).isfloating {
        LYR_FLOAT
    } else {
        LYR_TILE
    };
    wlr_scene_node_reparent(node((*c).scene), g!().layers[layer]);

    if fullscreen {
        (*c).prev = (*c).geom;
        resize(c, (*(*c).mon).m, false);
    } else {
        /* restore previous size instead of arrange for floating windows since
         * client positions are set by the user and cannot be recalculated */
        resize(c, (*c).prev, false);
    }
    arrange((*c).mon);
    printstatus();
}

pub unsafe fn setlayout(lt: Option<usize>) {
    crate::workspace::set_layout(lt);
}

/// arg > 1.0 will set mfact absolutely
pub unsafe fn setmfact(f: f32) {
    let selmon = g!().selmon;
    if selmon.is_null() || crate::workspace::floating(selmon) {
        return;
    }
    let f = if f < 1.0 { f + (*selmon).mfact } else { f - 1.0 };
    if !(0.1..=0.9).contains(&f) {
        return;
    }
    (*selmon).mfact = f;
    arrange(selmon);
}

pub unsafe fn setmon(c: *mut Client, m: *mut Monitor, newtags: u32) {
    let oldmon = (*c).mon;
    if oldmon == m {
        return;
    }
    (*c).mon = m;
    if oldmon.is_null() && !m.is_null() && client_mapped(c) {
        crate::infinity::place(c, m);
        if (*c).float_pinned { (*c).floating_geom = (*c).geom; }
    }
    (*c).prev = (*c).geom;

    /* Scene graph sends surface leave/enter events on move and resize */
    if !oldmon.is_null() {
        arrange(oldmon);
    }
    if !m.is_null() {
        /* Make sure window actually overlaps with the monitor */
        resize(c, (*c).geom, false);
        /* assign tags of target monitor */
        (*c).tags = if newtags != 0 { newtags } else { (*m).tagset[(*m).seltags] };
        setfullscreen(c, (*c).isfullscreen); /* This will call arrange(c->mon) */
        setfloating(c, (*c).isfloating);
    }
    focusclient(focustop(g!().selmon), true);
}

pub unsafe fn setpsel(_: *mut c_void, data: *mut c_void) {
    /* This event is raised by the seat when a client wants to set the
     * selection, usually when the user copies something. wlroots allows
     * compositors to ignore such requests if they so choose, but in dwl we
     * always honor them */
    let event = data as *mut wlr_seat_request_set_primary_selection_event;
    wlr_seat_set_primary_selection(g!().seat, (*event).source, (*event).serial);
}

pub unsafe fn setsel(_: *mut c_void, data: *mut c_void) {
    let event = data as *mut wlr_seat_request_set_selection_event;
    wlr_seat_set_selection(g!().seat, (*event).source, (*event).serial);
}

pub unsafe fn setup() {
    G = Box::into_raw(Box::new(Dwl::new()));
    crate::lua_config::reload();

    let mut sa: libc::sigaction = std::mem::zeroed();
    sa.sa_flags = libc::SA_RESTART;
    sa.sa_sigaction = handlesig as extern "C" fn(libc::c_int) as usize;
    libc::sigemptyset(&mut sa.sa_mask);
    for sig in [libc::SIGCHLD, libc::SIGINT, libc::SIGTERM, libc::SIGPIPE] {
        libc::sigaction(sig, &sa, null_mut());
    }

    wlr_log_init(crate::LOG_LEVEL, None);

    /* The Wayland display is managed by libwayland. It handles accepting
     * clients from the Unix socket, managing Wayland globals, and so on. */
    g!().dpy = wl_display_create();
    g!().event_loop = wl_display_get_event_loop(g!().dpy);
    let dpy = g!().dpy;

    /* The backend is a wlroots feature which abstracts the underlying input
     * and output hardware. The autocreate option will choose the most
     * suitable backend based on the current environment, such as opening an
     * X11 window if an X11 server is running. */
    g!().backend = wlr_backend_autocreate(g!().event_loop, &mut g!().session);
    if g!().backend.is_null() {
        die("couldn't create backend");
    }
    let backend = g!().backend;

    /* Initialize the scene graph used to lay out windows */
    g!().scene = wlr_scene_create();
    let scene = g!().scene;
    g!().root_bg = wlr_scene_rect_create(&mut (*scene).tree, 0, 0, ROOTCOLOR.as_ptr());
    for i in 0..NUM_LAYERS {
        g!().layers[i] = wlr_scene_tree_create(&mut (*scene).tree);
    }
    g!().drag_icon = wlr_scene_tree_create(&mut (*scene).tree);
    wlr_scene_node_place_below(node(g!().drag_icon), node(g!().layers[LYR_BLOCK]));
    crate::canvas_scene::init();

    /* Autocreates a renderer, either Pixman, GLES2 or Vulkan for us. The user
     * can also specify a renderer using the WLR_RENDERER env var.
     * The renderer is responsible for defining the various pixel formats it
     * supports for shared memory, this configures that for clients. */
    g!().drw = wlr_renderer_autocreate(backend);
    if g!().drw.is_null() {
        die("couldn't create renderer");
    }
    let drw = g!().drw;
    g!().gpu_reset = Listener::new(&mut (*drw).events.lost, null_mut::<c_void>(), gpureset);

    /* Create shm, drm and linux_dmabuf interfaces by ourselves.
     * The simplest way is to call:
     *      wlr_renderer_init_wl_display(drw);
     * but we need to create the linux_dmabuf interface manually to integrate
     * it with wlr_scene. */
    wlr_renderer_init_wl_shm(drw, dpy);

    if !wlr_renderer_get_texture_formats(drw, WLR_BUFFER_CAP_DMABUF).is_null() {
        wlr_drm_create(dpy, drw);
        wlr_scene_set_linux_dmabuf_v1(scene, wlr_linux_dmabuf_v1_create_with_renderer(dpy, 5, drw));
    }

    let drm_fd = wlr_renderer_get_drm_fd(drw);
    if drm_fd >= 0 && (*drw).features.timeline && (*backend).features.timeline {
        wlr_linux_drm_syncobj_manager_v1_create(dpy, 1, drm_fd);
    }

    /* Autocreates an allocator for us.  The allocator is the bridge between
     * the renderer and the backend. It handles the buffer creation, allowing
     * wlroots to render onto the screen */
    g!().alloc = wlr_allocator_autocreate(backend, drw);
    if g!().alloc.is_null() {
        die("couldn't create allocator");
    }

    /* This creates some hands-off wlroots interfaces. The compositor is
     * necessary for clients to allocate surfaces and the data device manager
     * handles the clipboard. Each of these wlroots interfaces has room for
     * you to dig your fingers in and play with their behavior if you want.
     * Note that the clients cannot set the selection directly without
     * compositor approval, see the setsel() function. */
    g!().compositor = wlr_compositor_create(dpy, 6, drw);
    wlr_subcompositor_create(dpy);
    wlr_data_device_manager_create(dpy);
    wlr_export_dmabuf_manager_v1_create(dpy);
    wlr_screencopy_manager_v1_create(dpy);
    wlr_ext_image_copy_capture_manager_v1_create(dpy, 1);
    wlr_ext_output_image_capture_source_manager_v1_create(dpy, 1);
    wlr_data_control_manager_v1_create(dpy);
    wlr_ext_data_control_manager_v1_create(dpy, 1);
    wlr_primary_selection_v1_device_manager_create(dpy);
    wlr_viewporter_create(dpy);
    wlr_single_pixel_buffer_manager_v1_create(dpy);
    wlr_fractional_scale_manager_v1_create(dpy, 1);
    wlr_presentation_create(dpy, backend, 2);
    wlr_alpha_modifier_v1_create(dpy);

    /* Initializes the interface used to implement urgency hints */
    g!().activation = wlr_xdg_activation_v1_create(dpy);
    listen_global(&mut (*g!().activation).events.request_activate, urgent);

    wlr_scene_set_gamma_control_manager_v1(scene, wlr_gamma_control_manager_v1_create(dpy));

    g!().power_mgr = wlr_output_power_manager_v1_create(dpy);
    listen_global(&mut (*g!().power_mgr).events.set_mode, powermgrsetmode);

    /* Creates an output layout, which is a wlroots utility for working with
     * an arrangement of screens in a physical layout. */
    g!().output_layout = wlr_output_layout_create(dpy);
    listen_global(&mut (*g!().output_layout).events.change, updatemons);

    wlr_xdg_output_manager_v1_create(dpy, g!().output_layout);

    /* Configure a listener to be notified when new outputs are available on
     * the backend. */
    listen_global(&mut (*backend).events.new_output, createmon);

    /* Set up our client lists, the xdg-shell and the layer-shell. The
     * xdg-shell is a Wayland protocol which is used for application windows.
     * For more detail on shells, refer to the article:
     *
     * https://drewdevault.com/2018/07/29/Wayland-shells.html
     */
    g!().xdg_shell = wlr_xdg_shell_create(dpy, 6);
    listen_global(&mut (*g!().xdg_shell).events.new_toplevel, createnotify);
    listen_global(&mut (*g!().xdg_shell).events.new_popup, createpopup);

    g!().layer_shell = wlr_layer_shell_v1_create(dpy, 4);
    listen_global(&mut (*g!().layer_shell).events.new_surface, createlayersurface);

    g!().idle_notifier = wlr_idle_notifier_v1_create(dpy);

    g!().idle_inhibit_mgr = wlr_idle_inhibit_v1_create(dpy);
    listen_global(&mut (*g!().idle_inhibit_mgr).events.new_inhibitor, createidleinhibitor);

    g!().session_lock_mgr = wlr_session_lock_manager_v1_create(dpy);
    listen_global(&mut (*g!().session_lock_mgr).events.new_lock, locksession);
    let locked_color = [0.1f32, 0.1, 0.1, 1.0];
    g!().locked_bg = wlr_scene_rect_create(
        g!().layers[LYR_BLOCK],
        g!().sgeom.width,
        g!().sgeom.height,
        locked_color.as_ptr(),
    );
    wlr_scene_node_set_enabled(rect_node(g!().locked_bg), false);

    g!().foreign_toplevel_list = wlr_ext_foreign_toplevel_list_v1_create(dpy, 1);
    g!().foreign_toplevel_capture_mgr =
        wlr_ext_foreign_toplevel_image_capture_source_manager_v1_create(dpy, 1);
    listen_global(&mut (*g!().foreign_toplevel_capture_mgr).events.new_request, capturerequest);

    /* Use decoration protocols to negotiate server-side decorations */
    wlr_server_decoration_manager_set_default_mode(
        wlr_server_decoration_manager_create(dpy),
        WLR_SERVER_DECORATION_MANAGER_MODE_SERVER,
    );
    g!().xdg_decoration_mgr = wlr_xdg_decoration_manager_v1_create(dpy);
    listen_global(&mut (*g!().xdg_decoration_mgr).events.new_toplevel_decoration, createdecoration);

    g!().pointer_constraints = wlr_pointer_constraints_v1_create(dpy);
    listen_global(&mut (*g!().pointer_constraints).events.new_constraint, createpointerconstraint);

    g!().relative_pointer_mgr = wlr_relative_pointer_manager_v1_create(dpy);

    /* Creates a cursor, which is a wlroots utility for tracking the cursor
     * image shown on screen. */
    g!().cursor = wlr_cursor_create();
    let cursor = g!().cursor;
    wlr_cursor_attach_output_layout(cursor, g!().output_layout);

    /* Creates an xcursor manager, another wlroots utility which loads up
     * Xcursor themes to source cursor images from and makes sure that cursor
     * images are available at all scale factors on the screen (necessary for
     * HiDPI support). Scaled cursors will be loaded with each output. */
    g!().cursor_mgr = wlr_xcursor_manager_create(null(), 24);
    std::env::set_var("XCURSOR_SIZE", "24");

    /* wlr_cursor *only* displays an image on screen. It does not move around
     * when the pointer moves. However, we can attach input devices to it, and
     * it will generate aggregate events for all of them. In these events, we
     * can choose how we want to process them, forwarding them to clients and
     * moving the cursor around. More detail on this process is described in
     * https://drewdevault.com/2018/07/17/Input-handling-in-wlroots.html */
    listen_global(&mut (*cursor).events.motion, motionrelative);
    listen_global(&mut (*cursor).events.motion_absolute, motionabsolute);
    listen_global(&mut (*cursor).events.button, buttonpress);
    listen_global(&mut (*cursor).events.axis, axisnotify);
    listen_global(&mut (*cursor).events.frame, cursorframe);
    listen_global(&mut (*cursor).events.swipe_begin, crate::desktop::swipe_begin);
    listen_global(&mut (*cursor).events.swipe_update, crate::desktop::swipe_update);
    listen_global(&mut (*cursor).events.swipe_end, crate::desktop::swipe_end);
    listen_global(&mut (*cursor).events.pinch_begin, crate::desktop::pinch_begin);
    listen_global(&mut (*cursor).events.pinch_update, crate::desktop::pinch_update);
    listen_global(&mut (*cursor).events.pinch_end, crate::desktop::pinch_end);
    listen_global(&mut (*cursor).events.hold_begin, crate::infinity::hold_begin);
    listen_global(&mut (*cursor).events.hold_end, crate::infinity::hold_end);
    listen_global(&mut (*cursor).events.touch_down, crate::desktop::touch_down);
    listen_global(&mut (*cursor).events.touch_motion, crate::desktop::touch_motion);
    listen_global(&mut (*cursor).events.touch_up, crate::desktop::touch_up);
    listen_global(&mut (*cursor).events.touch_cancel, crate::desktop::touch_cancel);
    listen_global(&mut (*cursor).events.touch_frame, crate::desktop::touch_frame);

    g!().cursor_shape_mgr = wlr_cursor_shape_manager_v1_create(dpy, 1);
    listen_global(&mut (*g!().cursor_shape_mgr).events.request_set_shape, setcursorshape);

    /* Configures a seat, which is a single "seat" at which a user sits and
     * operates the computer. This conceptually includes up to one keyboard,
     * pointer, touch, and drawing tablet device. We also rig up a listener to
     * let us know when new input devices are available on the backend. */
    listen_global(&mut (*backend).events.new_input, inputdevice);
    g!().virtual_keyboard_mgr = wlr_virtual_keyboard_manager_v1_create(dpy);
    listen_global(&mut (*g!().virtual_keyboard_mgr).events.new_virtual_keyboard, virtualkeyboard);
    g!().virtual_pointer_mgr = wlr_virtual_pointer_manager_v1_create(dpy);
    listen_global(&mut (*g!().virtual_pointer_mgr).events.new_virtual_pointer, virtualpointer);

    g!().seat = wlr_seat_create(dpy, c"seat0".as_ptr());
    let seat = g!().seat;
    listen_global(&mut (*seat).events.request_set_cursor, setcursor);
    listen_global(&mut (*seat).events.request_set_selection, setsel);
    listen_global(&mut (*seat).events.request_set_primary_selection, setpsel);
    listen_global(&mut (*seat).events.request_start_drag, requeststartdrag);
    listen_global(&mut (*seat).events.start_drag, startdrag);

    g!().kb_group = createkeyboardgroup();

    g!().output_mgr = wlr_output_manager_v1_create(dpy);
    listen_global(&mut (*g!().output_mgr).events.apply, outputmgrapply);
    listen_global(&mut (*g!().output_mgr).events.test, outputmgrtest);

    g!().input_method_manager = wlr_input_method_manager_v2_create(dpy);
    g!().text_input_manager = wlr_text_input_manager_v3_create(dpy);
    g!().input_method_relay = input_method_relay_create();

    /* Make sure XWayland clients don't connect to the parent X server,
     * e.g when running in the x11 backend or the wayland backend and the
     * compositor has Xwayland support */
    std::env::remove_var("DISPLAY");
    #[cfg(feature = "xwayland")]
    crate::xwayland::setup_xwayland();
}

pub unsafe fn spawn(argv: &[&str]) {
    let Some((prog, args)) = argv.split_first() else {
        return;
    };
    let mut cmd = std::process::Command::new(prog);
    cmd.args(args);
    // dwl: dup2(STDERR_FILENO, STDOUT_FILENO)
    if let Ok(err) = std::io::stderr().as_fd().try_clone_to_owned() {
        cmd.stdout(err);
    }
    cmd.pre_exec(|| {
        libc::setsid();
        Ok(())
    });
    if let Err(e) = cmd.spawn() {
        eprintln!("iwc: execvp {prog} failed: {e}");
    }
}

pub unsafe fn startdrag(_: *mut c_void, data: *mut c_void) {
    let drag = data as *mut wlr_drag;
    if (*drag).icon.is_null() {
        return;
    }
    let scene_icon = wlr_scene_drag_icon_create(g!().drag_icon, (*drag).icon);
    (*(*drag).icon).data = (&mut (*scene_icon).node as *mut wlr_scene_node).cast();
    listen_static(&mut (*(*drag).icon).events.destroy, destroydragicon);
}

pub unsafe fn tag(ui: u32) {
    let selmon = g!().selmon;
    let sel = focustop(selmon);
    if sel.is_null() || (ui & TAGMASK) == 0 {
        return;
    }
    (*sel).tags = ui & TAGMASK;
    focusclient(focustop(selmon), true);
    arrange(selmon);
    printstatus();
}

pub unsafe fn tagmon(dir: wlr_direction) {
    let sel = focustop(g!().selmon);
    if !sel.is_null() {
        setmon(sel, dirtomon(dir), 0);
    }
}

pub unsafe fn tile(m: *mut Monitor) {
    let tiled: Vec<*mut Client> = g!()
        .clients
        .iter()
        .copied()
        .filter(|&c| visibleon(c, m) && !(*c).isfloating && !(*c).isfullscreen)
        .collect();
    let n = tiled.len() as i32;
    if n == 0 {
        return;
    }

    let outer = (*m).w;
    let gap = g!().settings.gaps.min((outer.width / 8).max(0)).min((outer.height / (n * 4)).max(0));
    let w = wlr_box(outer.x + gap / 2, outer.y + gap / 2,
                    outer.width - gap, outer.height - gap);
    let nmaster = (*m).nmaster;
    let mw = if n > nmaster {
        if nmaster != 0 { (w.width as f32 * (*m).mfact).round() as i32 } else { 0 }
    } else {
        w.width
    };
    let (mut my, mut ty) = (0, 0);
    for (i, c) in tiled.into_iter().enumerate() {
        let i = i as i32;
        if i < nmaster {
            let h = (w.height - my) / (n.min(nmaster) - i);
            resize(c, wlr_box(w.x + gap / 2, w.y + my + gap / 2, mw - gap, h - gap), false);
            my += h;
        } else {
            let h = (w.height - ty) / (n - i);
            resize(c, wlr_box(w.x + mw + gap / 2, w.y + ty + gap / 2, w.width - mw - gap, h - gap), false);
            ty += h;
        }
    }
}

pub unsafe fn togglefloating() {
    let sel = focustop(g!().selmon);
    /* return if fullscreen */
    if !sel.is_null() && !(*sel).isfullscreen {
        setfloating(sel, !(*sel).isfloating);
        (*sel).float_pinned = (*sel).isfloating;
    }
}

pub unsafe fn togglefullscreen() {
    let sel = focustop(g!().selmon);
    if !sel.is_null() {
        setfullscreen(sel, !(*sel).isfullscreen);
    }
}

pub unsafe fn toggletag(ui: u32) {
    let selmon = g!().selmon;
    let sel = focustop(selmon);
    if sel.is_null() {
        return;
    }
    let newtags = (*sel).tags ^ (ui & TAGMASK);
    if newtags == 0 {
        return;
    }
    (*sel).tags = newtags;
    focusclient(focustop(selmon), true);
    arrange(selmon);
    printstatus();
}

pub unsafe fn toggleview(ui: u32) {
    let selmon = g!().selmon;
    if selmon.is_null() {
        return;
    }
    let newtagset = (*selmon).tagset[(*selmon).seltags] ^ (ui & TAGMASK);
    if newtagset == 0 {
        return;
    }
    (*selmon).tagset[(*selmon).seltags] = newtagset;
    crate::workspace::activate(selmon);
    focusclient(focustop(selmon), true);
    arrange(selmon);
    printstatus();
}

pub unsafe fn unlocksession(lock: *mut SessionLock, _: *mut c_void) {
    destroylock(lock, true);
}

pub unsafe fn unmaplayersurfacenotify(l: *mut LayerSurface, _: *mut c_void) {
    (*l).mapped = false;
    wlr_scene_node_set_enabled(node((*l).scene), false);
    if g!().exclusive_focus == Exclusive::Layer(l) {
        g!().exclusive_focus = Exclusive::None;
    }
    let ls = (*l).layer_surface;
    if !(*ls).output.is_null() {
        (*l).mon = (*(*ls).output).data.cast();
        if !(*l).mon.is_null() {
            arrangelayers((*l).mon);
        }
    }
    if (*ls).surface == (*g!().seat).keyboard_state.focused_surface {
        focusclient(focustop(g!().selmon), true);
    }
    motionnotify0();
}

pub unsafe fn unmapnotify(c: *mut Client, _: *mut c_void) {
    let was_focused = c == g!().focused_client;
    /* Called when the surface is unmapped, and should no longer be shown. */
    if c == g!().grabc {
        g!().cursor_mode = CursorMode::Normal;
        g!().grabc = null_mut();
    }
    if c == g!().focused_client {
        g!().focused_client = null_mut();
    }

    if client_is_unmanaged(c) {
        if g!().exclusive_focus == Exclusive::Client(c) {
            g!().exclusive_focus = Exclusive::None;
            focusclient(focustop(g!().selmon), true);
        }
    } else {
        remove_ptr(&mut g!().clients, c);
        setmon(c, null_mut(), 0);
        remove_ptr(&mut g!().fstack, c);
    }

    if !(*c).foreign_toplevel_handle.is_null() {
        wlr_ext_foreign_toplevel_handle_v1_destroy((*c).foreign_toplevel_handle);
        (*c).foreign_toplevel_handle = null_mut();
    }
    if (*c).kind != XDG_SHELL && !(*c).image_capture_scene_surface.is_null() {
        wlr_scene_node_destroy(&mut (*(*(*c).image_capture_scene_surface).buffer).node);
        (*c).image_capture_scene_surface = null_mut();
    }

    if !(*c).image_capture_scene.is_null() {
        wlr_scene_node_destroy(&mut (*(*c).image_capture_scene).tree.node);
    }
    (*c).image_capture_scene = null_mut();
    (*c).image_capture_tree = null_mut();
    (*c).image_capture_source = null_mut();
    wlr_scene_node_destroy(node((*c).scene));
    (*c).scene = null_mut();
    (*c).border = [null_mut(); 4];
    (*client_surface(c)).data = null_mut();
    printstatus();
    motionnotify0();
    if was_focused {
        crate::infinity::center(g!().focused_client);
    }
}

pub unsafe fn updatemons(_: *mut c_void, _: *mut c_void) {
    /*
     * Called whenever the output layout changes: adding or removing a
     * monitor, changing an output's mode or position, etc. This is where
     * the change officially happens and we update geometry, window
     * positions, focus, and the stored configuration in wlroots'
     * output-manager implementation.
     */
    let config = wlr_output_configuration_v1_create();
    let ol = g!().output_layout;

    /* First remove from the layout the disabled monitors */
    for m in g!().mons.clone() {
        if (*(*m).wlr_output).enabled {
            continue;
        }
        let config_head = wlr_output_configuration_head_v1_create(config, (*m).wlr_output);
        (*config_head).state.enabled = false;
        if (*m).asleep {
            continue;
        }

        /* Remove this output from the layout to avoid cursor enter inside it */
        wlr_output_layout_remove(ol, (*m).wlr_output);
        closemon(m);
        (*m).m = wlr_box::default();
        (*m).w = wlr_box::default();
    }
    /* Insert outputs that need to */
    for m in g!().mons.clone() {
        if (*(*m).wlr_output).enabled && wlr_output_layout_get(ol, (*m).wlr_output).is_null() {
            wlr_output_layout_add_auto(ol, (*m).wlr_output);
        }
    }

    /* Now that we update the output layout we can get its box */
    wlr_output_layout_get_box(ol, null_mut(), &mut g!().sgeom);
    let sgeom = g!().sgeom;

    wlr_scene_node_set_position(rect_node(g!().root_bg), sgeom.x, sgeom.y);
    wlr_scene_rect_set_size(g!().root_bg, sgeom.width, sgeom.height);

    /* Make sure the clients are hidden when dwl is locked */
    wlr_scene_node_set_position(rect_node(g!().locked_bg), sgeom.x, sgeom.y);
    wlr_scene_rect_set_size(g!().locked_bg, sgeom.width, sgeom.height);

    for m in g!().mons.clone() {
        if !(*(*m).wlr_output).enabled {
            continue;
        }
        let config_head = wlr_output_configuration_head_v1_create(config, (*m).wlr_output);

        /* Get the effective monitor geometry to use for surfaces */
        wlr_output_layout_get_box(ol, (*m).wlr_output, &mut (*m).m);
        (*m).w = (*m).m;
        wlr_scene_output_set_position((*m).scene_output, (*m).m.x, (*m).m.y);

        wlr_scene_node_set_position(rect_node((*m).fullscreen_bg), (*m).m.x, (*m).m.y);
        wlr_scene_rect_set_size((*m).fullscreen_bg, (*m).m.width, (*m).m.height);

        if !(*m).lock_surface.is_null() {
            let scene_tree = (*(*(*m).lock_surface).surface).data as *mut wlr_scene_tree;
            wlr_scene_node_set_position(node(scene_tree), (*m).m.x, (*m).m.y);
            wlr_session_lock_surface_v1_configure(
                (*m).lock_surface,
                (*m).m.width as u32,
                (*m).m.height as u32,
            );
        }

        /* Calculate the effective monitor geometry to use for clients */
        arrangelayers(m);
        /* Don't move clients to the left output when plugging monitors */
        arrange(m);
        /* make sure fullscreen clients have the right size */
        let c = focustop(m);
        if !c.is_null() && (*c).isfullscreen {
            resize(c, (*m).m, false);
        }

        /* Try to re-set the gamma LUT when updating monitors,
         * it's only really needed when enabling a disabled output, but meh. */
        (*m).gamma_lut_changed = true;

        (*config_head).state.x = (*m).m.x;
        (*config_head).state.y = (*m).m.y;

        if g!().selmon.is_null() {
            g!().selmon = m;
        }
    }

    let selmon = g!().selmon;
    if !selmon.is_null() && (*(*selmon).wlr_output).enabled {
        for c in g!().clients.clone() {
            if (*c).mon.is_null() && client_mapped(c) {
                setmon(c, selmon, (*c).tags);
            }
        }
        focusclient(focustop(selmon), true);
        if !(*selmon).lock_surface.is_null() {
            client_notify_enter((*(*selmon).lock_surface).surface, seat_keyboard());
            client_activate_surface((*(*selmon).lock_surface).surface, true);
        }
    }

    /* FIXME: figure out why the cursor image is at 0,0 after turning all
     * the monitors on.
     * Move the cursor image where it used to be. It does not generate a
     * wl_pointer.motion event for the clients, it's only the image what it's
     * at the wrong position after all. */
    wlr_cursor_move(g!().cursor, null_mut(), 0.0, 0.0);

    wlr_output_manager_v1_set_configuration(g!().output_mgr, config);
}

pub unsafe fn updatetitle(c: *mut Client, _: *mut c_void) {
    if c == focustop((*c).mon) {
        printstatus();
    }

    if !(*c).foreign_toplevel_handle.is_null() {
        let state = wlr_ext_foreign_toplevel_handle_v1_state {
            app_id: client_get_appid_ptr(c),
            title: client_get_title_ptr(c),
        };
        wlr_ext_foreign_toplevel_handle_v1_update_state((*c).foreign_toplevel_handle, &state);
    }
}

pub unsafe fn urgent(_: *mut c_void, data: *mut c_void) {
    let event = data as *mut wlr_xdg_activation_v1_request_activate_event;
    let c = toplevel_from_wlr_surface((*event).surface).1;
    if c.is_null() || c == focustop(g!().selmon) {
        return;
    }

    (*c).isurgent = true;
    printstatus();

    if client_mapped(c) {
        client_set_border_color(c, &URGENTCOLOR);
    }
}

pub unsafe fn view(ui: u32) {
    let selmon = g!().selmon;
    if selmon.is_null() || (ui & TAGMASK) == (*selmon).tagset[(*selmon).seltags] {
        return;
    }
    (*selmon).seltags ^= 1; /* toggle sel tagset */
    if ui & TAGMASK != 0 {
        (*selmon).tagset[(*selmon).seltags] = ui & TAGMASK;
    }
    crate::workspace::activate(selmon);
    focusclient(focustop(selmon), true);
    arrange(selmon);
    printstatus();
}

pub unsafe fn virtualkeyboard(_: *mut c_void, data: *mut c_void) {
    let kb = data as *mut wlr_virtual_keyboard_v1;
    /* virtual keyboards shouldn't share keyboard group */
    let group = createkeyboardgroup();
    (*group).virtual_keyboard = &mut (*kb).keyboard;
    /* Set the keymap to match the group keymap */
    wlr_keyboard_set_keymap(&mut (*kb).keyboard, (*(*group).wlr_group).keyboard.keymap);
    (*group).destroy =
        Listener::new(&mut (*kb).keyboard.base.events.destroy, group, destroykeyboardgroup);

    /* Add the new keyboard to the group */
    wlr_keyboard_group_add_keyboard((*group).wlr_group, &mut (*kb).keyboard);
    let seat = g!().seat;
    wlr_seat_set_capabilities(seat, (*seat).capabilities | WL_SEAT_CAPABILITY_KEYBOARD);
}

pub unsafe fn virtualpointer(_: *mut c_void, data: *mut c_void) {
    let event = data as *mut wlr_virtual_pointer_v1_new_pointer_event;
    let device: *mut wlr_input_device = &mut (*(*event).new_pointer).pointer.base;

    wlr_cursor_attach_input_device(g!().cursor, device);
    if !(*event).suggested_output.is_null() {
        wlr_cursor_map_input_to_output(g!().cursor, device, (*event).suggested_output);
    }
    let seat = g!().seat;
    wlr_seat_set_capabilities(seat, (*seat).capabilities | WL_SEAT_CAPABILITY_POINTER);
}

pub unsafe fn xytomon(x: f64, y: f64) -> *mut Monitor {
    let o = wlr_output_layout_output_at(g!().output_layout, x, y);
    if o.is_null() {
        null_mut()
    } else {
        (*o).data.cast()
    }
}

pub struct NodeHit {
    pub surface: *mut wlr_surface,
    pub c: *mut Client,
    pub l: *mut LayerSurface,
    pub nx: f64,
    pub ny: f64,
}

pub unsafe fn xytonode(x: f64, y: f64) -> NodeHit {
    let mut surface: *mut wlr_surface = null_mut();
    let (mut nx, mut ny) = (0.0, 0.0);

    for layer in (0..NUM_LAYERS).rev() {
        if !surface.is_null() {
            break;
        }
        if layer == LYR_IMPOPUP {
            continue;
        }
        let n = if [LYR_TILE, LYR_FLOAT, LYR_UNMANAGED].contains(&layer) {
            let mut hit = null_mut();
            let roots = wl_list_items!(&mut (*g!().layers[layer]).children, wlr_scene_node, link);
            for root in roots.into_iter().rev() {
                let client = (*root).data as *mut Client;
                if !client.is_null() && !(*client).mon.is_null() && (*client).mon != xytomon(x, y) { continue; }
                let point = crate::infinity::world((*root).data.cast(), x, y);
                hit = wlr_scene_node_at(root, point.x, point.y, &mut nx, &mut ny);
                if !hit.is_null() { break; }
            }
            hit
        } else {
            wlr_scene_node_at(node(g!().layers[layer]), x, y, &mut nx, &mut ny)
        };
        if n.is_null() {
            continue;
        }
        if (*n).type_ == WLR_SCENE_NODE_BUFFER {
            let ss = wlr_scene_surface_try_from_buffer(wlr_scene_buffer_from_node(n));
            if !ss.is_null() {
                surface = (*ss).surface;
            }
        }
    }

    let (_, c, l) = toplevel_from_wlr_surface(surface);
    NodeHit { surface, c, l, nx, ny }
}

pub unsafe fn zoom() {
    let selmon = g!().selmon;
    let mut sel = focustop(selmon);

    if sel.is_null()
        || selmon.is_null()
        || crate::workspace::floating(selmon)
        || (*sel).isfloating
    {
        return;
    }

    /* Search for the first tiled window that is not sel, marking sel as
     * NULL if we pass it along the way */
    let mut found: *mut Client = null_mut();
    for &c in g!().clients.iter() {
        if visibleon(c, selmon) && !(*c).isfloating {
            if c != sel {
                found = c;
                break;
            }
            sel = null_mut();
        }
    }

    /* Return if no other tiled window was found */
    if found.is_null() {
        return;
    }

    /* If we passed sel, move c to the front; otherwise, move sel to the front */
    if sel.is_null() {
        sel = found;
    }
    remove_ptr(&mut g!().clients, sel);
    g!().clients.insert(0, sel);

    focusclient(sel, true);
    arrange(selmon);
}

use std::os::fd::AsFd;
