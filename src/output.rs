//! Virtual output devices: relative mouse and Xbox 360 gamepad.
//!
//! Outputs are hidden behind traits so the engine can be tested with mocks and
//! exercised with `--dry-run` without touching `/dev/uinput`.

use std::collections::HashSet;
use std::io;
use std::sync::{Arc, Mutex};

use evdev::uinput::VirtualDevice;
use evdev::{
    AbsInfo, AbsoluteAxisCode, AttributeSet, BusType, EventType, InputEvent, InputId, KeyCode,
    RelativeAxisCode, UinputAbsSetup,
};

use crate::action::{GamepadButton, MouseButton};
use crate::mapping::{STICK_MAX, STICK_MIN, TRIGGER_MAX, TRIGGER_MIN};

pub const VIRTUAL_MOUSE_NAME: &str = "Tablet Driver Virtual Mouse";
pub const VIRTUAL_GAMEPAD_NAME: &str = "Tablet Driver Virtual Gamepad";

/// Sink for relative mouse events.
pub trait MouseSink {
    fn emit_motion(&mut self, dx: i32, dy: i32) -> io::Result<()>;
    fn emit_button(&mut self, button: MouseButton, pressed: bool) -> io::Result<()>;
    fn emit_wheel(&mut self, delta: i32) -> io::Result<()>;
    fn name(&self) -> &str;
}

/// Sink for gamepad axes and buttons.
pub trait GamepadSink {
    fn set_steering(&mut self, value: i32) -> io::Result<()>;
    fn set_throttle(&mut self, value: i32) -> io::Result<()>;
    fn set_brake(&mut self, value: i32) -> io::Result<()>;
    fn set_combined_pedal(&mut self, value: i32) -> io::Result<()>;
    fn set_button(&mut self, button: GamepadButton, pressed: bool) -> io::Result<()>;
    /// Centres steering, releases pedals, and releases every held button.
    fn neutral(&mut self) -> io::Result<()>;
    fn name(&self) -> &str;
}

/// Both virtual devices, created only when a configuration needs them.
#[derive(Default)]
pub struct Outputs {
    pub mouse: Option<Box<dyn MouseSink>>,
    pub gamepad: Option<Box<dyn GamepadSink>>,
}

impl Outputs {
    pub fn motion(&mut self, dx: i32, dy: i32) -> io::Result<()> {
        if dx == 0 && dy == 0 {
            return Ok(());
        }
        match &mut self.mouse {
            Some(mouse) => mouse.emit_motion(dx, dy),
            None => {
                log::warn!("mouse motion requested but no mouse output is active");
                Ok(())
            }
        }
    }

    pub fn mouse_button(&mut self, button: MouseButton, pressed: bool) -> io::Result<()> {
        match &mut self.mouse {
            Some(mouse) => mouse.emit_button(button, pressed),
            None => {
                log::warn!("mouse button requested but no mouse output is active");
                Ok(())
            }
        }
    }

    pub fn wheel(&mut self, delta: i32) -> io::Result<()> {
        match &mut self.mouse {
            Some(mouse) => mouse.emit_wheel(delta),
            None => {
                log::warn!("wheel requested but no mouse output is active");
                Ok(())
            }
        }
    }

    pub fn steering(&mut self, value: i32) -> io::Result<()> {
        match &mut self.gamepad {
            Some(gamepad) => gamepad.set_steering(value),
            None => Ok(()),
        }
    }

    pub fn throttle(&mut self, value: i32) -> io::Result<()> {
        match &mut self.gamepad {
            Some(gamepad) => gamepad.set_throttle(value),
            None => Ok(()),
        }
    }

    pub fn brake(&mut self, value: i32) -> io::Result<()> {
        match &mut self.gamepad {
            Some(gamepad) => gamepad.set_brake(value),
            None => Ok(()),
        }
    }

    pub fn combined_pedal(&mut self, value: i32) -> io::Result<()> {
        match &mut self.gamepad {
            Some(gamepad) => gamepad.set_combined_pedal(value),
            None => Ok(()),
        }
    }

