//! Pure mapping math: tablet coordinates to steering and pedals.
//!
//! Everything here is deterministic and free of I/O so it can be tested
//! without `/dev/uinput` or a physical tablet.

use crate::config::{GamepadConfig, PedalScheme, SteeringAxis};

/// Output range of the virtual gamepad's stick axes.
pub const STICK_MIN: i32 = -32767;
pub const STICK_MAX: i32 = 32767;
/// Output range of the virtual gamepad's trigger axes.
pub const TRIGGER_MIN: i32 = 0;
pub const TRIGGER_MAX: i32 = 32767;

/// Inclusive range of an absolute tablet axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AxisRange {
    pub min: i32,
    pub max: i32,
}

impl AxisRange {
    pub const fn new(min: i32, max: i32) -> Self {
        Self { min, max }
    }

    pub fn span(self) -> f64 {
        (self.max - self.min) as f64
    }

    /// Normalises a raw value into `[0.0, 1.0]`.
    pub fn normalize(self, raw: i32) -> f64 {
        if self.span() <= 0.0 {
            return 0.5;
        }
        ((raw - self.min) as f64 / self.span()).clamp(0.0, 1.0)
    }
}

impl Default for AxisRange {
    fn default() -> Self {
        Self::new(0, 21600)
    }
}

/// Applies a deadzone and response curve to a signed value in `[-1.0, 1.0]`.
fn shape(value: f64, deadzone: f64, curve: f64) -> f64 {
    let clamped = value.clamp(-1.0, 1.0);
    let magnitude = clamped.abs();
    if magnitude <= deadzone {
        return 0.0;
    }
    let deadzone = deadzone.clamp(0.0, 0.999_999);
    let normalised = (magnitude - deadzone) / (1.0 - deadzone);
    let curved = if (curve - 1.0).abs() > 1e-9 {
        normalised.powf(curve)
    } else {
        normalised
    };
    clamped.signum() * curved
}

fn to_stick(value: f64) -> i32 {
    (value.clamp(-1.0, 1.0) * STICK_MAX as f64).round() as i32
}

fn to_trigger(value: f64) -> i32 {
    (value.clamp(0.0, 1.0) * TRIGGER_MAX as f64).round() as i32
}

/// Maps one tablet axis to steering.
#[derive(Debug, Clone)]
pub struct SteeringMapper {
    pub range: AxisRange,
    pub center: f64,
    pub default_center: f64,
    pub half_span: f64,
    pub deadzone: f64,
    pub curve: f64,
    pub invert: bool,
}

impl SteeringMapper {
    pub fn new(range: AxisRange, config: &GamepadConfig) -> Self {
        Self {
            range,
            center: config.steering_center,
            default_center: config.steering_center,
            half_span: config.steering_half_span,
            deadzone: config.steering_deadzone,
            curve: config.steering_curve,
            invert: config.invert_steering,
        }
    }

    /// Signed deflection in `[-1.0, 1.0]` after deadzone and curve.
    pub fn deflection(&self, raw: i32) -> f64 {
        let normalised = self.range.normalize(raw);
        let half_span = self.half_span.max(1e-9);
        let offset = (normalised - self.center) / half_span;
        let shaped = shape(offset, self.deadzone, self.curve);
        if self.invert {
            -shaped
        } else {
            shaped
        }
    }

    /// Maps a raw axis value to `[-32767, 32767]`.
    pub fn map(&self, raw: i32) -> i32 {
        to_stick(self.deflection(raw))
    }

    /// Anchors neutral steering at the current pen position.
    pub fn recenter(&mut self, raw: i32) {
        self.center = self.range.normalize(raw);
    }

    /// Restores the configured centre.
    pub fn reset_center(&mut self) {
        self.center = self.default_center;
    }
}

/// Throttle/brake output for one input update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PedalOutput {
    Off,
    Split { throttle: i32, brake: i32 },
    Pressure { throttle: i32 },
    Combined(i32),
}

/// Maps the tablet's other axis (or pressure) to throttle and brake.
#[derive(Debug, Clone)]
pub struct PedalMapper {
    pub range: AxisRange,
    pub scheme: PedalScheme,
    pub deadzone: f64,
    pub curve: f64,
}

impl PedalMapper {
    pub fn new(range: AxisRange, config: &GamepadConfig) -> Self {
        Self {
            range,
            scheme: config.pedal_scheme,
            deadzone: config.pedal_deadzone,
            curve: config.pedal_curve,
        }
    }

    /// Maps a raw axis value. The top of the tablet is throttle.
    pub fn map_axis(&self, raw: i32) -> PedalOutput {
        if !self.scheme.is_enabled() {
            return PedalOutput::Off;
        }
        let normalised = self.range.normalize(raw);
        // +1 at the top (throttle), -1 at the bottom (brake).
        let deflection = (0.5 - normalised) * 2.0;
        match self.scheme {
            PedalScheme::Split => {
                let shaped = shape(deflection, self.deadzone, self.curve);
                if shaped >= 0.0 {
                    PedalOutput::Split {
                        throttle: to_trigger(shaped),
                        brake: 0,
                    }
                } else {
                    PedalOutput::Split {
                        throttle: 0,
                        brake: to_trigger(-shaped),
                    }
                }
            }
            PedalScheme::Combined => {
                PedalOutput::Combined(to_stick(shape(deflection, self.deadzone, self.curve)))
            }
            PedalScheme::Pressure => PedalOutput::Off,
        }
    }

