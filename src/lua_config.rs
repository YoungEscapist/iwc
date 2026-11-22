use crate::prelude::*;
use mlua::{Lua, Result, Table};
use std::path::PathBuf;

pub struct Settings {
    pub niri: bool,
    pub gaps: i32,
    pub column_width: f64,
    pub center_focused: bool,
    pub keys: Vec<Key>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            niri: false,
            gaps: 16,
            column_width: 0.5,
            center_focused: false,
            keys: keys(),
        }
    }
}

fn parse(source: &str) -> Result<Settings> {
    let lua = Lua::new();
    let table: Table = lua.load(source).set_name("iwc/config.lua").eval()?;
    let mut settings = Settings::default();
    let layout = table
        .get::<Option<String>>("layout")?
        .unwrap_or("master".into());
    settings.niri = match layout.as_str() {
        "niri" => true,
        "master" => false,
        _ => return Err(mlua::Error::external("layout must be niri or master")),
    };
    settings.gaps = table.get::<Option<i32>>("gaps")?.unwrap_or(settings.gaps);
    settings.column_width = table
        .get::<Option<f64>>("column_width")?
        .unwrap_or(settings.column_width);
    settings.center_focused = table
        .get::<Option<bool>>("center_focused")?
        .unwrap_or(false);
    if !(0..=256).contains(&settings.gaps) || !(0.1..=1.0).contains(&settings.column_width) {
        return Err(mlua::Error::external(
            "gaps must be 0..256, column_width 0.1..1",
        ));
    }
    if let Some(bindings) = table.get::<Option<Table>>("binds")? {
        for binding in bindings.pairs::<String, String>() {
            let (chord, command) = binding?;
            let mut parts: Vec<_> = chord.split('+').collect();
            let name = CString::new(parts.pop().unwrap()).map_err(mlua::Error::external)?;
            let keysym =
                unsafe { xkb_keysym_from_name(name.as_ptr(), XKB_KEYSYM_CASE_INSENSITIVE) };
            let mut modifiers = 0;
            for part in parts {
                modifiers |= match part {
                    "Super" => MODKEY,
                    "Shift" => WLR_MODIFIER_SHIFT,
                    "Ctrl" => WLR_MODIFIER_CTRL,
                    "Alt" => WLR_MODIFIER_ALT,
                    _ => return Err(mlua::Error::external(format!("unknown modifier: {part}"))),
                };
            }
            let action = crate::desktop::parse_command(&command)
                .filter(|_| keysym != XKB_KEY_NoSymbol)
                .ok_or_else(|| {
                    mlua::Error::external(format!("invalid binding: {chord} = {command}"))
                })?;
            settings
                .keys
                .retain(|key| key.modifiers != modifiers || key.keysym != keysym);
            settings.keys.push(Key {
                modifiers,
                keysym,
                action,
            });
        }
    }
    Ok(settings)
}

pub fn load() -> Result<Settings> {
    let explicit = std::env::var_os("IWC_CONFIG");
    let path = explicit.clone().map(PathBuf::from).unwrap_or_else(|| {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
            })
            .join("iwc/config.lua")
    });
    match std::fs::read_to_string(&path) {
        Ok(source) => parse(&source),
        Err(error) if explicit.is_none() && error.kind() == std::io::ErrorKind::NotFound => {
            Ok(Settings::default())
        }
        Err(error) => Err(mlua::Error::external(format!(
            "{}: {error}",
            path.display()
        ))),
    }
}

pub unsafe fn reload() {
    match load() {
        Ok(settings) => {
            crate::desktop::cancel_gestures();
            g!().settings = settings;
            for monitor in g!().mons.clone() {
                arrange(monitor);
            }
            printstatus();
        }
        Err(error) => eprintln!("iwc: Lua config: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lua_layout_and_bindings() {
        let settings = parse("local gap = 8; return {layout='master', gaps=gap*2, column_width=2/3, binds={['Super+t']='floating'}}").unwrap();
        assert!(!settings.niri);
        assert_eq!(settings.gaps, 16);
        assert_eq!(settings.column_width, 2.0 / 3.0);
        assert_eq!(
            settings
                .keys
                .iter()
                .filter(|key| key.modifiers == MODKEY && key.keysym == XKB_KEY_t)
                .count(),
            1
        );
        assert!(
            settings
                .keys
                .iter()
                .any(|key| key.keysym == XKB_KEY_t
                    && matches!(key.action, Action::SetLayout(Some(1))))
        );
        assert!(!Settings::default().niri);
        assert!(!parse("return {}").unwrap().niri);
        assert!(!parse(include_str!("../config.lua")).unwrap().niri);
        assert!(parse("return {layout='niri'}").unwrap().niri);
        for source in [
            "return {",
            "return false",
            "return {layout='bad'}",
            "return {gaps=-1}",
            "return {column_width=0/0}",
            "return {binds={['Hyper+t']='layout'}}",
            "return {binds={['Super+t']='move-left'}}",
        ] {
            assert!(parse(source).is_err(), "{source}");
        }
    }
}