    pub fn gamepad_button(&mut self, button: GamepadButton, pressed: bool) -> io::Result<()> {
        match &mut self.gamepad {
            Some(gamepad) => gamepad.set_button(button, pressed),
            None => {
                log::warn!("gamepad button requested but no gamepad output is active");
                Ok(())
            }
        }
    }

    pub fn neutral(&mut self) -> io::Result<()> {
        if let Some(gamepad) = &mut self.gamepad {
            gamepad.neutral()?;
        }
        Ok(())
    }
}

fn mouse_key(button: MouseButton) -> KeyCode {
    match button {
        MouseButton::Left => KeyCode::BTN_LEFT,
        MouseButton::Right => KeyCode::BTN_RIGHT,
        MouseButton::Middle => KeyCode::BTN_MIDDLE,
        MouseButton::Side => KeyCode::BTN_SIDE,
        MouseButton::Extra => KeyCode::BTN_EXTRA,
    }
}

fn gamepad_key(button: GamepadButton) -> KeyCode {
    match button {
        GamepadButton::South => KeyCode::BTN_SOUTH,
        GamepadButton::East => KeyCode::BTN_EAST,
        GamepadButton::North => KeyCode::BTN_NORTH,
        GamepadButton::West => KeyCode::BTN_WEST,
        GamepadButton::Select => KeyCode::BTN_SELECT,
        GamepadButton::Start => KeyCode::BTN_START,
        GamepadButton::Mode => KeyCode::BTN_MODE,
        GamepadButton::ThumbL => KeyCode::BTN_THUMBL,
        GamepadButton::ThumbR => KeyCode::BTN_THUMBR,
        GamepadButton::ShoulderL => KeyCode::BTN_TL,
        GamepadButton::ShoulderR => KeyCode::BTN_TR,
        GamepadButton::DpadUp => KeyCode::BTN_DPAD_UP,
        GamepadButton::DpadDown => KeyCode::BTN_DPAD_DOWN,
        GamepadButton::DpadLeft => KeyCode::BTN_DPAD_LEFT,
        GamepadButton::DpadRight => KeyCode::BTN_DPAD_RIGHT,
    }
}

/// Real uinput virtual mouse.
pub struct UinputMouse {
    device: VirtualDevice,
}

impl UinputMouse {
    pub fn open() -> io::Result<Self> {
        let mut keys = AttributeSet::<KeyCode>::new();
        for key in [
            KeyCode::BTN_LEFT,
            KeyCode::BTN_RIGHT,
            KeyCode::BTN_MIDDLE,
            KeyCode::BTN_SIDE,
            KeyCode::BTN_EXTRA,
        ] {
            keys.insert(key);
        }

        let mut axes = AttributeSet::<RelativeAxisCode>::new();
        for axis in [
            RelativeAxisCode::REL_X,
            RelativeAxisCode::REL_Y,
            RelativeAxisCode::REL_WHEEL,
            RelativeAxisCode::REL_HWHEEL,
        ] {
            axes.insert(axis);
        }

        let device = VirtualDevice::builder()
            .map_err(uinput_error)?
            .name(VIRTUAL_MOUSE_NAME)
            .input_id(InputId::new(BusType::BUS_USB, 0x045e, 0x0040, 0x0111))
            .with_keys(&keys)
            .map_err(uinput_error)?
            .with_relative_axes(&axes)
            .map_err(uinput_error)?
            .build()
            .map_err(uinput_error)?;

        Ok(Self { device })
    }
}

impl MouseSink for UinputMouse {
    fn emit_motion(&mut self, dx: i32, dy: i32) -> io::Result<()> {
        let mut events = Vec::with_capacity(2);
        if dx != 0 {
            events.push(InputEvent::new(
                EventType::RELATIVE.0,
                RelativeAxisCode::REL_X.0,
                dx,
            ));
        }
        if dy != 0 {
            events.push(InputEvent::new(
                EventType::RELATIVE.0,
                RelativeAxisCode::REL_Y.0,
                dy,
            ));
        }
        if events.is_empty() {
            return Ok(());
        }
        self.device.emit(&events)
    }

    fn emit_button(&mut self, button: MouseButton, pressed: bool) -> io::Result<()> {
        let event = InputEvent::new(EventType::KEY.0, mouse_key(button).0, i32::from(pressed));
        self.device.emit(&[event])
    }

