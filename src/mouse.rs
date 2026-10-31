//! Relative mouse translation (delta and rate schemes).

use crate::config::{MouseConfig, MouseScheme};
use crate::mapping::AxisRange;

/// Converts tablet coordinates into relative mouse motion.
#[derive(Debug, Clone)]
pub struct MouseController {
    pub x_range: AxisRange,
    pub y_range: AxisRange,
    pub config: MouseConfig,
    in_proximity: bool,
    last_x: Option<i32>,
    last_y: Option<i32>,
    accum_x: f64,
    accum_y: f64,
}

impl MouseController {
    pub fn new(x_range: AxisRange, y_range: AxisRange, config: MouseConfig) -> Self {
        Self {
            x_range,
            y_range,
            config,
            in_proximity: false,
            last_x: None,
            last_y: None,
            accum_x: 0.0,
            accum_y: 0.0,
        }
    }

    /// Called when the pen enters or leaves tablet proximity. Leaving clears
    /// the tracking baseline so re-entering elsewhere does not jump the camera.
    pub fn set_proximity(&mut self, in_proximity: bool) {
        if self.in_proximity && !in_proximity {
            self.last_x = None;
            self.last_y = None;
            self.accum_x = 0.0;
            self.accum_y = 0.0;
        }
        self.in_proximity = in_proximity;
    }

    pub fn in_proximity(&self) -> bool {
        self.in_proximity
    }

    /// Feeds a raw X coordinate and returns a mouse `dx` for this event.
    pub fn handle_x(&mut self, raw: i32) -> i32 {
        if !self.in_proximity {
            return 0;
        }
        let previous = self.last_x.replace(raw);
        if self.config.scheme != MouseScheme::Delta {
            return 0;
        }
        let Some(previous) = previous else {
            return 0;
        };
        let delta = (raw - previous) as f64 * self.config.scale * self.config.sensitivity;
        let delta = if self.config.invert_x { -delta } else { delta };
        self.accum_x += delta;
        self.take_accum_x()
    }

    /// Feeds a raw Y coordinate and returns a mouse `dy` for this event.
    pub fn handle_y(&mut self, raw: i32) -> i32 {
        if !self.in_proximity {
            return 0;
        }
        let previous = self.last_y.replace(raw);
        if self.config.scheme != MouseScheme::Delta {
            return 0;
        }
        let Some(previous) = previous else {
            return 0;
        };
        let delta = (raw - previous) as f64 * self.config.scale * self.config.sensitivity;
        let delta = if self.config.invert_y { -delta } else { delta };
        self.accum_y += delta;
        self.take_accum_y()
    }

    /// Emits integrated rate-mode motion. `dt` is seconds since the last tick.
    pub fn tick(&mut self, dt: f64) -> (i32, i32) {
        if !self.in_proximity || self.config.scheme != MouseScheme::Rate {
            return (0, 0);
        }
        let (Some(x), Some(y)) = (self.last_x, self.last_y) else {
            return (0, 0);
        };

        let deflection_x = (self.x_range.normalize(x) - 0.5) * 2.0;
        let deflection_y = (self.y_range.normalize(y) - 0.5) * 2.0;
        let rate_x = shape_deflection(deflection_x, self.config.deadzone);
        let rate_y = shape_deflection(deflection_y, self.config.deadzone);

        let speed = self.config.rate_speed * self.config.sensitivity * dt.max(0.0);
        self.accum_x += if self.config.invert_x {
            -rate_x
        } else {
            rate_x
        } * speed;
        self.accum_y += if self.config.invert_y {
            -rate_y
        } else {
            rate_y
        } * speed;

        (self.take_accum_x(), self.take_accum_y())
    }

    /// Clears accumulators and the tracking baseline (used on shutdown).
    pub fn reset(&mut self) {
        self.last_x = None;
        self.last_y = None;
        self.accum_x = 0.0;
        self.accum_y = 0.0;
    }

    fn take_accum_x(&mut self) -> i32 {
        let whole = self.accum_x.trunc() as i32;
        self.accum_x -= whole as f64;
        whole
    }

