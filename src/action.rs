//! Actions that a tablet button can emit.
//!
//! Only mouse and gamepad actions exist. Keyboard output is deliberately
//! absent so the driver never competes with a game's own key bindings.

use evdev::KeyCode;

/// A physical button on the tablet that can be bound to an [`Action`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InputButton {
    /// Pen tip (`BTN_TOUCH`).
    PenTip,
    /// Lower pen barrel button (`BTN_STYLUS`).
    PenBarrelLower,
    /// Upper pen barrel button (`BTN_STYLUS2`).
    PenBarrelUpper,
    /// ExpressKey 0 (`BTN_0`).
    Pad0,
    /// ExpressKey 1 (`BTN_1`).
    Pad1,
    /// ExpressKey 2 (`BTN_2`).
    Pad2,
    /// ExpressKey 3 (`BTN_3`).
    Pad3,
}

impl InputButton {
    pub const ALL: [Self; 7] = [
        Self::PenTip,
        Self::PenBarrelLower,
        Self::PenBarrelUpper,
        Self::Pad0,
        Self::Pad1,
        Self::Pad2,
        Self::Pad3,
    ];

    /// Maps an evdev key code to a bindable tablet button.
    pub fn from_key(key: KeyCode) -> Option<Self> {
        match key {
            KeyCode::BTN_TOUCH => Some(Self::PenTip),
            KeyCode::BTN_STYLUS => Some(Self::PenBarrelLower),
            KeyCode::BTN_STYLUS2 => Some(Self::PenBarrelUpper),
            KeyCode::BTN_0 => Some(Self::Pad0),
            KeyCode::BTN_1 => Some(Self::Pad1),
            KeyCode::BTN_2 => Some(Self::Pad2),
            KeyCode::BTN_3 => Some(Self::Pad3),
            _ => None,
        }
    }

    /// Whether this is one of the ExpressKey pad buttons.
    pub fn is_pad(self) -> bool {
        matches!(self, Self::Pad0 | Self::Pad1 | Self::Pad2 | Self::Pad3)
    }

    /// Configuration field name used in `[buttons]`.
    pub fn field_name(self) -> &'static str {
        match self {
            Self::PenTip => "pen_tip",
            Self::PenBarrelLower => "pen_barrel_lower",
            Self::PenBarrelUpper => "pen_barrel_upper",
            Self::Pad0 => "pad_0",
            Self::Pad1 => "pad_1",
            Self::Pad2 => "pad_2",
            Self::Pad3 => "pad_3",
        }
    }
}

/// A virtual mouse button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Side,
    Extra,
}

/// A standard Xbox 360 gamepad button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GamepadButton {
    South,
    East,
    North,
    West,
    Select,
    Start,
    Mode,
    ThumbL,
    ThumbR,
    ShoulderL,
    ShoulderR,
    DpadUp,
    DpadDown,
    DpadLeft,
    DpadRight,
}

/// A single action bound to a physical tablet button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    None,
    Mouse(MouseButton),
    /// A wheel notch. Positive is up, negative is down.
    Wheel(i32),
    Gamepad(GamepadButton),
    /// Re-anchor steering at the current pen position (or reset to the
    /// configured centre when the pen is lifted).
    Recenter,
    /// Release the pen grab so the compositor handles it natively. Pressing
    /// again resumes game input. Bind to an ExpressKey (`pad_0..pad_3`).
    TogglePassthrough,
    /// Like [`Self::TogglePassthrough`], but only while the button is held.
    HoldPassthrough,
}