    /// Maps raw stylus pressure to throttle.
    pub fn map_pressure(&self, pressure: i32, pressure_max: i32) -> PedalOutput {
        if self.scheme != PedalScheme::Pressure {
            return PedalOutput::Off;
        }
        if pressure_max <= 0 {
            return PedalOutput::Pressure { throttle: 0 };
        }
        let normalised = (pressure as f64 / pressure_max as f64).clamp(0.0, 1.0);
        let shaped = shape(normalised, self.deadzone, self.curve);
        PedalOutput::Pressure {
            throttle: to_trigger(shaped),
        }
    }
}

impl PedalScheme {
    /// `Pressure` is only meaningful when the pen reports pressure, but it is
    /// still an enabled scheme; the engine falls back cleanly if it does not.
    pub fn is_enabled(self) -> bool {
        matches!(self, Self::Split | Self::Pressure | Self::Combined)
    }
}

impl SteeringAxis {
    pub fn range_for(self, x: AxisRange, y: AxisRange) -> AxisRange {
        match self {
            Self::X => x,
            Self::Y => y,
        }
    }

    /// The axis used for pedals is the other one.
    pub fn pedal_range_for(self, x: AxisRange, y: AxisRange) -> AxisRange {
        match self {
            Self::X => y,
            Self::Y => x,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gamepad_config() -> GamepadConfig {
        GamepadConfig {
            steering_curve: 1.0,
            steering_deadzone: 0.0,
            ..GamepadConfig::default()
        }
    }

    #[test]
    fn steering_maps_edges_and_center() {
        let mapper = SteeringMapper::new(AxisRange::new(0, 20000), &gamepad_config());
        assert_eq!(mapper.map(10000), 0);
        assert_eq!(mapper.map(20000), STICK_MAX);
        assert_eq!(mapper.map(0), STICK_MIN);
    }

    #[test]
    fn steering_deadzone_zeroes_the_center() {
        let mut config = gamepad_config();
        config.steering_deadzone = 0.2;
        let mapper = SteeringMapper::new(AxisRange::new(0, 20000), &config);
        assert_eq!(mapper.map(10000), 0);
        // 0.55 normalised -> offset 0.1, inside the 0.2 deadzone.
        assert_eq!(mapper.map(11000), 0);
        // 0.8 normalised -> offset 0.6 -> (0.6-0.2)/0.8 = 0.5
        assert_eq!(mapper.map(16000), 16384);
    }

    #[test]
    fn steering_curve_is_symmetric() {
        let mut config = gamepad_config();
        config.steering_curve = 2.0;
        let mapper = SteeringMapper::new(AxisRange::new(0, 20000), &config);
        assert_eq!(mapper.map(15000), 8192); // 0.5^2 * 32767
        assert_eq!(mapper.map(5000), -8192);
    }

    #[test]
    fn recenter_and_reset_change_neutral_point() {
        let mut mapper = SteeringMapper::new(AxisRange::new(0, 20000), &gamepad_config());
        mapper.recenter(15000);
        assert_eq!(mapper.map(15000), 0);
        assert_eq!(mapper.map(20000), 16384);
        mapper.reset_center();
        assert_eq!(mapper.map(10000), 0);
    }

    #[test]
    fn split_pedals_map_top_to_throttle_and_bottom_to_brake() {
        let mut config = gamepad_config();
        config.pedal_scheme = PedalScheme::Split;
        let mapper = PedalMapper::new(AxisRange::new(0, 10000), &config);
        assert_eq!(
            mapper.map_axis(0),
            PedalOutput::Split {
                throttle: TRIGGER_MAX,
                brake: 0
            }
        );
        assert_eq!(
            mapper.map_axis(10000),
            PedalOutput::Split {
                throttle: 0,
                brake: TRIGGER_MAX
            }
        );
        assert_eq!(
            mapper.map_axis(5000),
            PedalOutput::Split {
                throttle: 0,
                brake: 0
            }
        );
    }

    #[test]
    fn pressure_pedals_scale_to_trigger() {
        let mut config = gamepad_config();
        config.pedal_scheme = PedalScheme::Pressure;
        let mapper = PedalMapper::new(AxisRange::new(0, 10000), &config);
        assert_eq!(
            mapper.map_pressure(0, 4095),
            PedalOutput::Pressure { throttle: 0 }
        );
        assert_eq!(
            mapper.map_pressure(4095, 4095),
            PedalOutput::Pressure {
                throttle: TRIGGER_MAX
            }
        );
    }

    #[test]
    fn combined_pedal_uses_signed_axis() {
        let mut config = gamepad_config();
        config.pedal_scheme = PedalScheme::Combined;
        let mapper = PedalMapper::new(AxisRange::new(0, 10000), &config);
        assert_eq!(mapper.map_axis(0), PedalOutput::Combined(STICK_MAX));
        assert_eq!(mapper.map_axis(10000), PedalOutput::Combined(STICK_MIN));
        assert_eq!(mapper.map_axis(5000), PedalOutput::Combined(0));
    }
}