    fn take_accum_y(&mut self) -> i32 {
        let whole = self.accum_y.trunc() as i32;
        self.accum_y -= whole as f64;
        whole
    }
}

fn shape_deflection(value: f64, deadzone: f64) -> f64 {
    let magnitude = value.abs();
    if magnitude <= deadzone {
        return 0.0;
    }
    let deadzone = deadzone.clamp(0.0, 0.999_999);
    value.signum() * ((magnitude - deadzone) / (1.0 - deadzone))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn delta_config() -> MouseConfig {
        MouseConfig {
            scheme: MouseScheme::Delta,
            scale: 1.0,
            sensitivity: 1.0,
            ..MouseConfig::default()
        }
    }

    #[test]
    fn delta_emits_difference_between_samples() {
        let mut mouse = MouseController::new(
            AxisRange::new(0, 20000),
            AxisRange::new(0, 10000),
            delta_config(),
        );
        mouse.set_proximity(true);

        // First sample only seeds the baseline.
        assert_eq!(mouse.handle_x(5000), 0);
        assert_eq!(mouse.handle_y(5000), 0);

        assert_eq!(mouse.handle_x(5010), 10);
        assert_eq!(mouse.handle_y(5005), 5);
        assert_eq!(mouse.handle_x(5000), -10);
    }

    #[test]
    fn delta_accumulates_sub_pixel_motion() {
        let mut config = delta_config();
        config.scale = 0.25;
        let mut mouse =
            MouseController::new(AxisRange::new(0, 20000), AxisRange::new(0, 10000), config);
        mouse.set_proximity(true);
        mouse.handle_x(0);
        // Three 1-count moves = 0.75 counts, nothing emitted yet.
        for x in 1..=3 {
            assert_eq!(mouse.handle_x(x), 0);
        }
        // Fourth move reaches one whole count.
        assert_eq!(mouse.handle_x(4), 1);
    }

    #[test]
    fn lifting_clears_baseline() {
        let mut mouse = MouseController::new(
            AxisRange::new(0, 20000),
            AxisRange::new(0, 10000),
            delta_config(),
        );
        mouse.set_proximity(true);
        mouse.handle_x(1000);
        assert_eq!(mouse.handle_x(1010), 10);

        mouse.set_proximity(false);
        mouse.handle_x(9000); // ignored while out of proximity
        mouse.set_proximity(true);
        assert_eq!(mouse.handle_x(15000), 0); // seeds a fresh baseline
        assert_eq!(mouse.handle_x(15010), 10);
    }

    #[test]
    fn inverting_axes_flips_direction() {
        let mut config = delta_config();
        config.invert_x = true;
        config.invert_y = true;
        let mut mouse =
            MouseController::new(AxisRange::new(0, 20000), AxisRange::new(0, 10000), config);
        mouse.set_proximity(true);
        mouse.handle_x(1000);
        mouse.handle_y(1000);
        assert_eq!(mouse.handle_x(1010), -10);
        assert_eq!(mouse.handle_y(1010), -10);
    }

    #[test]
    fn rate_mode_emits_continuous_motion() {
        let mut config = delta_config();
        config.scheme = MouseScheme::Rate;
        config.deadzone = 0.1;
        config.rate_speed = 1000.0;
        let mut mouse =
            MouseController::new(AxisRange::new(0, 10000), AxisRange::new(0, 10000), config);
        mouse.set_proximity(true);
        mouse.handle_x(5000);
        mouse.handle_y(5000);
        assert_eq!(mouse.tick(0.1), (0, 0));

        // Full right deflection: 1000 counts/s * 0.1s.
        mouse.handle_x(10000);
        assert_eq!(mouse.tick(0.1), (100, 0));

        // Inside the deadzone: no motion.
        mouse.handle_x(5200);
        assert_eq!(mouse.tick(0.1), (0, 0));

        // Lifting stops rate motion immediately.
        mouse.set_proximity(false);
        assert_eq!(mouse.tick(0.1), (0, 0));
    }
}