impl Action {
    /// Parses a user-facing action name.
    ///
    /// Both short names (`left`, `a`, `wheel_up`) and evdev-style names
    /// (`btn_left`, `btn_south`) are accepted.
    pub fn parse(value: &str) -> Result<Self, String> {
        let name = value.trim().to_ascii_lowercase().replace([' ', '-'], "_");
        let action = match name.as_str() {
            "" | "none" | "off" | "disabled" => Self::None,

            "btn_left" | "left" | "mouse_left" | "lmb" | "click" => Self::Mouse(MouseButton::Left),
            "btn_right" | "right" | "mouse_right" | "rmb" => Self::Mouse(MouseButton::Right),
            "btn_middle" | "middle" | "mouse_middle" | "mmb" => Self::Mouse(MouseButton::Middle),
            "btn_side" | "side" | "mouse_side" => Self::Mouse(MouseButton::Side),
            "btn_extra" | "extra" | "mouse_extra" => Self::Mouse(MouseButton::Extra),

            "wheel_up" | "scroll_up" | "wheel+1" => Self::Wheel(1),
            "wheel_down" | "scroll_down" | "wheel-1" => Self::Wheel(-1),

            "btn_a" | "a" | "btn_south" | "south" | "cross" => Self::Gamepad(GamepadButton::South),
            "btn_b" | "b" | "btn_east" | "east" | "circle" => Self::Gamepad(GamepadButton::East),
            "btn_x" | "x" | "btn_north" | "north" | "square" => Self::Gamepad(GamepadButton::North),
            "btn_y" | "y" | "btn_west" | "west" | "triangle" => Self::Gamepad(GamepadButton::West),
            "btn_select" | "select" | "back" | "view" => Self::Gamepad(GamepadButton::Select),
            "btn_start" | "start" | "menu" => Self::Gamepad(GamepadButton::Start),
            "btn_mode" | "mode" | "guide" | "home" => Self::Gamepad(GamepadButton::Mode),
            "btn_thumbl" | "thumbl" | "l3" | "left_stick" => Self::Gamepad(GamepadButton::ThumbL),
            "btn_thumbr" | "thumbr" | "r3" | "right_stick" => Self::Gamepad(GamepadButton::ThumbR),
            "btn_shoulder_left" | "btn_tl" | "tl" | "lb" | "l1" | "left_bumper" => {
                Self::Gamepad(GamepadButton::ShoulderL)
            }
            "btn_shoulder_right" | "btn_tr" | "tr" | "rb" | "r1" | "right_bumper" => {
                Self::Gamepad(GamepadButton::ShoulderR)
            }
            "btn_dpad_up" | "dpad_up" | "up" => Self::Gamepad(GamepadButton::DpadUp),
            "btn_dpad_down" | "dpad_down" | "down" => Self::Gamepad(GamepadButton::DpadDown),
            "btn_dpad_left" | "dpad_left" | "dpad_l" => Self::Gamepad(GamepadButton::DpadLeft),
            "btn_dpad_right" | "dpad_right" | "dpad_r" => Self::Gamepad(GamepadButton::DpadRight),

            "recenter" | "centre" | "center" | "reset" => Self::Recenter,

            "toggle_passthrough" | "toggle_grab" | "passthrough" | "menu_mode" => {
                Self::TogglePassthrough
            }
            "hold_passthrough" | "hold_grab" => Self::HoldPassthrough,

            other => {
                return Err(format!(
                    "unknown action '{other}' (expected none, btn_left, btn_right, \
                     btn_middle, wheel_up, wheel_down, btn_a..btn_y, btn_start, \
                     btn_select, btn_mode, btn_tl, btn_tr, btn_thumbl, btn_thumbr, \
                     btn_dpad_*, recenter, toggle_passthrough, or hold_passthrough)"
                ))
            }
        };
        Ok(action)
    }

    pub fn is_mouse(self) -> bool {
        matches!(self, Self::Mouse(_) | Self::Wheel(_))
    }

    pub fn is_gamepad(self) -> bool {
        matches!(self, Self::Gamepad(_))
    }

    /// Whether this action controls the pen-grab passthrough state.
    pub fn is_passthrough(self) -> bool {
        matches!(self, Self::TogglePassthrough | Self::HoldPassthrough)
    }
}

