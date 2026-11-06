//! Battery heater switching.
//!
//! Each of the two battery heaters switches on its own battery temperature, with hysteresis. The
//! switching itself is done by the task.

/// At or below this battery temperature, in °C, a heater turns on.
pub const HEATER_ON_TEMP: f32 = 0.0;
/// At or above this battery temperature, in °C, a heater turns off.
pub const HEATER_OFF_TEMP: f32 = 5.0;

/// The mainboard runs this much warmer than the battery pack, in °C. Without the fuel gauge's
/// thermistors, battery temperature is estimated as mainboard temperature minus this.
pub const MAINBOARD_TEMP_OFFSET: f32 = 2.0;

/// Whether a heater should be on, given whether it is now and its battery's temperature in °C.
/// Between the two thresholds it stays as it is. A NaN temperature leaves it as it is.
pub fn heater_on(on: bool, temp: f32) -> bool {
    if !on && temp <= HEATER_ON_TEMP {
        true
    } else if on && temp >= HEATER_OFF_TEMP {
        false
    } else {
        on
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turns_on_when_cold() {
        assert!(heater_on(false, 0.0));
        assert!(heater_on(false, -10.0));
    }

    #[test]
    fn turns_off_when_warm() {
        assert!(!heater_on(true, 5.0));
        assert!(!heater_on(true, 20.0));
    }

    #[test]
    fn holds_between_thresholds() {
        assert!(heater_on(true, 2.5));
        assert!(!heater_on(false, 2.5));
    }

    #[test]
    fn nan_holds() {
        assert!(heater_on(true, f32::NAN));
        assert!(!heater_on(false, f32::NAN));
    }
}
