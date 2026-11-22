//! Port of ime.h: relay between text-input-v3 clients and an input-method-v2
//! (IME) client.

use crate::prelude::*;

pub struct InputMethodRelay {
    pub text_inputs: Vec<*mut TextInput>,
    pub input_method: *mut wlr_input_method_v2,

    pub focused_surface: *mut wlr_surface,
    pub active_text_input: *mut TextInput,

    pub popups: Vec<*mut InputMethodPopup>,
    pub popup_tree: *mut wlr_scene_tree,

    pub new_text_input: Listener,
    pub new_input_method: Listener,

    pub input_method_commit: Listener,
    pub input_method_grab_keyboard: Listener,
    pub input_method_destroy: Listener,
    pub input_method_new_popup_surface: Listener,

    pub keyboard_grab_destroy: Listener,
    pub focused_surface_destroy: Listener,
}

pub struct InputMethodPopup {
    pub popup_surface: *mut wlr_input_popup_surface_v2,
    pub tree: *mut wlr_scene_tree,
    pub scene_surface: *mut wlr_scene_tree,
    pub relay: *mut InputMethodRelay,
    pub destroy: Listener,
    pub commit: Listener,
}

pub struct TextInput {
    pub relay: *mut InputMethodRelay,
    pub input: *mut wlr_text_input_v3,
    pub enable: Listener,
    pub commit: Listener,
    pub disable: Listener,
    pub destroy: Listener,
}

unsafe fn output_from_wlr_output(wlr_output: *mut wlr_output) -> *mut Monitor {
    g!().mons
        .iter()
        .copied()
        .find(|&m| (*m).wlr_output == wlr_output)
        .unwrap_or(null_mut())
}

unsafe fn output_nearest_to(lx: i32, ly: i32) -> *mut Monitor {
    let (mut closest_x, mut closest_y) = (0.0, 0.0);
    let ol = g!().output_layout;
    wlr_output_layout_closest_point(ol, null_mut(), lx as f64, ly as f64, &mut closest_x, &mut closest_y);
    output_from_wlr_output(wlr_output_layout_output_at(ol, closest_x, closest_y))
}

unsafe fn keyboard_is_emulated_by_input_method(
    keyboard: *mut wlr_keyboard,
    input_method: *mut wlr_input_method_v2,
) -> bool {
    if keyboard.is_null() || input_method.is_null() {
        return false;
    }
    let virtual_keyboard = wlr_input_device_get_virtual_keyboard(&mut (*keyboard).base);
    !virtual_keyboard.is_null()
        && wl_resource_get_client((*virtual_keyboard).resource)
            == wl_resource_get_client((*input_method).resource)
}

unsafe fn get_keyboard_grab(keyboard: *mut KeyboardGroup) -> *mut wlr_input_method_keyboard_grab_v2 {
    let relay = g!().input_method_relay;
    if relay.is_null() {
        return null_mut();
    }
    let input_method = (*relay).input_method;
    if input_method.is_null()
        || (*input_method).keyboard_grab.is_null()
        || keyboard != g!().kb_group
        || keyboard_is_emulated_by_input_method((*keyboard).virtual_keyboard, input_method)
    {
        return null_mut();
    }
    (*input_method).keyboard_grab
}

pub unsafe fn input_method_keyboard_grab_forward_modifiers(keyboard: *mut KeyboardGroup) -> bool {
    let keyboard_grab = get_keyboard_grab(keyboard);
    if keyboard_grab.is_null() {
        return false;
    }
    let kb: *mut wlr_keyboard = &mut (*(*keyboard).wlr_group).keyboard;
    wlr_input_method_keyboard_grab_v2_set_keyboard(keyboard_grab, kb);
    wlr_input_method_keyboard_grab_v2_send_modifiers(keyboard_grab, &mut (*kb).modifiers);
    true
}

