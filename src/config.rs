//! Built-in defaults and action types; user layouts and bindings are configured in Lua.

use crate::ffi::*;
use crate::state::{AxisDir, CursorMode, Monitor};

pub const fn color(hex: u32) -> [f32; 4] {
    [
        ((hex >> 24) & 0xff) as f32 / 255.0,
        ((hex >> 16) & 0xff) as f32 / 255.0,
        ((hex >> 8) & 0xff) as f32 / 255.0,
        (hex & 0xff) as f32 / 255.0,
    ]
}

/* appearance */
pub const SLOPPYFOCUS: bool = true; // focus follows mouse
/// true means idle inhibitors will disable idle tracking even if their surface isn't visible
pub const BYPASS_SURFACE_VISIBILITY: bool = false;
pub const BORDERPX: i32 = 2; // border pixel of windows
pub const TOUCHPAD_MOVE_GAIN: f64 = 2.5;
pub const TOUCHPAD_RESIZE_GAIN: f64 = 3.0;
pub const PINCH_IN_THRESHOLD: f64 = 0.85;
pub const PINCH_OUT_THRESHOLD: f64 = 1.15;
pub const ROOTCOLOR: [f32; 4] = color(0x000000ff);
pub const BORDERCOLOR: [f32; 4] = color(0x444444ff);
pub const FOCUSCOLOR: [f32; 4] = color(0xaaaaaaff);
pub const URGENTCOLOR: [f32; 4] = color(0xffffffff);
/// This conforms to the xdg-protocol. Set the alpha to zero to restore the old behavior
pub const FULLSCREEN_BG: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

/* tagging - TAGCOUNT must be no greater than 31 */
pub const TAGCOUNT: u32 = 9;

/* logging */
pub const LOG_LEVEL: wlr_log_importance = WLR_ERROR;

pub struct Rule {
    pub id: Option<&'static str>,
    pub title: Option<&'static str>,
    pub tags: u32,
    pub isfloating: bool,
    pub monitor: i32,
}

pub const RULES: &[Rule] = &[
    /* app_id, title, tags mask, isfloating, monitor */
    // как window{ app_id = "okno", float = true } в parallax
    Rule { id: Some("okno"), title: None, tags: 0, isfloating: true, monitor: -1 },
    // виджет TuiMus: своих виджетов у iwc нет, пусть хотя бы не ломает тайлинг
    Rule { id: Some("tuimus-widget"), title: None, tags: 0, isfloating: true, monitor: -1 },
];

pub struct Layout {
    pub symbol: &'static str,
    /// No arrange function means floating behavior.
    pub arrange: Option<unsafe fn(*mut Monitor)>,
}

pub const LAYOUTS: &[Layout] = &[
    Layout { symbol: "[]", arrange: Some(crate::workspace::tile) },
    Layout { symbol: "∞", arrange: None },
];

pub struct MonitorRule {
    pub name: Option<&'static str>,
    pub mfact: f32,
    pub nmaster: i32,
    pub scale: f32,
    pub rr: wl_output_transform,
    pub x: i32,
    pub y: i32,
}

/// (x=-1, y=-1) is reserved as an "autoconfigure" monitor position indicator.
/// WARNING: negative values other than (-1, -1) cause problems with Xwayland
/// clients due to https://gitlab.freedesktop.org/xorg/xserver/-/issues/899
/// The last rule (name None) is the default and must stay.
pub const MONRULES: &[MonitorRule] = &[
    // как monitor{} в parallax: DP-2 слева сверху, HDMI-A-1 под ним со сдвигом 320.
    // Режим (2560x1080@200) iwc не задаёт: берётся preferred mode монитора.
    MonitorRule { name: Some("DP-2"), mfact: 0.55, nmaster: 1, scale: 1.0,
                  rr: WL_OUTPUT_TRANSFORM_NORMAL, x: 0, y: 0 },
    MonitorRule { name: Some("HDMI-A-1"), mfact: 0.55, nmaster: 1, scale: 1.0,
                  rr: WL_OUTPUT_TRANSFORM_NORMAL, x: 320, y: 1080 },
    MonitorRule {
        name: None,
        mfact: 0.55,
        nmaster: 1,
        scale: 1.0,
        rr: WL_OUTPUT_TRANSFORM_NORMAL,
        x: -1,
        y: -1,
    },
];

/* keyboard */
pub struct XkbRules {
    pub rules: Option<&'static str>,
    pub model: Option<&'static str>,
    pub layout: Option<&'static str>,
    pub variant: Option<&'static str>,
    pub options: Option<&'static str>,
}

/// как xkb{} в parallax; Ctrl+Space (layout_next в parallax) — через xkb
pub const XKB_RULES: XkbRules = XkbRules {
    rules: None,
    model: None,
    layout: Some("us,ru"),
    variant: None,
    options: Some("grp:alt_shift_toggle,grp:ctrl_space_toggle"),
};

pub const REPEAT_RATE: i32 = 25;
pub const REPEAT_DELAY: i32 = 600;

