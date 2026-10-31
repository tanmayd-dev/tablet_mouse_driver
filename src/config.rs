//! Configuration model, preset resolution, and CLI overrides.

use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use clap::ValueEnum;
use serde::{Deserialize, Serialize};

use crate::action::{Action, InputButton};
use crate::cli::RunArgs;
use crate::presets;

/// What the tablet motion is translated into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Relative mouse motion (camera look, cursor movement).
    #[default]
    Mouse,
    /// Analog Xbox 360 gamepad (steering and optional pedals).
    Gamepad,
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Mouse => "mouse",
            Self::Gamepad => "gamepad",
        })
    }
}

/// How tablet motion becomes mouse motion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum MouseScheme {
    /// Trackpad style: pen movement produces relative mouse deltas.
    #[default]
    Delta,
    /// Joystick style: pen deflection from centre produces a continuous rate.
    Rate,
}

impl fmt::Display for MouseScheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Delta => "delta",
            Self::Rate => "rate",
        })
    }
}

/// Which tablet axis drives steering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum SteeringAxis {
    #[default]
    X,
    Y,
}

impl fmt::Display for SteeringAxis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::X => "x",
            Self::Y => "y",
        })
    }
}

/// How tablet input drives throttle and brake when pedals are enabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum PedalScheme {
    /// Upper half of the tablet is throttle, lower half is brake.
    #[default]
    Split,
    /// Stylus pressure is throttle.
    Pressure,
    /// Vertical position drives the combined `ABS_Y` axis.
    Combined,
}

impl fmt::Display for PedalScheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Split => "split",
            Self::Pressure => "pressure",
            Self::Combined => "combined",
        })
    }
}

/// General process and device settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GeneralConfig {
    pub mode: Mode,
    /// Grab the pen node exclusively so raw absolute coordinates do not reach
    /// the compositor while a game holds relative pointer lock.
    pub grab_pen: bool,
    /// Grab the ExpressKey pad exclusively. Usually unnecessary.
    pub grab_pad: bool,
    /// Centre steering and release pedals when the pen leaves proximity.
    pub lift_centers: bool,
    /// Explicit pen device path. `None` auto-detects the best tablet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device: Option<PathBuf>,
    /// Explicit ExpressKey pad path. `None` auto-detects a companion pad.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pad_device: Option<PathBuf>,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            mode: Mode::Mouse,
            grab_pen: true,
            grab_pad: false,
            lift_centers: true,
            device: None,
            pad_device: None,
        }
    }
}

/// Relative-mouse translation settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MouseConfig {
    pub scheme: MouseScheme,
    /// Multiplier applied on top of `scale`.
    pub sensitivity: f64,
    /// Raw tablet counts to mouse counts. The Intuos M reports 100 counts/mm.
    pub scale: f64,
    pub invert_x: bool,
    pub invert_y: bool,
    /// Resting deadzone for rate mode, in `[0.0, 1.0)`.
    pub deadzone: f64,
    /// Full-deflection mouse counts per second for rate mode.
    pub rate_speed: f64,
    /// Delta scheme only: reject a single-sample raw coordinate jump larger
    /// than this. The kernel occasionally reports `(0, 0)` before the
    /// proximity-out event; such a spike would otherwise flick the camera.
    /// `0` disables the guard.
    pub max_jump: i32,
}

impl Default for MouseConfig {
    fn default() -> Self {
        Self {
            scheme: MouseScheme::Delta,
            sensitivity: 1.0,
            scale: 0.04,
            invert_x: false,
            invert_y: false,
            deadzone: 0.05,
            rate_speed: 1200.0,
            max_jump: 4000,
        }
    }
}

/// Gamepad steering and pedal settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GamepadConfig {
    pub steering_axis: SteeringAxis,
    /// Tablet position mapped to neutral, in `[0.0, 1.0]`.
    pub steering_center: f64,
    /// Distance from centre that reaches full lock, in `(0.0, 1.0]`.
    pub steering_half_span: f64,
    /// Steering deadzone in `[0.0, 1.0)`.
    pub steering_deadzone: f64,
    /// Response exponent. `1.0` is linear, `>1.0` gives finer centre control.
    pub steering_curve: f64,
    pub invert_steering: bool,
    /// Whether tablet input drives throttle/brake at all. Disabled by default
    /// so keyboard pedals can be used without interference.
    pub enable_pedals: bool,
    pub pedal_scheme: PedalScheme,
    pub pedal_deadzone: f64,
    pub pedal_curve: f64,
}

