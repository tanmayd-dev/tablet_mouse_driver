//! Event processing and the live polling loop.

use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::io;
use std::os::fd::{AsRawFd, RawFd};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use evdev::{AbsoluteAxisCode, Device, EventType, InputEvent, KeyCode};

use crate::action::{Action, InputButton};
use crate::config::{Config, Mode, MouseScheme, PedalScheme};
use crate::device::{self, DeviceInfo};
use crate::mapping::{AxisRange, PedalMapper, PedalOutput, SteeringMapper};
use crate::mouse::MouseController;
use crate::output::{DryRunGamepad, DryRunMouse, GamepadSink, MouseSink, Outputs};

/// `_IOW('E', 0x90, int)` — take exclusive ownership of an evdev node.
const EVIOCGRAB: libc::c_ulong = 0x4004_4590;

/// Releases an `EVIOCGRAB` on drop, including on panic or early return.
pub struct GrabGuard {
    fd: RawFd,
    active: bool,
}

impl GrabGuard {
    pub fn new(fd: RawFd, grab: bool) -> io::Result<Option<Self>> {
        if !grab {
            return Ok(None);
        }
        let result = unsafe { libc::ioctl(fd, EVIOCGRAB, 1 as libc::c_int) };
        if result < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Some(Self { fd, active: true }))
    }
}

impl Drop for GrabGuard {
    fn drop(&mut self) {
        if self.active {
            let _ = unsafe { libc::ioctl(self.fd, EVIOCGRAB, 0 as libc::c_int) };
        }
    }
}

/// Translates tablet events into virtual device output.
pub struct Engine {
    pub config: Config,
    pub mouse: MouseController,
    pub steering: SteeringMapper,
    pub pedals: PedalMapper,
    pub outputs: Outputs,

    actions: HashMap<InputButton, Action>,
    held: HashSet<InputButton>,
    action_counts: HashMap<Action, usize>,
    pen_in_proximity: bool,
    has_pen_tool: bool,
    last_raw_x: Option<i32>,
    last_raw_y: Option<i32>,
    pressure_max: i32,
    last_steering: Option<i32>,
    last_throttle: Option<i32>,
    last_brake: Option<i32>,
    last_combined: Option<i32>,
}

