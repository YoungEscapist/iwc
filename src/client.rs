//! Port of client.h: everything that differs between xdg-shell and Xwayland
//! clients, kept in one place.

use crate::prelude::*;

pub struct Client {
    pub kind: i32, // XDG_SHELL or X11
    pub mon: *mut Monitor,
    pub scene: *mut wlr_scene_tree,
    pub border: [*mut wlr_scene_rect; 4], // top, bottom, left, right
    pub scene_surface: *mut wlr_scene_tree,
    pub geom: wlr_box,   // layout-relative, includes border
    pub prev: wlr_box,   // layout-relative, includes border
    pub floating_geom: wlr_box,
    pub bounds: wlr_box, // only width and height are used
    pub xdg: *mut wlr_xdg_surface,
    #[cfg(feature = "xwayland")]
    pub xwayland: *mut wlr_xwayland_surface,
    pub decoration: *mut wlr_xdg_toplevel_decoration_v1,
    pub foreign_toplevel_handle: *mut wlr_ext_foreign_toplevel_handle_v1,
    pub image_capture_scene: *mut wlr_scene,
    pub image_capture_source: *mut wlr_ext_image_capture_source_v1,
    pub image_capture_tree: *mut wlr_scene_tree,
    pub image_capture_scene_surface: *mut wlr_scene_surface,
    pub commit: Listener,
    pub map: Listener,
    pub maximize: Listener,
    pub unmap: Listener,
    pub destroy: Listener,
    pub set_title: Listener,
    pub fullscreen: Listener,
    pub set_decoration_mode: Listener,
    pub destroy_decoration: Listener,
    #[cfg(feature = "xwayland")]
    pub activate: Listener,
    #[cfg(feature = "xwayland")]
    pub associate: Listener,
    #[cfg(feature = "xwayland")]
    pub dissociate: Listener,
    #[cfg(feature = "xwayland")]
    pub configure: Listener,
    #[cfg(feature = "xwayland")]
    pub set_geometry: Listener,
    #[cfg(feature = "xwayland")]
    pub set_hints: Listener,
    pub bw: i32,
    pub tags: u32,
    pub isfloating: bool,
    pub float_pinned: bool,
    pub isurgent: bool,
    pub isfullscreen: bool,
}

impl Client {
    pub fn new(kind: i32) -> Self {
        Client {
            kind,
            mon: null_mut(),
            scene: null_mut(),
            border: [null_mut(); 4],
            scene_surface: null_mut(),
            geom: wlr_box::default(),
            prev: wlr_box::default(),
            floating_geom: wlr_box::default(),
            bounds: wlr_box::default(),
            xdg: null_mut(),
            #[cfg(feature = "xwayland")]
            xwayland: null_mut(),
            decoration: null_mut(),
            foreign_toplevel_handle: null_mut(),
            image_capture_scene: null_mut(),
            image_capture_source: null_mut(),
            image_capture_tree: null_mut(),
            image_capture_scene_surface: null_mut(),
            commit: Listener::none(),
            map: Listener::none(),
            maximize: Listener::none(),
            unmap: Listener::none(),
            destroy: Listener::none(),
            set_title: Listener::none(),
            fullscreen: Listener::none(),
            set_decoration_mode: Listener::none(),
            destroy_decoration: Listener::none(),
            #[cfg(feature = "xwayland")]
            activate: Listener::none(),
            #[cfg(feature = "xwayland")]
            associate: Listener::none(),
            #[cfg(feature = "xwayland")]
            dissociate: Listener::none(),
            #[cfg(feature = "xwayland")]
            configure: Listener::none(),
            #[cfg(feature = "xwayland")]
            set_geometry: Listener::none(),
            #[cfg(feature = "xwayland")]
            set_hints: Listener::none(),
            bw: 0,
            tags: 0,
            isfloating: false,
            float_pinned: false,
            isurgent: false,
            isfullscreen: false,
        }
    }
}

const XDG_TOPLEVEL_CONFIGURE_BOUNDS_SINCE_VERSION: i32 = 4;
pub const XDG_TOPLEVEL_WM_CAPABILITIES_SINCE_VERSION: i32 = 5;

/* Leave these functions first; they're used in the others */
#[inline]
pub unsafe fn client_is_x11(c: *mut Client) -> bool {
    (*c).kind == X11
}