pub unsafe fn input_method_keyboard_grab_forward_key(
    keyboard: *mut KeyboardGroup,
    event: *mut wlr_keyboard_key_event,
) -> bool {
    let keyboard_grab = get_keyboard_grab(keyboard);
    if keyboard_grab.is_null() {
        return false;
    }
    wlr_input_method_keyboard_grab_v2_set_keyboard(keyboard_grab, &mut (*(*keyboard).wlr_group).keyboard);
    wlr_input_method_keyboard_grab_v2_send_key(
        keyboard_grab,
        (*event).time_msec,
        (*event).keycode,
        (*event).state,
    );
    true
}

unsafe fn get_active_text_input(relay: *mut InputMethodRelay) -> *mut TextInput {
    if (*relay).input_method.is_null() {
        return null_mut();
    }
    for &text_input in (*relay).text_inputs.iter() {
        let input = (*text_input).input;
        if !(*input).focused_surface.is_null() && (*input).current_enabled {
            return text_input;
        }
    }
    null_mut()
}

unsafe fn update_active_text_input(relay: *mut InputMethodRelay) {
    let active_text_input = get_active_text_input(relay);

    if !(*relay).input_method.is_null() && (*relay).active_text_input != active_text_input {
        if !active_text_input.is_null() {
            wlr_input_method_v2_send_activate((*relay).input_method);
        } else {
            wlr_input_method_v2_send_deactivate((*relay).input_method);
        }
        wlr_input_method_v2_send_done((*relay).input_method);
    }

    (*relay).active_text_input = active_text_input;
}

unsafe fn update_text_inputs_focused_surface(relay: *mut InputMethodRelay) {
    for text_input in (*relay).text_inputs.clone() {
        let input = (*text_input).input;
        let focused = (*relay).focused_surface;

        let new_focused_surface = if !(*relay).input_method.is_null()
            && !focused.is_null()
            && wl_resource_get_client((*input).resource) == wl_resource_get_client((*focused).resource)
        {
            focused
        } else {
            null_mut()
        };

        if (*input).focused_surface == new_focused_surface {
            continue;
        }
        if !(*input).focused_surface.is_null() {
            wlr_text_input_v3_send_leave(input);
        }
        if !new_focused_surface.is_null() {
            wlr_text_input_v3_send_enter(input, new_focused_surface);
        }
    }
}

unsafe fn update_popup_position(popup: *mut InputMethodPopup) {
    let relay = (*popup).relay;
    let text_input = (*relay).active_text_input;
    let popup_surface = (*(*popup).popup_surface).surface;

    if text_input.is_null() || (*relay).focused_surface.is_null() || !(*popup_surface).mapped {
        return;
    }

    let focused = (*relay).focused_surface;
    let xdg_surface = wlr_xdg_surface_try_from_wlr_surface(focused);
    let layer_surface = wlr_layer_surface_v1_try_from_wlr_surface(focused);
    let input = (*text_input).input;

    let mut cursor_rect = wlr_box::default();
    if ((*input).current.features & WLR_TEXT_INPUT_V3_FEATURE_CURSOR_RECTANGLE) != 0
        && (!xdg_surface.is_null() || !layer_surface.is_null())
    {
        cursor_rect = (*input).current.cursor_rectangle;

        let tree = (*focused).data as *mut wlr_scene_tree;
        if !tree.is_null() {
            let (mut lx, mut ly) = (0, 0);
            wlr_scene_node_coords(&mut (*tree).node, &mut lx, &mut ly);
            cursor_rect.x += lx;
            cursor_rect.y += ly;
        }

        if !xdg_surface.is_null() {
            cursor_rect.x -= (*xdg_surface).geometry.x;
            cursor_rect.y -= (*xdg_surface).geometry.y;
        }
        let (_, client, layer) = toplevel_from_wlr_surface(focused);
        if !client.is_null() && layer.is_null() {
            let point = crate::infinity::screen(client, cursor_rect.x as f64, cursor_rect.y as f64);
            let scale = crate::infinity::scale(client);
            cursor_rect.x = point.x.round() as i32;
            cursor_rect.y = point.y.round() as i32;
            cursor_rect.width = (cursor_rect.width as f64 * scale).round() as i32;
            cursor_rect.height = (cursor_rect.height as f64 * scale).round() as i32;
        }
    }

    let output = output_nearest_to(cursor_rect.x, cursor_rect.y);
    if output.is_null() || !(*(*output).wlr_output).enabled {
        return;
    }

    let mut output_box = wlr_box::default();
    wlr_output_layout_get_box(g!().output_layout, (*output).wlr_output, &mut output_box);

    let mut rules: wlr_xdg_positioner_rules = std::mem::zeroed();
    rules.anchor_rect = cursor_rect;
    rules.anchor = XDG_POSITIONER_ANCHOR_BOTTOM_LEFT;
    rules.gravity = XDG_POSITIONER_GRAVITY_BOTTOM_RIGHT;
    rules.size.width = (*popup_surface).current.width;
    rules.size.height = (*popup_surface).current.height;
    rules.constraint_adjustment =
        XDG_POSITIONER_CONSTRAINT_ADJUSTMENT_FLIP_Y | XDG_POSITIONER_CONSTRAINT_ADJUSTMENT_SLIDE_X;

    let mut popup_box = wlr_box::default();
    wlr_xdg_positioner_rules_get_geometry(&rules, &mut popup_box);
    wlr_xdg_positioner_rules_unconstrain_box(&rules, &output_box, &mut popup_box);

    wlr_scene_node_set_position(&mut (*(*popup).tree).node, popup_box.x, popup_box.y);
    wlr_scene_node_raise_to_top(&mut (*(*relay).popup_tree).node);

    let mut rect = wlr_box(
        cursor_rect.x - popup_box.x,
        cursor_rect.y - popup_box.y,
        cursor_rect.width,
        cursor_rect.height,
    );
    wlr_input_popup_surface_v2_send_text_input_rectangle((*popup).popup_surface, &mut rect);
}