/* Trackpad */
pub const TAP_TO_CLICK: bool = true;
pub const TAP_AND_DRAG: bool = true;
pub const DRAG_LOCK: bool = false;
pub const NATURAL_SCROLLING: bool = false;
pub const DISABLE_WHILE_TYPING: bool = true;
pub const LEFT_HANDED: bool = false;
pub const MIDDLE_BUTTON_EMULATION: bool = false;
/// LIBINPUT_CONFIG_SCROLL_{NO_SCROLL,2FG,EDGE,ON_BUTTON_DOWN}
pub const SCROLL_METHOD: libinput_config_scroll_method = LIBINPUT_CONFIG_SCROLL_2FG;
/// LIBINPUT_CONFIG_CLICK_METHOD_{NONE,BUTTON_AREAS,CLICKFINGER}
pub const CLICK_METHOD: libinput_config_click_method = LIBINPUT_CONFIG_CLICK_METHOD_CLICKFINGER;
/// LIBINPUT_CONFIG_SEND_EVENTS_{ENABLED,DISABLED,DISABLED_ON_EXTERNAL_MOUSE}
pub const SEND_EVENTS_MODE: u32 = LIBINPUT_CONFIG_SEND_EVENTS_ENABLED;
/// LIBINPUT_CONFIG_ACCEL_PROFILE_{FLAT,ADAPTIVE}
pub const ACCEL_PROFILE: libinput_config_accel_profile = LIBINPUT_CONFIG_ACCEL_PROFILE_ADAPTIVE;
pub const ACCEL_SPEED: f64 = 0.0;
/// LIBINPUT_CONFIG_TAP_MAP_LRM -- 1/2/3 finger tap maps to left/right/middle
/// LIBINPUT_CONFIG_TAP_MAP_LMR -- 1/2/3 finger tap maps to left/middle/right
pub const BUTTON_MAP: libinput_config_tap_button_map = LIBINPUT_CONFIG_TAP_MAP_LRM;

/// Everything a key, button or axis binding can do (dwl's function + Arg).
#[derive(Copy, Clone, Debug)]
pub enum Action {
    Spawn(&'static [&'static str]),
    FocusStack(i32),
    FocusDirection(i32, i32),
    Pan(i32, i32),
    CanvasZoom(i32),
    CenterWindow,
    IncNMaster(i32),
    /// < 1.0 adds to mfact, > 1.0 sets it to `value - 1.0`
    SetMFact(f32),
    Zoom,
    View(u32),
    StepTag(i32),
    ToggleView(u32),
    Tag(u32),
    ToggleTag(u32),
    KillClient,
    /// None toggles tiling/floating for the current workspace.
    SetLayout(Option<usize>),
    CycleLayout,
    ReloadConfig,
    ToggleFloating,
    ToggleFullscreen,
    FocusMon(wlr_direction),
    TagMon(wlr_direction),
    MoveResize(CursorMode),
    Chvt(u32),
    Quit,
}

pub struct Key {
    pub modifiers: u32,
    pub keysym: xkb_keysym_t,
    pub action: Action,
}

pub struct Button {
    pub modifiers: u32,
    pub button: u32,
    pub action: Action,
}

pub struct Axis {
    pub modifiers: u32,
    pub dir: AxisDir,
    pub action: Action,
}

/// Super, как в parallax
pub const MODKEY: u32 = WLR_MODIFIER_LOGO;
const SHIFT: u32 = WLR_MODIFIER_SHIFT;
const CTRL: u32 = WLR_MODIFIER_CTRL;
const ALT: u32 = WLR_MODIFIER_ALT;

/* commands */
pub const TERMCMD: &[&str] = &["ghostty"];
pub const MENUCMD: &[&str] = &["iwc-desktop", "launcher"];

/// Helper for spawning shell commands in the pre dwm-5.0 fashion.
#[macro_export]
macro_rules! shcmd {
    ($cmd:expr) => {
        $crate::config::Action::Spawn(&["/bin/sh", "-c", $cmd])
    };
}