#[inline]
pub unsafe fn xdg_toplevel(c: *mut Client) -> *mut wlr_xdg_toplevel {
    (*(*c).xdg).__bindgen_anon_1.toplevel
}

/// Returns (max, min).
pub unsafe fn client_get_size_hints(c: *mut Client) -> (wlr_box, wlr_box) {
    let mut max = wlr_box::default();
    let mut min = wlr_box::default();
    #[cfg(feature = "xwayland")]
    if client_is_x11(c) {
        let size_hints = (*(*c).xwayland).size_hints;
        if !size_hints.is_null() {
            max.width = (*size_hints).max_width;
            max.height = (*size_hints).max_height;
            min.width = (*size_hints).min_width;
            min.height = (*size_hints).min_height;
        }
        return (max, min);
    }
    let state = &(*xdg_toplevel(c)).current;
    max.width = state.max_width;
    max.height = state.max_height;
    min.width = state.min_width;
    min.height = state.min_height;
    (max, min)
}

pub unsafe fn client_surface(c: *mut Client) -> *mut wlr_surface {
    #[cfg(feature = "xwayland")]
    if client_is_x11(c) {
        return (*(*c).xwayland).surface;
    }
    (*(*c).xdg).surface
}

pub unsafe fn client_mapped(c: *mut Client) -> bool {
    let s = client_surface(c);
    !s.is_null() && (*s).mapped
}

/// Returns (type, client, layer surface); type is -1 when `s` belongs to
/// neither.
pub unsafe fn toplevel_from_wlr_surface(
    s: *mut wlr_surface,
) -> (i32, *mut Client, *mut LayerSurface) {
    if s.is_null() {
        return (-1, null_mut(), null_mut());
    }
    let root_surface = wlr_surface_get_root_surface(s);

    #[cfg(feature = "xwayland")]
    {
        let xsurface = wlr_xwayland_surface_try_from_wlr_surface(root_surface);
        if !xsurface.is_null() {
            let c = (*xsurface).data as *mut Client;
            if c.is_null() {
                return (-1, null_mut(), null_mut());
            }
            return ((*c).kind, c, null_mut());
        }
    }

    let layer_surface = wlr_layer_surface_v1_try_from_wlr_surface(root_surface);
    if !layer_surface.is_null() {
        let l = (*layer_surface).data as *mut LayerSurface;
        if l.is_null() {
            return (-1, null_mut(), null_mut());
        }
        return (LAYER_SHELL, null_mut(), l);
    }

    let mut xdg_surface = wlr_xdg_surface_try_from_wlr_surface(root_surface);
    while !xdg_surface.is_null() {
        match (*xdg_surface).role {
            WLR_XDG_SURFACE_ROLE_POPUP => {
                let popup = (*xdg_surface).__bindgen_anon_1.popup;
                if popup.is_null() || (*popup).parent.is_null() {
                    return (-1, null_mut(), null_mut());
                }
                let tmp = wlr_xdg_surface_try_from_wlr_surface((*popup).parent);
                if tmp.is_null() {
                    return toplevel_from_wlr_surface((*popup).parent);
                }
                xdg_surface = tmp;
            }
            WLR_XDG_SURFACE_ROLE_TOPLEVEL => {
                let c = (*xdg_surface).data as *mut Client;
                if c.is_null() {
                    return (-1, null_mut(), null_mut());
                }
                return ((*c).kind, c, null_mut());
            }
            _ => return (-1, null_mut(), null_mut()),
        }
    }
    (-1, null_mut(), null_mut())
}

/* The others */
pub unsafe fn client_activate_surface(s: *mut wlr_surface, activated: bool) {
    #[cfg(feature = "xwayland")]
    {
        let xsurface = wlr_xwayland_surface_try_from_wlr_surface(s);
        if !xsurface.is_null() {
            wlr_xwayland_surface_activate(xsurface, activated);
            return;
        }
    }
    let toplevel = wlr_xdg_toplevel_try_from_wlr_surface(s);
    if !toplevel.is_null() {
        wlr_xdg_toplevel_set_activated(toplevel, activated);
    }
}

pub unsafe fn client_set_bounds(c: *mut Client, width: i32, height: i32) -> u32 {
    if client_is_x11(c) {
        return 0;
    }
    let toplevel = xdg_toplevel(c);
    if wl_resource_get_version((*toplevel).resource) >= XDG_TOPLEVEL_CONFIGURE_BOUNDS_SINCE_VERSION
        && width >= 0
        && height >= 0
        && ((*c).bounds.width != width || (*c).bounds.height != height)
    {
        (*c).bounds.width = width;
        (*c).bounds.height = height;
        return wlr_xdg_toplevel_set_bounds(toplevel, width, height);
    }
    0
}