unsafe fn update_popups_position(relay: *mut InputMethodRelay) {
    for popup in (*relay).popups.clone() {
        update_popup_position(popup);
    }
}

unsafe fn handle_input_method_commit(relay: *mut InputMethodRelay, _: *mut c_void) {
    let input_method = (*relay).input_method;
    let text_input = (*relay).active_text_input;
    if text_input.is_null() {
        return;
    }
    let input = (*text_input).input;
    let current = &(*input_method).current;

    if !current.preedit.text.is_null() {
        wlr_text_input_v3_send_preedit_string(
            input,
            current.preedit.text,
            current.preedit.cursor_begin,
            current.preedit.cursor_end,
        );
    }
    if !current.commit_text.is_null() {
        wlr_text_input_v3_send_commit_string(input, current.commit_text);
    }
    if current.delete.before_length != 0 || current.delete.after_length != 0 {
        wlr_text_input_v3_send_delete_surrounding_text(
            input,
            current.delete.before_length,
            current.delete.after_length,
        );
    }
    wlr_text_input_v3_send_done(input);
}

unsafe fn handle_keyboard_grab_destroy(relay: *mut InputMethodRelay, data: *mut c_void) {
    let keyboard_grab = data as *mut wlr_input_method_keyboard_grab_v2;
    (*relay).keyboard_grab_destroy.remove();

    if !keyboard_grab.is_null() && !(*keyboard_grab).keyboard.is_null() {
        /* Send modifier state to original client */
        wlr_seat_keyboard_notify_modifiers(
            (*(*keyboard_grab).input_method).seat,
            &mut (*(*keyboard_grab).keyboard).modifiers,
        );
    }
}

unsafe fn handle_input_method_grab_keyboard(relay: *mut InputMethodRelay, data: *mut c_void) {
    let keyboard_grab = data as *mut wlr_input_method_keyboard_grab_v2;
    let active_keyboard: *mut wlr_keyboard = &mut (*(*g!().kb_group).wlr_group).keyboard;

    if !keyboard_is_emulated_by_input_method(active_keyboard, (*relay).input_method) {
        /* Send modifier state to grab */
        wlr_input_method_keyboard_grab_v2_set_keyboard(keyboard_grab, active_keyboard);
    }

    (*relay).keyboard_grab_destroy =
        Listener::new(&mut (*keyboard_grab).events.destroy, relay, handle_keyboard_grab_destroy);
}

