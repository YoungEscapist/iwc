//! Types and global state of dwl.c.
//!
//! A wlroots compositor is one thread re-entering itself through signals
//! (focusing a client emits events whose handlers focus other things...).
//! Like dwl we keep one global state object, reachable through the raw
//! pointer [`G`].  It is always accessed as a place expression through that
//! pointer (`g!().field`), never as a long-lived `&mut`, so re-entrant calls
//! do not alias references.

use crate::client::Client;
use crate::ffi::*;
use crate::listener::Listener;
use std::ptr::null_mut;

/* scene layers */
pub const LYR_BG: usize = 0;
pub const LYR_BOTTOM: usize = 1;
pub const LYR_TILE: usize = 2;
pub const LYR_FLOAT: usize = 3;
pub const LYR_TOP: usize = 4;
pub const LYR_FS: usize = 5;
pub const LYR_UNMANAGED: usize = 6;
pub const LYR_OVERLAY: usize = 7;
pub const LYR_IMPOPUP: usize = 8;
pub const LYR_BLOCK: usize = 9;
pub const NUM_LAYERS: usize = 10;

/// Map from ZWLR_LAYER_SHELL_* constants to LYR_*.
pub const LAYERMAP: [usize; 4] = [LYR_BG, LYR_BOTTOM, LYR_TOP, LYR_OVERLAY];

/* client types, as returned by toplevel_from_wlr_surface() */
pub const XDG_SHELL: i32 = 0;
pub const LAYER_SHELL: i32 = 1;
pub const X11: i32 = 2;

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum CursorMode {
    Normal,
    Pressed,
    Move,
    Resize,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum AxisDir {
    Up,
    Right,
    Down,
    Left,
}

/// dwl's `void *exclusive_focus`, which held either a Client or a LayerSurface.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Exclusive {
    None,
    Client(*mut Client),
    Layer(*mut LayerSurface),
}

pub struct KeyboardGroup {
    pub wlr_group: *mut wlr_keyboard_group,
    pub virtual_keyboard: *mut wlr_keyboard,
    pub keysyms: [xkb_keysym_t; 2],
    pub mods: u32,
    pub key_repeat_source: *mut wl_event_source,
    pub modifiers: Listener,
    pub key: Listener,
    pub destroy: Listener,
}

pub struct LayerSurface {
    pub mon: *mut Monitor,
    pub scene: *mut wlr_scene_tree,
    pub popups: *mut wlr_scene_tree,
    pub scene_layer: *mut wlr_scene_layer_surface_v1,
    /// Which `Monitor::layers` list this surface is linked into (dwl's `link`).
    pub list_mon: *mut Monitor,
    pub list_layer: usize,
    pub mapped: bool,
    pub layer_surface: *mut wlr_layer_surface_v1,
    pub destroy: Listener,
    pub unmap: Listener,
    pub surface_commit: Listener,
}

pub struct Monitor {
    pub wlr_output: *mut wlr_output,
    pub scene_output: *mut wlr_scene_output,
    pub fullscreen_bg: *mut wlr_scene_rect, // see createmon() for info
    pub frame: Listener,
    pub destroy: Listener,
    pub request_state: Listener,
    pub destroy_lock_surface: Listener,
    pub lock_surface: *mut wlr_session_lock_surface_v1,
    pub m: wlr_box, // monitor area, layout-relative
    pub w: wlr_box, // window area, layout-relative
    /// LayerSurfaces per zwlr layer, newest first (like wl_list_insert at head).
    pub layers: [Vec<*mut LayerSurface>; 4],
    pub workspaces: [crate::workspace::Workspace; crate::config::TAGCOUNT as usize],
    pub seltags: usize,
    pub tagset: [u32; 2],
    pub mfact: f32,
    pub gamma_lut_changed: bool,
    pub nmaster: i32,
    pub ltsymbol: String,
    pub asleep: bool,
}

pub struct PointerConstraint {
    pub constraint: *mut wlr_pointer_constraint_v1,
    pub destroy: Listener,
}

pub struct SessionLock {
    pub scene: *mut wlr_scene_tree,
    pub lock: *mut wlr_session_lock_v1,
    pub new_surface: Listener,
    pub unlock: Listener,
    pub destroy: Listener,
}

/// An xdg_popup waiting for its initial commit.  dwl used a self-freeing
/// listener for this; we also watch the popup's destroy so a popup that dies
/// before its first commit does not leave a dangling listener behind.
pub struct Popup {
    pub commit: Listener,
    pub destroy: Listener,
}

pub struct Dwl {
    pub canvas: crate::infinity::Canvas,
    pub desktop: Option<crate::desktop::Desktop>,
    pub child_pid: libc::pid_t,
    pub locked: bool,
    pub exclusive_focus: Exclusive,
    pub focused_client: *mut Client,
    pub dpy: *mut wl_display,
    pub event_loop: *mut wl_event_loop,
    pub backend: *mut wlr_backend,
    pub scene: *mut wlr_scene,
    pub layers: [*mut wlr_scene_tree; NUM_LAYERS],
    pub drag_icon: *mut wlr_scene_tree,
    pub drw: *mut wlr_renderer,
    pub alloc: *mut wlr_allocator,
    pub compositor: *mut wlr_compositor,
    pub session: *mut wlr_session,