const BROKEN: &CStr = c"broken";

pub unsafe fn client_get_appid_ptr(c: *mut Client) -> *const c_char {
    #[cfg(feature = "xwayland")]
    if client_is_x11(c) {
        let class = (*(*c).xwayland).class;
        return if class.is_null() { BROKEN.as_ptr() } else { class };
    }
    let app_id = (*xdg_toplevel(c)).app_id;
    if app_id.is_null() {
        BROKEN.as_ptr()
    } else {
        app_id
    }
}

pub unsafe fn client_get_title_ptr(c: *mut Client) -> *const c_char {
    #[cfg(feature = "xwayland")]
    if client_is_x11(c) {
        let title = (*(*c).xwayland).title;
        return if title.is_null() { BROKEN.as_ptr() } else { title };
    }
    let title = (*xdg_toplevel(c)).title;
    if title.is_null() {
        BROKEN.as_ptr()
    } else {
        title
    }
}

pub unsafe fn client_get_appid(c: *mut Client) -> String {
    cstr(client_get_appid_ptr(c)).into_owned()
}

pub unsafe fn client_get_title(c: *mut Client) -> String {
    cstr(client_get_title_ptr(c)).into_owned()
}

pub unsafe fn client_get_clip(c: *mut Client) -> wlr_box {
    let mut clip = wlr_box(0, 0, (*c).geom.width - (*c).bw, (*c).geom.height - (*c).bw);
    if client_is_x11(c) {
        return clip;
    }
    clip.x = (*(*c).xdg).geometry.x;
    clip.y = (*(*c).xdg).geometry.y;
    clip
}

pub unsafe fn client_get_geometry(c: *mut Client) -> wlr_box {
    #[cfg(feature = "xwayland")]
    if client_is_x11(c) {
        let x = (*c).xwayland;
        return wlr_box((*x).x as i32, (*x).y as i32, (*x).width as i32, (*x).height as i32);
    }
    (*(*c).xdg).geometry
}

pub unsafe fn client_get_parent(c: *mut Client) -> *mut Client {
    #[cfg(feature = "xwayland")]
    if client_is_x11(c) {
        let parent = (*(*c).xwayland).parent;
        if !parent.is_null() {
            return toplevel_from_wlr_surface((*parent).surface).1;
        }
        return null_mut();
    }
    let parent = (*xdg_toplevel(c)).parent;
    if !parent.is_null() {
        return toplevel_from_wlr_surface((*(*parent).base).surface).1;
    }
    null_mut()
}

pub unsafe fn client_has_children(c: *mut Client) -> bool {
    #[cfg(feature = "xwayland")]
    if client_is_x11(c) {
        return !wl_list_is_empty(&(*(*c).xwayland).children);
    }
    // surface.xdg->link is never empty because it always contains at least
    // the surface itself.
    wl_list_length(&(*(*c).xdg).link) > 1
}

pub unsafe fn client_is_float_type(c: *mut Client) -> bool {
    let (max, min) = client_get_size_hints(c);
    let fixed = min.width > 0
        && min.height > 0
        && (min.width == max.width || min.height == max.height);

    #[cfg(feature = "xwayland")]
    if client_is_x11(c) {
        let surface = (*c).xwayland;
        if (*surface).modal {
            return true;
        }
        for t in [
            WLR_XWAYLAND_NET_WM_WINDOW_TYPE_DIALOG,
            WLR_XWAYLAND_NET_WM_WINDOW_TYPE_SPLASH,
            WLR_XWAYLAND_NET_WM_WINDOW_TYPE_TOOLBAR,
            WLR_XWAYLAND_NET_WM_WINDOW_TYPE_UTILITY,
        ] {
            if wlr_xwayland_surface_has_window_type(surface, t) {
                return true;
            }
        }
        return fixed;
    }

    !(*xdg_toplevel(c)).parent.is_null() || fixed
}

pub unsafe fn client_is_unmanaged(c: *mut Client) -> bool {
    #[cfg(feature = "xwayland")]
    if client_is_x11(c) {
        return (*(*c).xwayland).override_redirect;
    }
    let _ = c;
    false
}