pub fn keys() -> Vec<Key> {
    use Action::*;
    let k = |modifiers, keysym, action| Key { modifiers, keysym, action };
    let mut keys = vec![
        k(MODKEY | SHIFT, XKB_KEY_q, Quit),
        k(MODKEY, XKB_KEY_q, KillClient),
        k(MODKEY, XKB_KEY_Return, Spawn(TERMCMD)),
        k(0, XKB_KEY_F11, ToggleFullscreen),
        k(MODKEY, XKB_KEY_f, ToggleFullscreen),
        k(MODKEY, XKB_KEY_d, CycleLayout),
        k(MODKEY | SHIFT, XKB_KEY_d, SetLayout(Some(1))),
        k(MODKEY, XKB_KEY_t, SetLayout(Some(0))),
        k(MODKEY | SHIFT, XKB_KEY_r, ReloadConfig),
        k(MODKEY, XKB_KEY_Home, CenterWindow),
        k(MODKEY, XKB_KEY_Tab, FocusStack(1)),
        k(MODKEY | SHIFT, XKB_KEY_Tab, FocusStack(-1)),
        k(ALT, XKB_KEY_Tab, FocusStack(1)),
        k(ALT | SHIFT, XKB_KEY_Tab, FocusStack(-1)),
        k(MODKEY, XKB_KEY_h, FocusDirection(-1, 0)),
        k(MODKEY, XKB_KEY_l, FocusDirection(1, 0)),
        k(MODKEY, XKB_KEY_k, FocusDirection(0, -1)),
        k(MODKEY, XKB_KEY_j, FocusDirection(0, 1)),
        k(MODKEY, XKB_KEY_Left, FocusDirection(-1, 0)),
        k(MODKEY, XKB_KEY_Right, FocusDirection(1, 0)),
        k(MODKEY, XKB_KEY_Up, FocusDirection(0, -1)),
        k(MODKEY, XKB_KEY_Down, FocusDirection(0, 1)),
        k(MODKEY | CTRL, XKB_KEY_Left, Pan(-1, 0)),
        k(MODKEY | CTRL, XKB_KEY_Right, Pan(1, 0)),
        k(MODKEY | CTRL, XKB_KEY_Up, Pan(0, -1)),
        k(MODKEY | CTRL, XKB_KEY_Down, Pan(0, 1)),
        k(MODKEY, XKB_KEY_equal, CanvasZoom(1)),
        k(MODKEY | SHIFT, XKB_KEY_plus, CanvasZoom(1)),
        k(MODKEY, XKB_KEY_minus, CanvasZoom(-1)),
        k(MODKEY, XKB_KEY_0, CanvasZoom(0)),
        k(MODKEY, XKB_KEY_Next, FocusMon(WLR_DIRECTION_RIGHT)),
        k(MODKEY, XKB_KEY_Prior, FocusMon(WLR_DIRECTION_LEFT)),
        k(MODKEY | CTRL, XKB_KEY_Next, TagMon(WLR_DIRECTION_RIGHT)),
        k(MODKEY | CTRL, XKB_KEY_Prior, TagMon(WLR_DIRECTION_LEFT)),
        // программы
        k(MODKEY, XKB_KEY_space, Spawn(MENUCMD)),
        k(0, XKB_KEY_XF86AudioRaiseVolume, shcmd!("wpctl set-volume -l 1 @DEFAULT_AUDIO_SINK@ 5%+")),
        k(0, XKB_KEY_XF86AudioLowerVolume, shcmd!("wpctl set-volume @DEFAULT_AUDIO_SINK@ 5%-")),
        k(0, XKB_KEY_XF86AudioMute, shcmd!("wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle")),
        k(0, XKB_KEY_XF86AudioMicMute, shcmd!("wpctl set-mute @DEFAULT_AUDIO_SOURCE@ toggle")),
        k(MODKEY, XKB_KEY_s, Spawn(MENUCMD)),
        k(MODKEY, XKB_KEY_w, shcmd!("iwc-desktop wallpaper-picker")),
        k(MODKEY | SHIFT, XKB_KEY_w, shcmd!("iwc-desktop wallpaper-next")),
        k(MODKEY, XKB_KEY_c, shcmd!(
            "pkill -x okno || cliphist list | okno -dmenu -p '' -display-columns 2 | cliphist decode | wl-copy"
        )),
        k(0, XKB_KEY_Print, shcmd!(
            "mkdir -p ~/Pictures/Screenshots && grim -g \"$(slurp)\" - | tee ~/Pictures/Screenshots/$(date +%F_%H-%M-%S).png | wl-copy"
        )),
        k(0, XKB_KEY_XF86MonBrightnessUp, shcmd!("brightnessctl set 5%+")),
        k(0, XKB_KEY_XF86MonBrightnessDown, shcmd!("brightnessctl set 5%-")),
        k(0, XKB_KEY_XF86KbdBrightnessUp, shcmd!("brightnessctl -d '*kbd_backlight*' set 1+")),
        k(0, XKB_KEY_XF86KbdBrightnessDown, shcmd!("brightnessctl -d '*kbd_backlight*' set 1-")),
    ];
    // TAGKEYS
    for tag in 0..TAGCOUNT {
        let key = XKB_KEY_1 + tag;
        keys.push(k(MODKEY, key, View(1 << tag)));
        keys.push(k(MODKEY | CTRL, key, ToggleView(1 << tag)));
        keys.push(k(MODKEY | SHIFT, key, Tag(1 << tag)));
        keys.push(k(MODKEY | CTRL | SHIFT, key, ToggleTag(1 << tag)));
    }
    // Ctrl-Alt-Fx is used to switch to another VT, if you don't know what a VT
    // is do not remove them.
    for n in 1..=12 {
        keys.push(k(CTRL | ALT, XKB_KEY_F1 + n - 1, Chvt(n)));
    }
    keys
}

pub const BUTTONS: &[Button] = &[
    Button { modifiers: MODKEY, button: BTN_LEFT, action: Action::MoveResize(CursorMode::Move) },
    Button { modifiers: MODKEY, button: BTN_MIDDLE, action: Action::CenterWindow },
    Button { modifiers: MODKEY, button: BTN_RIGHT, action: Action::MoveResize(CursorMode::Resize) },
];

pub const AXES: &[Axis] = &[];