impl Engine {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        config: Config,
        actions: HashMap<InputButton, Action>,
        outputs: Outputs,
        x_range: AxisRange,
        y_range: AxisRange,
        pressure_max: i32,
        has_pen_tool: bool,
    ) -> Self {
        let steering_range = config.gamepad.steering_axis.range_for(x_range, y_range);
        let pedal_range = config
            .gamepad
            .steering_axis
            .pedal_range_for(x_range, y_range);
        let steering = SteeringMapper::new(steering_range, &config.gamepad);
        let pedals = PedalMapper::new(pedal_range, &config.gamepad);
        let mouse = MouseController::new(x_range, y_range, config.mouse.clone());
        let mut mouse = mouse;
        if !has_pen_tool {
            mouse.set_proximity(true);
        }

        Self {
            config,
            mouse,
            steering,
            pedals,
            outputs,
            actions,
            held: HashSet::new(),
            action_counts: HashMap::new(),
            pen_in_proximity: !has_pen_tool,
            has_pen_tool,
            last_raw_x: None,
            last_raw_y: None,
            pressure_max: pressure_max.max(1),
            last_steering: None,
            last_throttle: None,
            last_brake: None,
            last_combined: None,
        }
    }

    fn active(&self) -> bool {
        self.pen_in_proximity || !self.has_pen_tool
    }

    /// Splits a batch of events into `SYN_REPORT`-terminated frames.
    pub fn handle_batch(&mut self, events: &[InputEvent]) -> io::Result<()> {
        let mut start = 0;
        for (index, event) in events.iter().enumerate() {
            if event.event_type() == EventType::SYNCHRONIZATION {
                self.handle_frame(&events[start..=index])?;
                start = index + 1;
            }
        }
        if start < events.len() {
            self.handle_frame(&events[start..])?;
        }
        Ok(())
    }

    /// Processes ExpressKey pad events.
    ///
    /// The pad node on some tablets also exposes `ABS_X`/`ABS_Y` (for a touch
    /// ring). Those must never be mistaken for pen position, so only key
    /// events are accepted from the pad.
    pub fn handle_pad_batch(&mut self, events: &[InputEvent]) -> io::Result<()> {
        for event in events {
            if event.event_type() == EventType::KEY {
                self.handle_event(event)?;
            }
        }
        Ok(())
    }

    /// Processes one frame, suppressing the kernel's bogus `(0, 0)` coordinate
    /// update that arrives in the same frame as a pen-lift.
    pub fn handle_frame(&mut self, frame: &[InputEvent]) -> io::Result<()> {
        let lift = frame.iter().any(|event| {
            event.event_type() == EventType::KEY
                && event.code() == KeyCode::BTN_TOOL_PEN.0
                && event.value() == 0
        });

        if lift {
            for event in frame {
                if event.event_type() != EventType::ABSOLUTE {
                    self.handle_event(event)?;
                }
            }
            self.pen_in_proximity = false;
            self.mouse.set_proximity(false);
            self.last_raw_x = None;
            self.last_raw_y = None;
            if self.config.general.lift_centers {
                self.center_gamepad()?;
            }
            return Ok(());
        }

        for event in frame {
            self.handle_event(event)?;
        }
        Ok(())
    }

    pub fn handle_event(&mut self, event: &InputEvent) -> io::Result<()> {
        match event.event_type() {
            EventType::KEY => {
                let key = KeyCode::new(event.code());
                let pressed = event.value() != 0;
                if key == KeyCode::BTN_TOOL_PEN {
                    self.pen_in_proximity = pressed;
                    self.mouse.set_proximity(pressed);
                } else if let Some(button) = InputButton::from_key(key) {
                    self.set_button(button, pressed)?;
                }
            }
            EventType::ABSOLUTE => match AbsoluteAxisCode(event.code()) {
                AbsoluteAxisCode::ABS_X => self.on_x(event.value())?,
                AbsoluteAxisCode::ABS_Y => self.on_y(event.value())?,
                AbsoluteAxisCode::ABS_PRESSURE => self.on_pressure(event.value())?,
                _ => {}
            },
            _ => {}
        }
        Ok(())
    }

    fn on_x(&mut self, raw: i32) -> io::Result<()> {
        if !self.active() {
            return Ok(());
        }

        match self.config.general.mode {
            Mode::Mouse => {
                self.last_raw_x = Some(raw);
                let dx = self.mouse.handle_x(raw);
                if dx != 0 {
                    self.outputs.motion(dx, 0)?;
                }
            }
            Mode::Gamepad => {
                if self.config.gamepad.steering_axis == crate::config::SteeringAxis::X {
                    self.last_raw_x = Some(raw);
                    let value = self.steering.map(raw);
                    self.emit_steering(value)?;
                } else if self.config.gamepad.enable_pedals {
                    let output = self.pedals.map_axis(raw);
                    self.apply_pedal(output)?;
                }
            }
        }
        Ok(())
    }

    fn on_y(&mut self, raw: i32) -> io::Result<()> {
        if !self.active() {
            return Ok(());
        }

        match self.config.general.mode {
            Mode::Mouse => {
                self.last_raw_y = Some(raw);
                let dy = self.mouse.handle_y(raw);
                if dy != 0 {
                    self.outputs.motion(0, dy)?;
                }
            }
            Mode::Gamepad => {
                if self.config.gamepad.steering_axis == crate::config::SteeringAxis::Y {
                    self.last_raw_y = Some(raw);
                    let value = self.steering.map(raw);
                    self.emit_steering(value)?;
                } else if self.config.gamepad.enable_pedals {
                    let output = self.pedals.map_axis(raw);
                    self.apply_pedal(output)?;
                }
            }
        }
        Ok(())
    }

    fn on_pressure(&mut self, value: i32) -> io::Result<()> {
        if self.config.general.mode == Mode::Gamepad
            && self.config.gamepad.enable_pedals
            && self.config.gamepad.pedal_scheme == PedalScheme::Pressure
        {
            let output = self.pedals.map_pressure(value, self.pressure_max);
            self.apply_pedal(output)?;
        }
        Ok(())
    }

    fn apply_pedal(&mut self, output: PedalOutput) -> io::Result<()> {
        match output {
            PedalOutput::Off => {}
            PedalOutput::Split { throttle, brake } => {
                self.emit_throttle(throttle)?;
                self.emit_brake(brake)?;
            }
            PedalOutput::Pressure { throttle } => self.emit_throttle(throttle)?,
            PedalOutput::Combined(value) => self.emit_combined(value)?,
        }
        Ok(())
    }

    fn emit_steering(&mut self, value: i32) -> io::Result<()> {
        if self.last_steering == Some(value) {
            return Ok(());
        }
        self.outputs.steering(value)?;
        self.last_steering = Some(value);
        Ok(())
    }

    fn emit_throttle(&mut self, value: i32) -> io::Result<()> {
        if self.last_throttle == Some(value) {
            return Ok(());
        }
        self.outputs.throttle(value)?;
        self.last_throttle = Some(value);
        Ok(())
    }

    fn emit_brake(&mut self, value: i32) -> io::Result<()> {
        if self.last_brake == Some(value) {
            return Ok(());
        }
        self.outputs.brake(value)?;
        self.last_brake = Some(value);
        Ok(())
    }

    fn emit_combined(&mut self, value: i32) -> io::Result<()> {
        if self.last_combined == Some(value) {
            return Ok(());
        }
        self.outputs.combined_pedal(value)?;
        self.last_combined = Some(value);
        Ok(())
    }

    fn center_gamepad(&mut self) -> io::Result<()> {
        if self.config.general.mode != Mode::Gamepad {
            return Ok(());
        }
        self.steering.reset_center();
        self.emit_steering(0)?;
        self.emit_throttle(0)?;
        self.emit_brake(0)?;
        self.emit_combined(0)?;
        Ok(())
    }

    fn set_button(&mut self, button: InputButton, pressed: bool) -> io::Result<()> {
        let was_held = self.held.contains(&button);
        if pressed == was_held {
            return Ok(());
        }
        if pressed {
            self.held.insert(button);
        } else {
            self.held.remove(&button);
        }

        let Some(&action) = self.actions.get(&button) else {
            return Ok(());
        };
        if action == Action::None {
            return Ok(());
        }

        if pressed {
            let count = self.action_counts.entry(action).or_insert(0);
            *count += 1;
            if *count == 1 {
                self.dispatch(action, true)?;
            }
        } else if let Some(count) = self.action_counts.get_mut(&action) {
            if *count > 0 {
                *count -= 1;
                if *count == 0 {
                    self.dispatch(action, false)?;
                }
            }
        }
        Ok(())
    }

    fn dispatch(&mut self, action: Action, pressed: bool) -> io::Result<()> {
        match action {
            Action::None => {}
            Action::Mouse(button) => self.outputs.mouse_button(button, pressed)?,
            Action::Wheel(delta) => {
                if pressed {
                    self.outputs.wheel(delta)?;
                }
            }
            Action::Gamepad(button) => self.outputs.gamepad_button(button, pressed)?,
            Action::Recenter => {
                if pressed {
                    self.recenter()?;
                }
            }
        }
        Ok(())
    }

    fn recenter(&mut self) -> io::Result<()> {
        let raw = match self.config.gamepad.steering_axis {
            crate::config::SteeringAxis::X => self.last_raw_x,
            crate::config::SteeringAxis::Y => self.last_raw_y,
        };

        if self.active() {
            if let Some(raw) = raw {
                self.steering.recenter(raw);
                let value = self.steering.map(raw);
                self.emit_steering(value)?;
                log::info!("Recentered steering at raw value {raw}");
                return Ok(());
            }
        }

        self.steering.reset_center();
        self.emit_steering(0)?;
        log::info!("Reset steering centre to the configured default");
        Ok(())
    }

    /// Emits integrated rate-mode mouse motion. Call on every poll tick.
    pub fn tick(&mut self, dt: f64) -> io::Result<()> {
        if self.config.general.mode == Mode::Mouse && self.config.mouse.scheme == MouseScheme::Rate
        {
            let (dx, dy) = self.mouse.tick(dt);
            if dx != 0 || dy != 0 {
                self.outputs.motion(dx, dy)?;
            }
        }
        Ok(())
    }

    /// Releases everything: buttons, steering, pedals.
    pub fn neutral(&mut self) -> io::Result<()> {
        let actions: Vec<Action> = self.action_counts.keys().copied().collect();
        for action in actions {
            if let Some(count) = self.action_counts.get_mut(&action) {
                *count = 0;
            }
            self.dispatch(action, false)?;
        }
        self.action_counts.clear();
        self.held.clear();
        self.outputs.neutral()?;
        self.mouse.reset();
        self.last_steering = Some(0);
        self.last_throttle = Some(0);
        self.last_brake = Some(0);
        self.last_combined = Some(0);
        Ok(())
    }
}