    pub xdg_shell: *mut wlr_xdg_shell,
    pub activation: *mut wlr_xdg_activation_v1,
    pub xdg_decoration_mgr: *mut wlr_xdg_decoration_manager_v1,
    pub clients: Vec<*mut Client>, // tiling order
    pub fstack: Vec<*mut Client>,  // focus order
    pub idle_notifier: *mut wlr_idle_notifier_v1,
    pub idle_inhibit_mgr: *mut wlr_idle_inhibit_manager_v1,
    pub layer_shell: *mut wlr_layer_shell_v1,
    pub output_mgr: *mut wlr_output_manager_v1,
    pub virtual_keyboard_mgr: *mut wlr_virtual_keyboard_manager_v1,
    pub virtual_pointer_mgr: *mut wlr_virtual_pointer_manager_v1,
    pub cursor_shape_mgr: *mut wlr_cursor_shape_manager_v1,
    pub power_mgr: *mut wlr_output_power_manager_v1,
    pub foreign_toplevel_list: *mut wlr_ext_foreign_toplevel_list_v1,
    pub foreign_toplevel_capture_mgr:
        *mut wlr_ext_foreign_toplevel_image_capture_source_manager_v1,

    pub pointer_constraints: *mut wlr_pointer_constraints_v1,
    pub relative_pointer_mgr: *mut wlr_relative_pointer_manager_v1,
    pub active_constraint: *mut wlr_pointer_constraint_v1,

    pub cursor: *mut wlr_cursor,
    pub cursor_mgr: *mut wlr_xcursor_manager,

    pub root_bg: *mut wlr_scene_rect,
    pub session_lock_mgr: *mut wlr_session_lock_manager_v1,
    pub locked_bg: *mut wlr_scene_rect,
    pub cur_lock: *mut wlr_session_lock_v1,

    pub seat: *mut wlr_seat,
    pub kb_group: *mut KeyboardGroup,
    pub cursor_mode: CursorMode,
    pub grabc: *mut Client,
    pub grabcx: i32, // client-relative
    pub grabcy: i32,

    pub output_layout: *mut wlr_output_layout,
    pub sgeom: wlr_box,
    pub mons: Vec<*mut Monitor>, // newest first, like dwl's wl_list
    pub selmon: *mut Monitor,
    pub consumed: Vec<bool>, // KEY_MAX + 1 entries

    pub settings: crate::lua_config::Settings,

    /// Global event handlers (dwl's static wl_listeners).
    pub listeners: Vec<Listener>,
    pub gpu_reset: Listener,

    pub input_method_manager: *mut wlr_input_method_manager_v2,
    pub text_input_manager: *mut wlr_text_input_manager_v3,
    pub input_method_relay: *mut crate::ime::InputMethodRelay,

    #[cfg(feature = "xwayland")]
    pub xwayland: *mut wlr_xwayland,
}

impl Dwl {
    pub fn new() -> Self {
        Dwl {
            canvas: crate::infinity::Canvas::default(),
            desktop: None,
            child_pid: -1,
            locked: false,
            exclusive_focus: Exclusive::None,
            focused_client: null_mut(),
            dpy: null_mut(),
            event_loop: null_mut(),
            backend: null_mut(),
            scene: null_mut(),
            layers: [null_mut(); NUM_LAYERS],
            drag_icon: null_mut(),
            drw: null_mut(),
            alloc: null_mut(),
            compositor: null_mut(),
            session: null_mut(),
            xdg_shell: null_mut(),
            activation: null_mut(),
            xdg_decoration_mgr: null_mut(),
            clients: Vec::new(),
            fstack: Vec::new(),
            idle_notifier: null_mut(),
            idle_inhibit_mgr: null_mut(),
            layer_shell: null_mut(),
            output_mgr: null_mut(),
            virtual_keyboard_mgr: null_mut(),
            virtual_pointer_mgr: null_mut(),
            cursor_shape_mgr: null_mut(),
            power_mgr: null_mut(),
            foreign_toplevel_list: null_mut(),
            foreign_toplevel_capture_mgr: null_mut(),
            pointer_constraints: null_mut(),
            relative_pointer_mgr: null_mut(),
            active_constraint: null_mut(),
            cursor: null_mut(),
            cursor_mgr: null_mut(),
            root_bg: null_mut(),
            session_lock_mgr: null_mut(),
            locked_bg: null_mut(),
            cur_lock: null_mut(),
            seat: null_mut(),
            kb_group: null_mut(),
            cursor_mode: CursorMode::Normal,
            grabc: null_mut(),
            grabcx: 0,
            grabcy: 0,
            output_layout: null_mut(),
            sgeom: wlr_box::default(),
            mons: Vec::new(),
            selmon: null_mut(),
            consumed: vec![false; KEY_MAX as usize + 1],
            settings: Default::default(),
            listeners: Vec::new(),
            gpu_reset: Listener::none(),
            input_method_manager: null_mut(),
            text_input_manager: null_mut(),
            input_method_relay: null_mut(),
            #[cfg(feature = "xwayland")]
            xwayland: null_mut(),
        }
    }
}

pub static mut G: *mut Dwl = null_mut();

/// The global state as a place expression: `g!().selmon`.
#[macro_export]
macro_rules! g {
    () => {
        (*$crate::state::G)
    };
}

pub fn remove_ptr<T>(v: &mut Vec<*mut T>, p: *mut T) {
    if let Some(i) = v.iter().position(|&x| x == p) {
        v.remove(i);
    }
}