    fn emit_wheel(&mut self, delta: i32) -> io::Result<()> {
        if delta == 0 {
            return Ok(());
        }
        let event = InputEvent::new(EventType::RELATIVE.0, RelativeAxisCode::REL_WHEEL.0, delta);
        self.device.emit(&[event])
    }

    fn name(&self) -> &str {
        VIRTUAL_MOUSE_NAME
    }
}

/// Real uinput virtual Xbox 360 gamepad.
pub struct UinputGamepad {
    device: VirtualDevice,
    pressed: HashSet<GamepadButton>,
    dpad_x: i32,
    dpad_y: i32,
}

/// Hat axis values for the currently held d-pad buttons.
///
/// Linux convention: `ABS_HAT0Y` is `-1` for up and `+1` for down.
fn dpad_axes(pressed: &HashSet<GamepadButton>) -> (i32, i32) {
    let mut x = 0;
    let mut y = 0;
    if pressed.contains(&GamepadButton::DpadLeft) {
        x -= 1;
    }
    if pressed.contains(&GamepadButton::DpadRight) {
        x += 1;
    }
    if pressed.contains(&GamepadButton::DpadUp) {
        y -= 1;
    }
    if pressed.contains(&GamepadButton::DpadDown) {
        y += 1;
    }
    (x, y)
}

impl UinputGamepad {
    pub fn open() -> io::Result<Self> {
        let mut keys = AttributeSet::<KeyCode>::new();
        for button in [
            GamepadButton::South,
            GamepadButton::East,
            GamepadButton::North,
            GamepadButton::West,
            GamepadButton::Select,
            GamepadButton::Start,
            GamepadButton::Mode,
            GamepadButton::ThumbL,
            GamepadButton::ThumbR,
            GamepadButton::ShoulderL,
            GamepadButton::ShoulderR,
            GamepadButton::DpadUp,
            GamepadButton::DpadDown,
            GamepadButton::DpadLeft,
            GamepadButton::DpadRight,
        ] {
            keys.insert(gamepad_key(button));
        }

        let stick = AbsInfo::new(0, STICK_MIN, STICK_MAX, 0, 0, 0);
        let trigger = AbsInfo::new(0, TRIGGER_MIN, TRIGGER_MAX, 0, 0, 0);

        let mut builder = VirtualDevice::builder()
            .map_err(uinput_error)?
            .name(VIRTUAL_GAMEPAD_NAME)
            // Microsoft Xbox 360 pad IDs for Wine/Proton/SDL2 recognition.
            .input_id(InputId::new(BusType::BUS_USB, 0x045e, 0x028e, 0x0110))
            .with_keys(&keys)
            .map_err(uinput_error)?;

        for axis in [
            AbsoluteAxisCode::ABS_X,
            AbsoluteAxisCode::ABS_Y,
            AbsoluteAxisCode::ABS_RX,
            AbsoluteAxisCode::ABS_RY,
        ] {
            builder = builder
                .with_absolute_axis(&UinputAbsSetup::new(axis, stick))
                .map_err(uinput_error)?;
        }
        for axis in [AbsoluteAxisCode::ABS_Z, AbsoluteAxisCode::ABS_RZ] {
            builder = builder
                .with_absolute_axis(&UinputAbsSetup::new(axis, trigger))
                .map_err(uinput_error)?;
        }

        // The kernel xpad driver exposes the d-pad as a hat; do the same so
        // SDL/Proton's Xbox 360 mapping recognises it.
        let hat = AbsInfo::new(0, -1, 1, 0, 0, 0);
        for axis in [AbsoluteAxisCode::ABS_HAT0X, AbsoluteAxisCode::ABS_HAT0Y] {
            builder = builder
                .with_absolute_axis(&UinputAbsSetup::new(axis, hat))
                .map_err(uinput_error)?;
        }

        let mut device = builder.build().map_err(uinput_error)?;

        // Publish a neutral initial state so clients see every axis.
        let initial = [
            InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_X.0, 0),
            InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_Y.0, 0),
            InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_Z.0, 0),
            InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_RX.0, 0),
            InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_RY.0, 0),
            InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_RZ.0, 0),
            InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_HAT0X.0, 0),
            InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_HAT0Y.0, 0),
        ];
        device.emit(&initial)?;

        Ok(Self {
            device,
            pressed: HashSet::new(),
            dpad_x: 0,
            dpad_y: 0,
        })
    }
}