fn needs_mouse(config: &Config, actions: &HashMap<InputButton, Action>) -> bool {
    config.general.mode == Mode::Mouse || actions.values().any(|action| action.is_mouse())
}

fn needs_gamepad(config: &Config, actions: &HashMap<InputButton, Action>) -> bool {
    config.general.mode == Mode::Gamepad || actions.values().any(|action| action.is_gamepad())
}

/// Creates only the virtual devices the configuration actually needs.
pub fn build_outputs(
    config: &Config,
    actions: &HashMap<InputButton, Action>,
    dry_run: bool,
) -> io::Result<Outputs> {
    let mouse: Option<Box<dyn MouseSink>> = if needs_mouse(config, actions) {
        Some(if dry_run {
            Box::new(DryRunMouse)
        } else {
            Box::new(crate::output::UinputMouse::open()?)
        })
    } else {
        None
    };

    let gamepad: Option<Box<dyn GamepadSink>> = if needs_gamepad(config, actions) {
        Some(if dry_run {
            Box::new(DryRunGamepad)
        } else {
            Box::new(crate::output::UinputGamepad::open()?)
        })
    } else {
        None
    };

    Ok(Outputs { mouse, gamepad })
}

/// Runs the driver until `term` is set or an input device disappears.
pub fn run(config: Config, dry_run: bool, term: Arc<AtomicBool>) -> Result<(), Box<dyn Error>> {
    let devices = device::scan_devices();
    let pen_info = device::select_pen(&devices, config.general.device.as_deref())?;
    let pad_info: Option<DeviceInfo> = match &config.general.pad_device {
        Some(path) => Some(device::inspect(path)?),
        None => device::find_pad(&devices, &pen_info).cloned(),
    };

    log::info!("Pen: {} ('{}')", pen_info.path.display(), pen_info.name);
    if let Some(pad) = &pad_info {
        log::info!("Pad: {} ('{}')", pad.path.display(), pad.name);
    }

    let mut pen = Device::open(&pen_info.path)?;
    let mut pad = match &pad_info {
        Some(pad) => Some(Device::open(&pad.path)?),
        None => None,
    };

    let actions = config.button_map()?;
    let outputs = build_outputs(&config, &actions, dry_run)?;

    let mut engine = Engine::new(
        config.clone(),
        actions,
        outputs,
        pen_info.x_range(),
        pen_info.y_range(),
        pen_info.pressure_max(),
        pen_info.has_pen_tool,
    );

    let _pen_grab = GrabGuard::new(pen.as_raw_fd(), config.general.grab_pen)?;
    if config.general.grab_pen {
        log::info!("Grabbed pen device exclusively");
    }

    let _pad_grab = match pad.as_ref() {
        Some(device) => GrabGuard::new(device.as_raw_fd(), config.general.grab_pad)?,
        None => None,
    };
    if config.general.grab_pad {
        log::info!("Grabbed pad device exclusively");
    }

    let mut poll_fds = vec![libc::pollfd {
        fd: pen.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    }];
    if let Some(device) = &pad {
        poll_fds.push(libc::pollfd {
            fd: device.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        });
    }

    let mut last_tick = Instant::now();
    log::info!(
        "Running in {} mode ({}). Ctrl-C to stop.",
        config.general.mode,
        if dry_run { "dry-run" } else { "uinput" }
    );

    while !term.load(Ordering::Relaxed) {
        let result =
            unsafe { libc::poll(poll_fds.as_mut_ptr(), poll_fds.len() as libc::nfds_t, 10) };

        if term.load(Ordering::Relaxed) {
            break;
        }

        let now = Instant::now();
        let dt = now.duration_since(last_tick).as_secs_f64();
        last_tick = now;

        if result < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error.into());
        }

        if poll_fds[0].revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0 {
            log::warn!("pen device disconnected");
            break;
        }

        if poll_fds[0].revents & libc::POLLIN != 0 {
            match pen.fetch_events() {
                Ok(events) => {
                    let events: Vec<InputEvent> = events.collect();
                    engine.handle_batch(&events)?;
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => {
                    log::warn!("pen read error: {error}");
                    break;
                }
            }
        }

        if poll_fds.len() > 1 && poll_fds[1].revents & libc::POLLIN != 0 {
            if let Some(device) = &mut pad {
                match device.fetch_events() {
                    Ok(events) => {
                        let events: Vec<InputEvent> = events.collect();
                        engine.handle_pad_batch(&events)?;
                    }
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                    Err(error) => log::warn!("pad read error: {error}"),
                }
            }
        }

        engine.tick(dt)?;
    }

    log::info!("Shutting down; releasing buttons and centring outputs");
    engine.neutral()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{MouseConfig, PedalScheme};
    use crate::output::{MockGamepad, MockMouse, SharedGamepad, SharedMouse};

    fn key(code: KeyCode, value: i32) -> InputEvent {
        InputEvent::new(EventType::KEY.0, code.0, value)
    }

    fn abs(code: AbsoluteAxisCode, value: i32) -> InputEvent {
        InputEvent::new(EventType::ABSOLUTE.0, code.0, value)
    }

    fn syn() -> InputEvent {
        InputEvent::new(EventType::SYNCHRONIZATION.0, 0, 0)
    }

    fn mouse_engine(config: Config) -> (Engine, SharedMouse) {
        let actions = config.button_map().unwrap();
        let shared = SharedMouse::default();
        let outputs = Outputs {
            mouse: Some(Box::new(shared.clone())),
            gamepad: None,
        };
        let engine = Engine::new(
            config,
            actions,
            outputs,
            AxisRange::new(0, 21600),
            AxisRange::new(0, 13500),
            4095,
            true,
        );
        (engine, shared)
    }

    fn gamepad_engine(config: Config) -> (Engine, SharedGamepad) {
        let actions = config.button_map().unwrap();
        let shared = SharedGamepad::default();
        let outputs = Outputs {
            mouse: None,
            gamepad: Some(Box::new(shared.clone())),
        };
        let engine = Engine::new(
            config,
            actions,
            outputs,
            AxisRange::new(0, 21600),
            AxisRange::new(0, 13500),
            4095,
            true,
        );
        (engine, shared)
    }

    fn delta_config() -> Config {
        Config {
            mouse: MouseConfig {
                scale: 1.0,
                sensitivity: 1.0,
                ..MouseConfig::default()
            },
            ..Config::default()
        }
    }

    fn gamepad_config() -> Config {
        Config {
            general: crate::config::GeneralConfig {
                mode: Mode::Gamepad,
                grab_pen: false,
                ..Default::default()
            },
            gamepad: crate::config::GamepadConfig {
                steering_curve: 1.0,
                steering_deadzone: 0.0,
                ..Default::default()
            },
            ..Config::default()
        }
    }

    #[test]
    fn mouse_delta_is_emitted_per_axis() {
        let (mut engine, mouse) = mouse_engine(delta_config());
        engine
            .handle_batch(&[
                key(KeyCode::BTN_TOOL_PEN, 1),
                abs(AbsoluteAxisCode::ABS_X, 1000),
                abs(AbsoluteAxisCode::ABS_Y, 1000),
                syn(),
            ])
            .unwrap();
        engine
            .handle_batch(&[
                abs(AbsoluteAxisCode::ABS_X, 1010),
                abs(AbsoluteAxisCode::ABS_Y, 1005),
                syn(),
            ])
            .unwrap();

        let snapshot: MockMouse = mouse.snapshot();
        assert_eq!(snapshot.motions, vec![(10, 0), (0, 5)]);
    }

    #[test]
    fn pen_tip_press_and_release_emit_mouse_button() {
        let (mut engine, mouse) = mouse_engine(delta_config());
        engine
            .handle_batch(&[
                key(KeyCode::BTN_TOOL_PEN, 1),
                abs(AbsoluteAxisCode::ABS_X, 5000),
                key(KeyCode::BTN_TOUCH, 1),
                syn(),
            ])
            .unwrap();
        engine
            .handle_batch(&[key(KeyCode::BTN_TOUCH, 0), syn()])
            .unwrap();

        assert_eq!(
            mouse.snapshot().buttons,
            vec![
                (crate::action::MouseButton::Left, true),
                (crate::action::MouseButton::Left, false)
            ]
        );
    }

    #[test]
    fn lift_does_not_emit_bogus_mouse_delta_from_zero_coordinates() {
        let (mut engine, mouse) = mouse_engine(delta_config());
        engine
            .handle_batch(&[
                key(KeyCode::BTN_TOOL_PEN, 1),
                abs(AbsoluteAxisCode::ABS_X, 20000),
                abs(AbsoluteAxisCode::ABS_Y, 12000),
                syn(),
            ])
            .unwrap();
        // Kernel sends ABS_X=0, ABS_Y=0 together with the lift.
        engine
            .handle_batch(&[
                key(KeyCode::BTN_TOOL_PEN, 0),
                abs(AbsoluteAxisCode::ABS_X, 0),
                abs(AbsoluteAxisCode::ABS_Y, 0),
                syn(),
            ])
            .unwrap();

        assert!(mouse.snapshot().motions.is_empty());
    }

    #[test]
    fn steering_centers_on_lift_and_ignores_zero_coordinates() {
        let (mut engine, gamepad) = gamepad_engine(gamepad_config());
        engine
            .handle_batch(&[
                key(KeyCode::BTN_TOOL_PEN, 1),
                abs(AbsoluteAxisCode::ABS_X, 21600),
                syn(),
            ])
            .unwrap();
        assert_eq!(gamepad.snapshot().steering, vec![crate::mapping::STICK_MAX]);

        engine
            .handle_batch(&[
                key(KeyCode::BTN_TOOL_PEN, 0),
                abs(AbsoluteAxisCode::ABS_X, 0),
                abs(AbsoluteAxisCode::ABS_Y, 0),
                syn(),
            ])
            .unwrap();
        assert_eq!(gamepad.snapshot().steering.last(), Some(&0));
    }

    #[test]
    fn pedals_disabled_ignores_vertical_axis() {
        let (mut engine, gamepad) = gamepad_engine(gamepad_config());
        engine
            .handle_batch(&[
                key(KeyCode::BTN_TOOL_PEN, 1),
                abs(AbsoluteAxisCode::ABS_Y, 0),
                syn(),
            ])
            .unwrap();
        let snapshot: MockGamepad = gamepad.snapshot();
        assert!(snapshot.throttle.is_empty());
        assert!(snapshot.brake.is_empty());
    }

    #[test]
    fn split_pedals_emit_throttle_and_brake() {
        let mut config = gamepad_config();
        config.gamepad.enable_pedals = true;
        config.gamepad.pedal_scheme = PedalScheme::Split;
        let (mut engine, gamepad) = gamepad_engine(config);

        engine
            .handle_batch(&[
                key(KeyCode::BTN_TOOL_PEN, 1),
                abs(AbsoluteAxisCode::ABS_Y, 0),
                syn(),
            ])
            .unwrap();
        engine
            .handle_batch(&[abs(AbsoluteAxisCode::ABS_Y, 13500), syn()])
            .unwrap();

        let snapshot = gamepad.snapshot();
        assert_eq!(
            snapshot.throttle.first(),
            Some(&crate::mapping::TRIGGER_MAX)
        );
        assert_eq!(snapshot.brake.last(), Some(&crate::mapping::TRIGGER_MAX));
    }

    #[test]
    fn steering_axis_y_uses_y_for_steering_and_x_for_pedals() {
        let mut config = gamepad_config();
        config.gamepad.steering_axis = crate::config::SteeringAxis::Y;
        config.gamepad.enable_pedals = true;
        config.gamepad.pedal_scheme = PedalScheme::Split;
        let (mut engine, gamepad) = gamepad_engine(config);

        engine
            .handle_batch(&[
                key(KeyCode::BTN_TOOL_PEN, 1),
                abs(AbsoluteAxisCode::ABS_Y, 13500),
                syn(),
            ])
            .unwrap();
        assert_eq!(
            gamepad.snapshot().steering.last(),
            Some(&crate::mapping::STICK_MAX)
        );

        engine
            .handle_batch(&[abs(AbsoluteAxisCode::ABS_X, 0), syn()])
            .unwrap();
        assert_eq!(
            gamepad.snapshot().throttle.last(),
            Some(&crate::mapping::TRIGGER_MAX)
        );
    }

    #[test]
    fn wheel_is_emitted_once_per_press() {
        let (mut engine, mouse) = mouse_engine(delta_config());
        engine
            .handle_batch(&[key(KeyCode::BTN_TOOL_PEN, 1), key(KeyCode::BTN_0, 1), syn()])
            .unwrap();
        engine
            .handle_batch(&[key(KeyCode::BTN_0, 1), syn()])
            .unwrap();
        engine
            .handle_batch(&[key(KeyCode::BTN_0, 0), syn()])
            .unwrap();
        assert_eq!(mouse.snapshot().wheels, vec![1]);
    }

    #[test]
    fn pad_absolute_axes_are_ignored() {
        let (mut engine, gamepad) = gamepad_engine(gamepad_config());
        engine
            .handle_batch(&[
                key(KeyCode::BTN_TOOL_PEN, 1),
                abs(AbsoluteAxisCode::ABS_X, 10800),
                syn(),
            ])
            .unwrap();
        let before = gamepad.snapshot().steering.len();

        // The pad node can expose ABS_X/ABS_Y for a touch ring. Those must not
        // be interpreted as pen position.
        engine
            .handle_pad_batch(&[
                abs(AbsoluteAxisCode::ABS_X, 0),
                abs(AbsoluteAxisCode::ABS_Y, 0),
                syn(),
            ])
            .unwrap();

        assert_eq!(gamepad.snapshot().steering.len(), before);
    }

    #[test]
    fn pad_buttons_still_work_when_pad_axes_are_filtered() {
        let (mut engine, mouse) = mouse_engine(delta_config());
        engine
            .handle_batch(&[key(KeyCode::BTN_TOOL_PEN, 1), syn()])
            .unwrap();
        engine
            .handle_pad_batch(&[key(KeyCode::BTN_0, 1), syn()])
            .unwrap();
        assert_eq!(mouse.snapshot().wheels, vec![1]);
    }

    #[test]
    fn two_inputs_sharing_an_action_use_reference_counts() {
        let mut config = delta_config();
        config.buttons.pen_barrel_upper = "btn_left".to_string();
        let (mut engine, mouse) = mouse_engine(config);

        engine
            .handle_batch(&[
                key(KeyCode::BTN_TOOL_PEN, 1),
                key(KeyCode::BTN_TOUCH, 1),
                key(KeyCode::BTN_STYLUS2, 1),
                syn(),
            ])
            .unwrap();
        engine
            .handle_batch(&[key(KeyCode::BTN_TOUCH, 0), syn()])
            .unwrap();
        // Still held by the barrel button, so no release yet.
        assert_eq!(
            mouse.snapshot().buttons,
            vec![(crate::action::MouseButton::Left, true)]
        );
        engine
            .handle_batch(&[key(KeyCode::BTN_STYLUS2, 0), syn()])
            .unwrap();
        assert_eq!(
            mouse.snapshot().buttons.last(),
            Some(&(crate::action::MouseButton::Left, false))
        );
    }

    #[test]
    fn neutral_releases_held_buttons() {
        let (mut engine, mouse) = mouse_engine(delta_config());
        engine
            .handle_batch(&[
                key(KeyCode::BTN_TOOL_PEN, 1),
                key(KeyCode::BTN_TOUCH, 1),
                syn(),
            ])
            .unwrap();
        engine.neutral().unwrap();
        assert_eq!(
            mouse.snapshot().buttons.last(),
            Some(&(crate::action::MouseButton::Left, false))
        );
    }

    #[test]
    fn rate_mode_ticks_emit_motion() {
        let mut config = delta_config();
        config.mouse.scheme = MouseScheme::Rate;
        config.mouse.deadzone = 0.0;
        config.mouse.rate_speed = 1000.0;
        let (mut engine, mouse) = mouse_engine(config);
        engine
            .handle_batch(&[
                key(KeyCode::BTN_TOOL_PEN, 1),
                abs(AbsoluteAxisCode::ABS_X, 21600),
                abs(AbsoluteAxisCode::ABS_Y, 6750),
                syn(),
            ])
            .unwrap();
        engine.tick(0.1).unwrap();
        assert_eq!(mouse.snapshot().motions, vec![(100, 0)]);
    }

    #[test]
    fn dry_run_outputs_are_created_without_uinput() {
        let config = delta_config();
        let actions = config.button_map().unwrap();
        let outputs = build_outputs(&config, &actions, true).unwrap();
        assert!(outputs.mouse.is_some());
        assert!(outputs.gamepad.is_none());
    }

    #[test]
    fn unused_outputs_are_not_created() {
        let mut config = gamepad_config();
        config.buttons = crate::config::ButtonConfig {
            pen_tip: "none".to_string(),
            pen_barrel_lower: "none".to_string(),
            pen_barrel_upper: "none".to_string(),
            pad_0: "none".to_string(),
            pad_1: "none".to_string(),
            pad_2: "none".to_string(),
            pad_3: "none".to_string(),
        };
        let actions = config.button_map().unwrap();
        let outputs = build_outputs(&config, &actions, true).unwrap();
        assert!(outputs.mouse.is_none());
        assert!(outputs.gamepad.is_some());
    }
}