impl Default for GamepadConfig {
    fn default() -> Self {
        Self {
            steering_axis: SteeringAxis::X,
            steering_center: 0.5,
            steering_half_span: 0.5,
            steering_deadzone: 0.04,
            steering_curve: 1.3,
            invert_steering: false,
            enable_pedals: false,
            pedal_scheme: PedalScheme::Split,
            pedal_deadzone: 0.03,
            pedal_curve: 1.0,
        }
    }
}

/// Button bindings. Every field accepts any name understood by
/// [`Action::parse`], or `none` to disable the button.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ButtonConfig {
    pub pen_tip: String,
    pub pen_barrel_lower: String,
    pub pen_barrel_upper: String,
    pub pad_0: String,
    pub pad_1: String,
    pub pad_2: String,
    pub pad_3: String,
}

impl Default for ButtonConfig {
    fn default() -> Self {
        Self {
            pen_tip: "btn_left".to_string(),
            pen_barrel_lower: "btn_right".to_string(),
            pen_barrel_upper: "btn_middle".to_string(),
            pad_0: "wheel_up".to_string(),
            pad_1: "wheel_down".to_string(),
            pad_2: "none".to_string(),
            pad_3: "none".to_string(),
        }
    }
}

impl ButtonConfig {
    pub fn get(&self, button: InputButton) -> &str {
        match button {
            InputButton::PenTip => &self.pen_tip,
            InputButton::PenBarrelLower => &self.pen_barrel_lower,
            InputButton::PenBarrelUpper => &self.pen_barrel_upper,
            InputButton::Pad0 => &self.pad_0,
            InputButton::Pad1 => &self.pad_1,
            InputButton::Pad2 => &self.pad_2,
            InputButton::Pad3 => &self.pad_3,
        }
    }
}

/// Fully resolved configuration.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Preset this file inherits from. Only meaningful while loading.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    pub general: GeneralConfig,
    pub mouse: MouseConfig,
    pub gamepad: GamepadConfig,
    pub buttons: ButtonConfig,
}

/// Errors produced while loading or validating a configuration.
#[derive(Debug)]
pub enum ConfigError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Toml(toml::de::Error),
    Invalid(String),
    UnknownPreset(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(f, "cannot read {}: {source}", path.display())
            }
            Self::Toml(err) => write!(f, "invalid TOML: {err}"),
            Self::Invalid(msg) => f.write_str(msg),
            Self::UnknownPreset(name) => write!(
                f,
                "unknown preset '{name}' (run 'tablet-driver presets' to list presets)"
            ),
        }
    }
}

impl std::error::Error for ConfigError {}

impl From<toml::de::Error> for ConfigError {
    fn from(err: toml::de::Error) -> Self {
        Self::Toml(err)
    }
}