unsafe fn handle_input_method_destroy(relay: *mut InputMethodRelay, _: *mut c_void) {
    (*relay).input_method_commit.remove();
    (*relay).input_method_grab_keyboard.remove();
    (*relay).input_method_new_popup_surface.remove();
    (*relay).input_method_destroy.remove();
    (*relay).keyboard_grab_destroy.remove();
    (*relay).input_method = null_mut();

    update_text_inputs_focused_surface(relay);
    update_active_text_input(relay);
}

unsafe fn handle_popup_surface_destroy(popup: *mut InputMethodPopup, _: *mut c_void) {
    wlr_scene_node_destroy(&mut (*(*popup).tree).node);
    remove_ptr(&mut (*(*popup).relay).popups, popup);
    drop(Box::from_raw(popup));
}

unsafe fn handle_popup_surface_commit(popup: *mut InputMethodPopup, _: *mut c_void) {
    update_popup_position(popup);
}

unsafe fn handle_input_method_new_popup_surface(relay: *mut InputMethodRelay, data: *mut c_void) {
    let popup_surface = data as *mut wlr_input_popup_surface_v2;
    let popup = Box::into_raw(Box::new(InputMethodPopup {
        popup_surface,
        tree: null_mut(),
        scene_surface: null_mut(),
        relay,
        destroy: Listener::none(),
        commit: Listener::none(),
    }));

    (*popup).destroy =
        Listener::new(&mut (*popup_surface).events.destroy, popup, handle_popup_surface_destroy);
    (*popup).commit = Listener::new(
        &mut (*(*popup_surface).surface).events.commit,
        popup,
        handle_popup_surface_commit,
    );

    (*popup).tree = wlr_scene_tree_create(g!().layers[LYR_IMPOPUP]);
    (*popup).scene_surface = wlr_scene_subsurface_tree_create((*popup).tree, (*popup_surface).surface);
    (*(*popup).scene_surface).node.data = popup.cast();

    (*relay).popups.insert(0, popup);

    update_popup_position(popup);
}

unsafe fn handle_new_input_method(relay: *mut InputMethodRelay, data: *mut c_void) {
    let input_method = data as *mut wlr_input_method_v2;
    if g!().seat != (*input_method).seat {
        return;
    }

    if !(*relay).input_method.is_null() {
        wlr_input_method_v2_send_unavailable(input_method);
        return;
    }

    (*relay).input_method = input_method;
    let events = &mut (*input_method).events;
    (*relay).input_method_commit = Listener::new(&mut events.commit, relay, handle_input_method_commit);
    (*relay).input_method_grab_keyboard =
        Listener::new(&mut events.grab_keyboard, relay, handle_input_method_grab_keyboard);
    (*relay).input_method_destroy = Listener::new(&mut events.destroy, relay, handle_input_method_destroy);
    (*relay).input_method_new_popup_surface =
        Listener::new(&mut events.new_popup_surface, relay, handle_input_method_new_popup_surface);

    update_text_inputs_focused_surface(relay);
    update_active_text_input(relay);
}

unsafe fn send_state_to_input_method(relay: *mut InputMethodRelay) {
    let input_method = (*relay).input_method;
    let input = (*(*relay).active_text_input).input;

    if ((*input).active_features & WLR_TEXT_INPUT_V3_FEATURE_SURROUNDING_TEXT) != 0 {
        wlr_input_method_v2_send_surrounding_text(
            input_method,
            (*input).current.surrounding.text,
            (*input).current.surrounding.cursor,
            (*input).current.surrounding.anchor,
        );
    }
    wlr_input_method_v2_send_text_change_cause(input_method, (*input).current.text_change_cause);
    if ((*input).active_features & WLR_TEXT_INPUT_V3_FEATURE_CONTENT_TYPE) != 0 {
        wlr_input_method_v2_send_content_type(
            input_method,
            (*input).current.content_type.hint,
            (*input).current.content_type.purpose,
        );
    }
    wlr_input_method_v2_send_done(input_method);
}

unsafe fn handle_text_input_enable(text_input: *mut TextInput, _: *mut c_void) {
    let relay = (*text_input).relay;
    update_active_text_input(relay);
    if (*relay).active_text_input == text_input {
        update_popups_position(relay);
        send_state_to_input_method(relay);
    }
    wlr_text_input_v3_send_done((*text_input).input);
}

