//! Built-in and user presets.

use std::fs;
use std::path::PathBuf;

use crate::config::ConfigError;

pub const BUILTIN_NAMES: &[&str] = &["minecraft", "trackmania", "small-bad", "desktop"];

const MINECRAFT: &str = include_str!("../presets/minecraft.toml");
const TRACKMANIA: &str = include_str!("../presets/trackmania.toml");
const SMALL_BAD: &str = include_str!("../presets/small-bad.toml");
const DESKTOP: &str = include_str!("../presets/desktop.toml");

/// Where a preset came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PresetSource {
    Builtin,
    User(PathBuf),
}

impl std::fmt::Display for PresetSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Builtin => f.write_str("built-in"),
            Self::User(path) => write!(f, "{}", path.display()),
        }
    }
}

/// A preset available to the CLI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresetEntry {
    pub name: String,
    pub source: PresetSource,
}

/// Directory holding user presets (`$XDG_CONFIG_HOME/tablet-driver/presets`).
pub fn user_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join("tablet-driver").join("presets"))
}

fn user_preset_path(name: &str) -> Option<PathBuf> {
    if name.is_empty() || name.contains('/') || name.contains('\\') || name == "." || name == ".." {
        return None;
    }
    user_dir().map(|dir| dir.join(format!("{name}.toml")))
}

/// Lists user presets first, then built-ins that are not shadowed by a user
/// preset of the same name.
pub fn list() -> Vec<PresetEntry> {
    let mut entries = Vec::new();

    if let Some(dir) = user_dir() {
        if let Ok(read_dir) = fs::read_dir(&dir) {
            let mut user: Vec<PresetEntry> = read_dir
                .flatten()
                .filter_map(|entry| {
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                        return None;
                    }
                    let name = path.file_stem()?.to_string_lossy().to_string();
                    Some(PresetEntry {
                        name,
                        source: PresetSource::User(path),
                    })
                })
                .collect();
            user.sort_by(|a, b| a.name.cmp(&b.name));
            entries.extend(user);
        }
    }

    for &name in BUILTIN_NAMES {
        let shadowed = entries.iter().any(|entry| entry.name == name);
        if !shadowed {
            entries.push(PresetEntry {
                name: name.to_string(),
                source: PresetSource::Builtin,
            });
        }
    }

    entries
}

/// Returns the raw TOML for a preset. User presets shadow built-ins.
pub fn load(name: &str) -> Result<String, ConfigError> {
    if let Some(path) = user_preset_path(name) {
        if path.is_file() {
            return fs::read_to_string(&path).map_err(|source| ConfigError::Io {
                path: path.clone(),
                source,
            });
        }
    }

    let text = match name {
        "minecraft" => MINECRAFT,
        "trackmania" => TRACKMANIA,
        "small-bad" => SMALL_BAD,
        "desktop" => DESKTOP,
        _ => return Err(ConfigError::UnknownPreset(name.to_string())),
    };
    Ok(text.to_string())
}

/// Loads a preset and parses it into a TOML value for merging.
pub fn load_value(name: &str) -> Result<toml::Value, ConfigError> {
    let text = load(name)?;
    toml::from_str(&text).map_err(ConfigError::Toml)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, Mode, MouseScheme};

    #[test]
    fn every_builtin_preset_parses_and_validates() {
        for &name in BUILTIN_NAMES {
            let value = load_value(name).unwrap();
            let config: Config = value.try_into().unwrap();
            config
                .validate()
                .unwrap_or_else(|err| panic!("preset {name} is invalid: {err}"));
        }
    }

    #[test]
    fn minecraft_preset_is_mouse_delta() {
        let value = load_value("minecraft").unwrap();
        let config: Config = value.try_into().unwrap();
        assert_eq!(config.general.mode, Mode::Mouse);
        assert_eq!(config.mouse.scheme, MouseScheme::Delta);
        assert!(config.general.grab_pen);
    }

    #[test]
    fn trackmania_preset_has_pedals_disabled_by_default() {
        let value = load_value("trackmania").unwrap();
        let config: Config = value.try_into().unwrap();
        assert_eq!(config.general.mode, Mode::Gamepad);
        assert!(!config.gamepad.enable_pedals);
        assert!(!config.general.grab_pen);
    }

    #[test]
    fn unknown_preset_is_rejected() {
        assert!(matches!(
            load_value("does-not-exist"),
            Err(ConfigError::UnknownPreset(_))
        ));
    }

    #[test]
    fn user_preset_paths_cannot_escape_the_directory() {
        assert!(user_preset_path("../evil").is_none());
        assert!(user_preset_path("").is_none());
    }
}