impl Config {
    /// Resolves a preset, overlays an optional config file, then applies CLI
    /// overrides. This is the single entry point used by `run` and `config`.
    pub fn resolve(args: &RunArgs) -> Result<Self, ConfigError> {
        let config_value = match &args.config {
            Some(path) => Some(load_toml(path)?),
            None => None,
        };

        // A config file may select its own base preset.
        let preset_name = args
            .preset
            .clone()
            .or_else(|| {
                config_value
                    .as_ref()
                    .and_then(|v| v.get("preset"))
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| "minecraft".to_string());

        let mut value = presets::load_value(&preset_name)?;
        if let Some(overlay) = config_value {
            merge_values(&mut value, overlay);
        }

        let mut config: Config = value.try_into()?;
        config.preset = Some(preset_name);
        config.apply_cli(args);
        config.validate()?;
        Ok(config)
    }

    /// Applies command-line overrides to an already resolved config.
    pub fn apply_cli(&mut self, args: &RunArgs) {
        if let Some(mode) = args.mode {
            self.general.mode = mode;
        }
        if let Some(scheme) = args.scheme {
            self.mouse.scheme = scheme;
        }
        if let Some(value) = args.sensitivity {
            self.mouse.sensitivity = value;
        }
        if let Some(value) = args.scale {
            self.mouse.scale = value;
        }
        if let Some(value) = args.max_jump {
            self.mouse.max_jump = value;
        }
        if args.invert_x {
            self.mouse.invert_x = true;
        }
        if args.invert_y {
            self.mouse.invert_y = true;
        }
        if args.grab {
            self.general.grab_pen = true;
        }
        if args.no_grab {
            self.general.grab_pen = false;
        }
        if args.grab_pad {
            self.general.grab_pad = true;
        }
        if args.no_grab_pad {
            self.general.grab_pad = false;
        }
        if args.pedals {
            self.gamepad.enable_pedals = true;
        }
        if args.no_pedals {
            self.gamepad.enable_pedals = false;
        }
        if let Some(scheme) = args.pedal_scheme {
            self.gamepad.pedal_scheme = scheme;
        }
        if let Some(axis) = args.steering_axis {
            self.gamepad.steering_axis = axis;
        }
        if let Some(value) = args.steering_curve {
            self.gamepad.steering_curve = value;
        }
        if let Some(value) = args.steering_deadzone {
            self.gamepad.steering_deadzone = value;
        }
        if let Some(value) = args.steering_center {
            self.gamepad.steering_center = value;
        }
        if let Some(value) = args.steering_half_span {
            self.gamepad.steering_half_span = value;
        }
        if args.invert_steering {
            self.gamepad.invert_steering = true;
        }
        if let Some(path) = &args.device {
            self.general.device = Some(path.clone());
        }
        if let Some(path) = &args.pad_device {
            self.general.pad_device = Some(path.clone());
        }
    }

    /// Parses and validates every button binding.
    pub fn button_map(&self) -> Result<HashMap<InputButton, Action>, ConfigError> {
        let mut map = HashMap::new();
        for button in InputButton::ALL {
            let raw = self.buttons.get(button);
            let action = Action::parse(raw).map_err(|err| {
                ConfigError::Invalid(format!("buttons.{}: {err}", button.field_name()))
            })?;
            map.insert(button, action);
        }
        Ok(map)
    }

    /// Validates numeric ranges and button names.
    pub fn validate(&self) -> Result<(), ConfigError> {
        let m = &self.mouse;
        if m.sensitivity <= 0.0 {
            return invalid("mouse.sensitivity must be > 0.0");
        }
        if m.scale <= 0.0 {
            return invalid("mouse.scale must be > 0.0");
        }
        if !(0.0..1.0).contains(&m.deadzone) {
            return invalid("mouse.deadzone must be in [0.0, 1.0)");
        }
        if m.rate_speed <= 0.0 {
            return invalid("mouse.rate_speed must be > 0.0");
        }
        if m.max_jump < 0 {
            return invalid("mouse.max_jump must be >= 0 (0 disables the guard)");
        }

        let g = &self.gamepad;
        if !(0.0..=1.0).contains(&g.steering_center) {
            return invalid("gamepad.steering_center must be in [0.0, 1.0]");
        }
        if g.steering_half_span <= 0.0 || g.steering_half_span > 1.0 {
            return invalid("gamepad.steering_half_span must be in (0.0, 1.0]");
        }
        if !(0.0..1.0).contains(&g.steering_deadzone) {
            return invalid("gamepad.steering_deadzone must be in [0.0, 1.0)");
        }
        if g.steering_curve <= 0.0 {
            return invalid("gamepad.steering_curve must be > 0.0");
        }
        if !(0.0..1.0).contains(&g.pedal_deadzone) {
            return invalid("gamepad.pedal_deadzone must be in [0.0, 1.0)");
        }
        if g.pedal_curve <= 0.0 {
            return invalid("gamepad.pedal_curve must be > 0.0");
        }

        let actions = self.button_map()?;
        for button in InputButton::ALL {
            // Pen events are ignored while passthrough is active, so a
            // pen-bound toggle could never be used to leave it. ExpressKeys
            // stay readable on the pad node.
            if actions[&button].is_passthrough() && !button.is_pad() {
                return invalid(&format!(
                    "buttons.{}: passthrough actions must be bound to an ExpressKey \
                     (pad_0..pad_3)",
                    button.field_name()
                ));
            }
        }
        Ok(())
    }
}

fn invalid<T>(message: &str) -> Result<T, ConfigError> {
    Err(ConfigError::Invalid(message.to_string()))
}

fn load_toml(path: &Path) -> Result<toml::Value, ConfigError> {
    let text = fs::read_to_string(path).map_err(|source| ConfigError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    toml::from_str(&text).map_err(ConfigError::Toml)
}

/// Recursively overlays `overlay` on `base`. Tables merge, scalars replace.
fn merge_values(base: &mut toml::Value, overlay: toml::Value) {
    match (base, overlay) {
        (toml::Value::Table(base_table), toml::Value::Table(overlay_table)) => {
            for (key, value) in overlay_table {
                match base_table.get_mut(&key) {
                    Some(existing) => merge_values(existing, value),
                    None => {
                        base_table.insert(key, value);
                    }
                }
            }
        }
        (base, overlay) => *base = overlay,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn args_from(argv: &[&str]) -> RunArgs {
        #[derive(Parser)]
        struct Wrapper {
            #[command(flatten)]
            run: RunArgs,
        }
        Wrapper::parse_from(argv).run
    }

    #[test]
    fn default_config_is_valid() {
        let config = Config::default();
        assert!(config.validate().is_ok());
        assert_eq!(config.general.mode, Mode::Mouse);
        assert!(!config.gamepad.enable_pedals);
    }

    #[test]
    fn rejects_bad_numeric_ranges() {
        let mut config = Config::default();
        config.mouse.sensitivity = 0.0;
        assert!(config.validate().is_err());

        let mut config = Config::default();
        config.gamepad.steering_half_span = 0.0;
        assert!(config.validate().is_err());

        let mut config = Config::default();
        config.gamepad.steering_curve = -1.0;
        assert!(config.validate().is_err());

        let mut config = Config::default();
        config.mouse.max_jump = -1;
        assert!(config.validate().is_err());
    }

    #[test]
    fn passthrough_actions_must_be_bound_to_the_pad() {
        let mut config = Config::default();
        config.buttons.pen_tip = "toggle_passthrough".to_string();
        let err = config.validate().unwrap_err().to_string();
        assert!(err.contains("buttons.pen_tip"));
        assert!(err.contains("pad_0..pad_3"));

        let mut config = Config::default();
        config.buttons.pad_2 = "toggle_passthrough".to_string();
        config.buttons.pad_3 = "hold_passthrough".to_string();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn rejects_unknown_action_name() {
        let mut config = Config::default();
        config.buttons.pen_tip = "key_space".to_string();
        let err = config.validate().unwrap_err().to_string();
        assert!(err.contains("buttons.pen_tip"));
        assert!(err.contains("unknown action"));
    }

    #[test]
    fn merges_nested_tables() {
        let mut base: toml::Value = toml::from_str(
            r#"
            [general]
            mode = "gamepad"
            grab_pen = true
            [gamepad]
            steering_curve = 1.0
            enable_pedals = false
            "#,
        )
        .unwrap();
        let overlay: toml::Value = toml::from_str(
            r#"
            [gamepad]
            enable_pedals = true
            "#,
        )
        .unwrap();
        merge_values(&mut base, overlay);
        let config: Config = base.try_into().unwrap();
        assert_eq!(config.general.mode, Mode::Gamepad);
        assert!(config.general.grab_pen);
        assert_eq!(config.gamepad.steering_curve, 1.0);
        assert!(config.gamepad.enable_pedals);
    }

    #[test]
    fn cli_overrides_win() {
        let args = args_from(&[
            "tablet-driver",
            "--mode",
            "gamepad",
            "--no-pedals",
            "--steering-curve",
            "2.0",
            "--sensitivity",
            "1.5",
            "--max-jump",
            "2500",
        ]);
        let mut config = Config::default();
        config.apply_cli(&args);
        assert_eq!(config.general.mode, Mode::Gamepad);
        assert!(!config.gamepad.enable_pedals);
        assert_eq!(config.gamepad.steering_curve, 2.0);
        assert_eq!(config.mouse.sensitivity, 1.5);
        assert_eq!(config.mouse.max_jump, 2500);
    }

    #[test]
    fn button_map_contains_all_inputs() {
        let config = Config::default();
        let map = config.button_map().unwrap();
        assert_eq!(map.len(), InputButton::ALL.len());
        assert_eq!(
            map.get(&InputButton::PenTip),
            Some(&Action::Mouse(crate::action::MouseButton::Left))
        );
    }
}