unsafe fn handle_text_input_disable(text_input: *mut TextInput, _: *mut c_void) {
    update_active_text_input((*text_input).relay);
}

unsafe fn handle_text_input_commit(text_input: *mut TextInput, _: *mut c_void) {
    let relay = (*text_input).relay;
    if (*relay).active_text_input == text_input {
        update_popups_position(relay);
        send_state_to_input_method(relay);
    }
}

unsafe fn handle_text_input_destroy(text_input: *mut TextInput, _: *mut c_void) {
    let relay = (*text_input).relay;
    remove_ptr(&mut (*relay).text_inputs, text_input);
    if (*relay).active_text_input == text_input {
        (*relay).active_text_input = null_mut();
    }
    drop(Box::from_raw(text_input));
    update_active_text_input(relay);
}

unsafe fn handle_new_text_input(relay: *mut InputMethodRelay, data: *mut c_void) {
    let wlr_text_input = data as *mut wlr_text_input_v3;
    if g!().seat != (*wlr_text_input).seat {
        return;
    }

    let text_input = Box::into_raw(Box::new(TextInput {
        relay,
        input: wlr_text_input,
        enable: Listener::none(),
        commit: Listener::none(),
        disable: Listener::none(),
        destroy: Listener::none(),
    }));
    (*relay).text_inputs.insert(0, text_input);

    let events = &mut (*wlr_text_input).events;
    (*text_input).enable = Listener::new(&mut events.enable, text_input, handle_text_input_enable);
    (*text_input).disable = Listener::new(&mut events.disable, text_input, handle_text_input_disable);
    (*text_input).commit = Listener::new(&mut events.commit, text_input, handle_text_input_commit);
    (*text_input).destroy = Listener::new(&mut events.destroy, text_input, handle_text_input_destroy);

    update_text_inputs_focused_surface(relay);
}

unsafe fn handle_focused_surface_destroy(relay: *mut InputMethodRelay, _: *mut c_void) {
    input_method_relay_set_focus(relay, null_mut());
}

pub unsafe fn input_method_relay_create() -> *mut InputMethodRelay {
    let relay = Box::into_raw(Box::new(InputMethodRelay {
        text_inputs: Vec::new(),
        input_method: null_mut(),
        focused_surface: null_mut(),
        active_text_input: null_mut(),
        popups: Vec::new(),
        popup_tree: wlr_scene_tree_create(&mut (*g!().scene).tree),
        new_text_input: Listener::none(),
        new_input_method: Listener::none(),
        input_method_commit: Listener::none(),
        input_method_grab_keyboard: Listener::none(),
        input_method_destroy: Listener::none(),
        input_method_new_popup_surface: Listener::none(),
        keyboard_grab_destroy: Listener::none(),
        focused_surface_destroy: Listener::none(),
    }));

    (*relay).new_text_input = Listener::new(
        &mut (*g!().text_input_manager).events.new_text_input,
        relay,
        handle_new_text_input,
    );
    (*relay).new_input_method = Listener::new(
        &mut (*g!().input_method_manager).events.new_input_method,
        relay,
        handle_new_input_method,
    );
    relay
}

pub unsafe fn input_method_relay_finish(relay: *mut InputMethodRelay) {
    if relay.is_null() {
        return;
    }
    // Text inputs, popups and the input method itself belong to clients that
    // were destroyed before us; just drop our bookkeeping.
    drop(Box::from_raw(relay));
}

pub unsafe fn input_method_relay_set_focus(relay: *mut InputMethodRelay, surface: *mut wlr_surface) {
    if relay.is_null() || (*relay).focused_surface == surface {
        return;
    }

    (*relay).focused_surface_destroy.remove();
    (*relay).focused_surface = surface;
    if !surface.is_null() {
        (*relay).focused_surface_destroy =
            Listener::new(&mut (*surface).events.destroy, relay, handle_focused_surface_destroy);
    }

    update_text_inputs_focused_surface(relay);
    update_active_text_input(relay);
}