impl std::fmt::Display for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            Self::None => "none",
            Self::Mouse(MouseButton::Left) => "btn_left",
            Self::Mouse(MouseButton::Right) => "btn_right",
            Self::Mouse(MouseButton::Middle) => "btn_middle",
            Self::Mouse(MouseButton::Side) => "btn_side",
            Self::Mouse(MouseButton::Extra) => "btn_extra",
            Self::Wheel(1) => "wheel_up",
            Self::Wheel(-1) => "wheel_down",
            Self::Wheel(_) => "wheel",
            Self::Gamepad(GamepadButton::South) => "btn_a",
            Self::Gamepad(GamepadButton::East) => "btn_b",
            Self::Gamepad(GamepadButton::North) => "btn_x",
            Self::Gamepad(GamepadButton::West) => "btn_y",
            Self::Gamepad(GamepadButton::Select) => "btn_select",
            Self::Gamepad(GamepadButton::Start) => "btn_start",
            Self::Gamepad(GamepadButton::Mode) => "btn_mode",
            Self::Gamepad(GamepadButton::ThumbL) => "btn_thumbl",
            Self::Gamepad(GamepadButton::ThumbR) => "btn_thumbr",
            Self::Gamepad(GamepadButton::ShoulderL) => "btn_tl",
            Self::Gamepad(GamepadButton::ShoulderR) => "btn_tr",
            Self::Gamepad(GamepadButton::DpadUp) => "btn_dpad_up",
            Self::Gamepad(GamepadButton::DpadDown) => "btn_dpad_down",
            Self::Gamepad(GamepadButton::DpadLeft) => "btn_dpad_left",
            Self::Gamepad(GamepadButton::DpadRight) => "btn_dpad_right",
            Self::Recenter => "recenter",
            Self::TogglePassthrough => "toggle_passthrough",
            Self::HoldPassthrough => "hold_passthrough",
        };
        f.write_str(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mouse_actions() {
        assert_eq!(Action::parse("left"), Ok(Action::Mouse(MouseButton::Left)));
        assert_eq!(
            Action::parse("btn_right"),
            Ok(Action::Mouse(MouseButton::Right))
        );
        assert_eq!(Action::parse("wheel_up"), Ok(Action::Wheel(1)));
        assert_eq!(Action::parse("scroll-down"), Ok(Action::Wheel(-1)));
        assert_eq!(Action::parse("none"), Ok(Action::None));
    }

    #[test]
    fn parses_gamepad_actions() {
        assert_eq!(
            Action::parse("btn_a"),
            Ok(Action::Gamepad(GamepadButton::South))
        );
        assert_eq!(
            Action::parse("btn_south"),
            Ok(Action::Gamepad(GamepadButton::South))
        );
        assert_eq!(
            Action::parse("LB"),
            Ok(Action::Gamepad(GamepadButton::ShoulderL))
        );
        assert_eq!(
            Action::parse("dpad_up"),
            Ok(Action::Gamepad(GamepadButton::DpadUp))
        );
    }

    #[test]
    fn parses_passthrough_actions() {
        for name in [
            "toggle_passthrough",
            "toggle-grab",
            "passthrough",
            "menu_mode",
        ] {
            assert_eq!(Action::parse(name), Ok(Action::TogglePassthrough), "{name}");
        }
        for name in ["hold_passthrough", "hold-grab"] {
            assert_eq!(Action::parse(name), Ok(Action::HoldPassthrough), "{name}");
        }
        assert!(Action::TogglePassthrough.is_passthrough());
        assert!(Action::HoldPassthrough.is_passthrough());
        assert!(!Action::Recenter.is_passthrough());
    }

    #[test]
    fn rejects_unknown_actions() {
        assert!(Action::parse("key_space").is_err());
        assert!(Action::parse("keyboard").is_err());
    }

    #[test]
    fn display_round_trips_common_names() {
        for name in [
            "none",
            "btn_left",
            "btn_right",
            "btn_middle",
            "wheel_up",
            "wheel_down",
            "btn_a",
            "btn_b",
            "btn_x",
            "btn_y",
            "btn_start",
            "recenter",
            "toggle_passthrough",
            "hold_passthrough",
        ] {
            let action = Action::parse(name).unwrap();
            assert_eq!(Action::parse(&action.to_string()), Ok(action));
        }
    }
}