impl GamepadSink for UinputGamepad {
    fn set_steering(&mut self, value: i32) -> io::Result<()> {
        let value = value.clamp(STICK_MIN, STICK_MAX);
        self.device.emit(&[InputEvent::new(
            EventType::ABSOLUTE.0,
            AbsoluteAxisCode::ABS_X.0,
            value,
        )])
    }

    fn set_throttle(&mut self, value: i32) -> io::Result<()> {
        let value = value.clamp(TRIGGER_MIN, TRIGGER_MAX);
        self.device.emit(&[InputEvent::new(
            EventType::ABSOLUTE.0,
            AbsoluteAxisCode::ABS_RZ.0,
            value,
        )])
    }

    fn set_brake(&mut self, value: i32) -> io::Result<()> {
        let value = value.clamp(TRIGGER_MIN, TRIGGER_MAX);
        self.device.emit(&[InputEvent::new(
            EventType::ABSOLUTE.0,
            AbsoluteAxisCode::ABS_Z.0,
            value,
        )])
    }

    fn set_combined_pedal(&mut self, value: i32) -> io::Result<()> {
        let value = value.clamp(STICK_MIN, STICK_MAX);
        self.device.emit(&[InputEvent::new(
            EventType::ABSOLUTE.0,
            AbsoluteAxisCode::ABS_Y.0,
            value,
        )])
    }

    fn set_button(&mut self, button: GamepadButton, pressed: bool) -> io::Result<()> {
        if pressed {
            self.pressed.insert(button);
        } else {
            self.pressed.remove(&button);
        }
        let mut events = vec![InputEvent::new(
            EventType::KEY.0,
            gamepad_key(button).0,
            i32::from(pressed),
        )];

        let (dpad_x, dpad_y) = dpad_axes(&self.pressed);
        if dpad_x != self.dpad_x {
            events.push(InputEvent::new(
                EventType::ABSOLUTE.0,
                AbsoluteAxisCode::ABS_HAT0X.0,
                dpad_x,
            ));
            self.dpad_x = dpad_x;
        }
        if dpad_y != self.dpad_y {
            events.push(InputEvent::new(
                EventType::ABSOLUTE.0,
                AbsoluteAxisCode::ABS_HAT0Y.0,
                dpad_y,
            ));
            self.dpad_y = dpad_y;
        }

        self.device.emit(&events)
    }

    fn neutral(&mut self) -> io::Result<()> {
        let mut events = vec![
            InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_X.0, 0),
            InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_Y.0, 0),
            InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_Z.0, 0),
            InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_RX.0, 0),
            InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_RY.0, 0),
            InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_RZ.0, 0),
            InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_HAT0X.0, 0),
            InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_HAT0Y.0, 0),
        ];
        for button in self.pressed.drain() {
            events.push(InputEvent::new(EventType::KEY.0, gamepad_key(button).0, 0));
        }
        self.dpad_x = 0;
        self.dpad_y = 0;
        self.device.emit(&events)
    }

    fn name(&self) -> &str {
        VIRTUAL_GAMEPAD_NAME
    }
}

fn uinput_error(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(format!("uinput: {error}"))
}

/// Logs mouse events instead of creating a uinput device.
#[derive(Debug, Default)]
pub struct DryRunMouse;

impl MouseSink for DryRunMouse {
    fn emit_motion(&mut self, dx: i32, dy: i32) -> io::Result<()> {
        log::debug!("[dry-run] mouse motion dx={dx} dy={dy}");
        Ok(())
    }

    fn emit_button(&mut self, button: MouseButton, pressed: bool) -> io::Result<()> {
        log::debug!("[dry-run] mouse {button:?} pressed={pressed}");
        Ok(())
    }

    fn emit_wheel(&mut self, delta: i32) -> io::Result<()> {
        log::debug!("[dry-run] wheel delta={delta}");
        Ok(())
    }