pub unsafe fn client_notify_enter(s: *mut wlr_surface, kb: *mut wlr_keyboard) {
    let seat = g!().seat;
    if kb.is_null() {
        wlr_seat_keyboard_notify_enter(seat, s, null_mut(), 0, null_mut());
        return;
    }
    let mut filtered = [0u32; WLR_KEYBOARD_KEYS_CAP as usize];
    let mut size = 0usize;
    for i in 0..(*kb).num_keycodes {
        let key = (*kb).keycodes[i];
        if !(&g!().consumed).get(key as usize).copied().unwrap_or(false) {
            filtered[size] = key;
            size += 1;
        }
    }
    wlr_seat_keyboard_notify_enter(seat, s, filtered.as_mut_ptr(), size, &mut (*kb).modifiers);
}

pub unsafe fn client_send_close(c: *mut Client) {
    #[cfg(feature = "xwayland")]
    if client_is_x11(c) {
        wlr_xwayland_surface_close((*c).xwayland);
        return;
    }
    wlr_xdg_toplevel_send_close(xdg_toplevel(c));
}

pub unsafe fn client_set_border_color(c: *mut Client, color: &[f32; 4]) {
    for b in (*c).border {
        if !b.is_null() {
            wlr_scene_rect_set_color(b, color.as_ptr());
        }
    }
}

pub unsafe fn client_set_fullscreen(c: *mut Client, fullscreen: bool) {
    #[cfg(feature = "xwayland")]
    if client_is_x11(c) {
        wlr_xwayland_surface_set_fullscreen((*c).xwayland, fullscreen);
        return;
    }
    wlr_xdg_toplevel_set_fullscreen(xdg_toplevel(c), fullscreen);
}

pub unsafe fn client_set_scale(s: *mut wlr_surface, scale: f32) {
    wlr_fractional_scale_v1_notify_scale(s, scale as f64);
    wlr_surface_set_preferred_buffer_scale(s, scale.ceil() as i32);
}

pub unsafe fn client_set_size(c: *mut Client, width: u32, height: u32) {
    #[cfg(feature = "xwayland")]
    if client_is_x11(c) {
        wlr_xwayland_surface_configure(
            (*c).xwayland,
            ((*c).geom.x + (*c).bw) as i16,
            ((*c).geom.y + (*c).bw) as i16,
            width as u16,
            height as u16,
        );
        return;
    }
    let toplevel = xdg_toplevel(c);
    if width as i32 != (*toplevel).scheduled.width || height as i32 != (*toplevel).scheduled.height {
        wlr_xdg_toplevel_set_size(toplevel, width as i32, height as i32);
    }
}

pub unsafe fn client_set_tiled(c: *mut Client, edges: u32) {
    #[cfg(feature = "xwayland")]
    if client_is_x11(c) {
        wlr_xwayland_surface_set_maximized(
            (*c).xwayland,
            edges != WLR_EDGE_NONE,
            edges != WLR_EDGE_NONE,
        );
        return;
    }
    let toplevel = xdg_toplevel(c);
    if wl_resource_get_version((*toplevel).resource)
        >= XDG_TOPLEVEL_STATE_TILED_RIGHT_SINCE_VERSION as i32
    {
        wlr_xdg_toplevel_set_tiled(toplevel, edges);
    } else {
        wlr_xdg_toplevel_set_maximized(toplevel, edges != WLR_EDGE_NONE);
    }
}

pub unsafe fn client_set_suspended(c: *mut Client, suspended: bool) {
    if client_is_x11(c) {
        return;
    }
    wlr_xdg_toplevel_set_suspended(xdg_toplevel(c), suspended);
}

pub unsafe fn client_wants_focus(c: *mut Client) -> bool {
    #[cfg(feature = "xwayland")]
    {
        return client_is_unmanaged(c)
            && wlr_xwayland_surface_override_redirect_wants_focus((*c).xwayland)
            && wlr_xwayland_surface_icccm_input_model((*c).xwayland)
                != WLR_ICCCM_INPUT_MODEL_NONE;
    }
    #[cfg(not(feature = "xwayland"))]
    {
        let _ = c;
        false
    }
}

pub unsafe fn client_wants_fullscreen(c: *mut Client) -> bool {
    #[cfg(feature = "xwayland")]
    if client_is_x11(c) {
        return (*(*c).xwayland).fullscreen;
    }
    (*xdg_toplevel(c)).requested.fullscreen
}