    fn name(&self) -> &str {
        VIRTUAL_MOUSE_NAME
    }
}

/// Logs gamepad events instead of creating a uinput device.
#[derive(Debug, Default)]
pub struct DryRunGamepad;

impl GamepadSink for DryRunGamepad {
    fn set_steering(&mut self, value: i32) -> io::Result<()> {
        log::debug!("[dry-run] steering ABS_X={value}");
        Ok(())
    }

    fn set_throttle(&mut self, value: i32) -> io::Result<()> {
        log::debug!("[dry-run] throttle ABS_RZ={value}");
        Ok(())
    }

    fn set_brake(&mut self, value: i32) -> io::Result<()> {
        log::debug!("[dry-run] brake ABS_Z={value}");
        Ok(())
    }

    fn set_combined_pedal(&mut self, value: i32) -> io::Result<()> {
        log::debug!("[dry-run] combined pedal ABS_Y={value}");
        Ok(())
    }

    fn set_button(&mut self, button: GamepadButton, pressed: bool) -> io::Result<()> {
        log::debug!("[dry-run] gamepad {button:?} pressed={pressed}");
        Ok(())
    }

    fn neutral(&mut self) -> io::Result<()> {
        log::debug!("[dry-run] gamepad neutral");
        Ok(())
    }

    fn name(&self) -> &str {
        VIRTUAL_GAMEPAD_NAME
    }
}

/// In-memory mouse output for tests.
#[derive(Debug, Clone, Default)]
pub struct MockMouse {
    pub motions: Vec<(i32, i32)>,
    pub buttons: Vec<(MouseButton, bool)>,
    pub wheels: Vec<i32>,
}

impl MouseSink for MockMouse {
    fn emit_motion(&mut self, dx: i32, dy: i32) -> io::Result<()> {
        if dx != 0 || dy != 0 {
            self.motions.push((dx, dy));
        }
        Ok(())
    }

    fn emit_button(&mut self, button: MouseButton, pressed: bool) -> io::Result<()> {
        self.buttons.push((button, pressed));
        Ok(())
    }

    fn emit_wheel(&mut self, delta: i32) -> io::Result<()> {
        self.wheels.push(delta);
        Ok(())
    }

    fn name(&self) -> &str {
        VIRTUAL_MOUSE_NAME
    }
}

/// In-memory gamepad output for tests.
#[derive(Debug, Clone, Default)]
pub struct MockGamepad {
    pub steering: Vec<i32>,
    pub throttle: Vec<i32>,
    pub brake: Vec<i32>,
    pub combined: Vec<i32>,
    pub buttons: Vec<(GamepadButton, bool)>,
    pub neutral_count: usize,
}

impl GamepadSink for MockGamepad {
    fn set_steering(&mut self, value: i32) -> io::Result<()> {
        self.steering.push(value.clamp(STICK_MIN, STICK_MAX));
        Ok(())
    }

    fn set_throttle(&mut self, value: i32) -> io::Result<()> {
        self.throttle.push(value.clamp(TRIGGER_MIN, TRIGGER_MAX));
        Ok(())
    }

    fn set_brake(&mut self, value: i32) -> io::Result<()> {
        self.brake.push(value.clamp(TRIGGER_MIN, TRIGGER_MAX));
        Ok(())
    }

    fn set_combined_pedal(&mut self, value: i32) -> io::Result<()> {
        self.combined.push(value.clamp(STICK_MIN, STICK_MAX));
        Ok(())
    }

    fn set_button(&mut self, button: GamepadButton, pressed: bool) -> io::Result<()> {
        self.buttons.push((button, pressed));
        Ok(())
    }

    fn neutral(&mut self) -> io::Result<()> {
        self.neutral_count += 1;
        Ok(())
    }

    fn name(&self) -> &str {
        VIRTUAL_GAMEPAD_NAME
    }
}

/// A clonable handle to a [`MockMouse`], usable as an engine sink.
#[derive(Clone, Default)]
pub struct SharedMouse(pub Arc<Mutex<MockMouse>>);

impl SharedMouse {
    pub fn snapshot(&self) -> MockMouse {
        self.0.lock().expect("mock mouse poisoned").clone()
    }
}

impl MouseSink for SharedMouse {
    fn emit_motion(&mut self, dx: i32, dy: i32) -> io::Result<()> {
        self.0
            .lock()
            .expect("mock mouse poisoned")
            .emit_motion(dx, dy)
    }

    fn emit_button(&mut self, button: MouseButton, pressed: bool) -> io::Result<()> {
        self.0
            .lock()
            .expect("mock mouse poisoned")
            .emit_button(button, pressed)
    }

    fn emit_wheel(&mut self, delta: i32) -> io::Result<()> {
        self.0
            .lock()
            .expect("mock mouse poisoned")
            .emit_wheel(delta)
    }

    fn name(&self) -> &str {
        VIRTUAL_MOUSE_NAME
    }
}

/// A clonable handle to a [`MockGamepad`], usable as an engine sink.
#[derive(Clone, Default)]
pub struct SharedGamepad(pub Arc<Mutex<MockGamepad>>);

impl SharedGamepad {
    pub fn snapshot(&self) -> MockGamepad {
        self.0.lock().expect("mock gamepad poisoned").clone()
    }
}

impl GamepadSink for SharedGamepad {
    fn set_steering(&mut self, value: i32) -> io::Result<()> {
        self.0
            .lock()
            .expect("mock gamepad poisoned")
            .set_steering(value)
    }

    fn set_throttle(&mut self, value: i32) -> io::Result<()> {
        self.0
            .lock()
            .expect("mock gamepad poisoned")
            .set_throttle(value)
    }

    fn set_brake(&mut self, value: i32) -> io::Result<()> {
        self.0
            .lock()
            .expect("mock gamepad poisoned")
            .set_brake(value)
    }

    fn set_combined_pedal(&mut self, value: i32) -> io::Result<()> {
        self.0
            .lock()
            .expect("mock gamepad poisoned")
            .set_combined_pedal(value)
    }

    fn set_button(&mut self, button: GamepadButton, pressed: bool) -> io::Result<()> {
        self.0
            .lock()
            .expect("mock gamepad poisoned")
            .set_button(button, pressed)
    }

    fn neutral(&mut self) -> io::Result<()> {
        self.0.lock().expect("mock gamepad poisoned").neutral()
    }

    fn name(&self) -> &str {
        VIRTUAL_GAMEPAD_NAME
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_mouse_records_events() {
        let mut mouse = MockMouse::default();
        mouse.emit_motion(3, -2).unwrap();
        mouse.emit_button(MouseButton::Left, true).unwrap();
        mouse.emit_wheel(1).unwrap();
        assert_eq!(mouse.motions, vec![(3, -2)]);
        assert_eq!(mouse.buttons, vec![(MouseButton::Left, true)]);
        assert_eq!(mouse.wheels, vec![1]);
    }

    #[test]
    fn mock_gamepad_clamps_axes() {
        let mut gamepad = MockGamepad::default();
        gamepad.set_steering(999_999).unwrap();
        gamepad.set_throttle(-10).unwrap();
        assert_eq!(gamepad.steering, vec![STICK_MAX]);
        assert_eq!(gamepad.throttle, vec![TRIGGER_MIN]);
    }

    #[test]
    fn shared_mocks_are_observable() {
        let mouse = SharedMouse::default();
        let mut sink = mouse.clone();
        sink.emit_motion(5, 6).unwrap();
        assert_eq!(mouse.snapshot().motions, vec![(5, 6)]);
    }

    #[test]
    fn dpad_axes_follow_linux_hat_convention() {
        let mut pressed = HashSet::new();
        assert_eq!(dpad_axes(&pressed), (0, 0));

        pressed.insert(GamepadButton::DpadUp);
        assert_eq!(dpad_axes(&pressed), (0, -1));

        pressed.insert(GamepadButton::DpadRight);
        assert_eq!(dpad_axes(&pressed), (1, -1));

        pressed.remove(&GamepadButton::DpadUp);
        pressed.insert(GamepadButton::DpadDown);
        assert_eq!(dpad_axes(&pressed), (1, 1));
    }
}
