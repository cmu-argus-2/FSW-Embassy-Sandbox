//! Rail power limits and the battery power flag.
//!
//! The reading itself is done by the task through the `fsw-hal` sensor traits.

use fsw_hal::SensorFault;
use fsw_hal::sensors::PowerReading;

use super::{PowerFlag, Rail, RailReading};

/// Number of readings averaged before comparing a rail's power to its limit. One reading per cycle
/// at 1 Hz, as in the Python flight software, where the window is the task frequency: so in effect
/// a single reading over the limit raises the alert.
pub const POWER_ALERT_WINDOW: usize = 1;

/// Average power at or above which a rail raises an alert, in W.
pub const fn power_limit(rail: Rail) -> f32 {
    match rail {
        // TODO: 1 W suits v2 mainboards; the Python notes 0.4 W for v3. Check against v4.
        Rail::Board => 1.0,
        Rail::Gps => 1.0,
        Rail::Radio => 3.3,
        Rail::Jetson => 16.0,
    }
}

/// At or below this state of charge, in %, the flag goes to [`PowerFlag::LowPower`].
pub const LOW_POWER_ENTRY_SOC: f32 = 30.0;
/// At or above this state of charge, in %, the flag goes back to [`PowerFlag::Nominal`].
pub const LOW_POWER_EXIT_SOC: f32 = 40.0;

/// Moving average of one rail's power over the last [`POWER_ALERT_WINDOW`] readings.
#[derive(Debug, Clone, Copy)]
pub struct PowerAverage {
    samples: [f32; POWER_ALERT_WINDOW],
    /// Readings taken so far, capped at the window.
    len: usize,
    /// Where the next reading goes.
    next: usize,
}

impl PowerAverage {
    pub const fn new() -> Self {
        Self {
            samples: [0.0; POWER_ALERT_WINDOW],
            len: 0,
            next: 0,
        }
    }

    /// Add a reading in W and return the average over the readings in the window.
    pub fn update(&mut self, power: f32) -> f32 {
        self.samples[self.next] = power;
        self.next += 1;
        if self.next == POWER_ALERT_WINDOW {
            self.next = 0;
        }
        self.len = (self.len + 1).min(POWER_ALERT_WINDOW);
        self.samples[..self.len].iter().sum::<f32>() / self.len as f32
    }
}

impl Default for PowerAverage {
    fn default() -> Self {
        Self::new()
    }
}

/// Check one rail: a failed read gives no value and no alert, and leaves the average as it was;
/// otherwise the reading's power goes into `average` and is compared to the rail's limit.
pub fn check_rail(
    rail: Rail,
    raw: Result<PowerReading, SensorFault>,
    average: &mut PowerAverage,
) -> RailReading {
    match raw {
        Ok(r) => {
            let avg = average.update(r.voltage * r.current);
            RailReading {
                rail,
                value: Some(r),
                over_limit: avg >= power_limit(rail),
            }
        }
        Err(SensorFault) => RailReading {
            rail,
            value: None,
            over_limit: false,
        },
    }
}

/// The power flag after reading state of charge `soc` (%), with hysteresis: low power at or below
/// [`LOW_POWER_ENTRY_SOC`], nominal at or above [`LOW_POWER_EXIT_SOC`], and in between the flag
/// stays low power if it already was, otherwise nominal.
///
/// Returns `None` if `soc` is outside 0-100 (or NaN): the reading is not trusted and the caller
/// should keep the current flag.
pub fn next_power_flag(current: PowerFlag, soc: f32) -> Option<PowerFlag> {
    if (0.0..=LOW_POWER_ENTRY_SOC).contains(&soc) {
        Some(PowerFlag::LowPower)
    } else if soc > LOW_POWER_ENTRY_SOC && soc < LOW_POWER_EXIT_SOC {
        Some(match current {
            PowerFlag::LowPower => PowerFlag::LowPower,
            PowerFlag::Unknown | PowerFlag::Nominal => PowerFlag::Nominal,
        })
    } else if (LOW_POWER_EXIT_SOC..=100.0).contains(&soc) {
        Some(PowerFlag::Nominal)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading(voltage: f32, current: f32) -> Result<PowerReading, SensorFault> {
        Ok(PowerReading { voltage, current })
    }

    #[test]
    fn rail_under_limit() {
        let r = check_rail(Rail::Radio, reading(3.3, 0.5), &mut PowerAverage::new());
        assert_eq!(
            r.value,
            Some(PowerReading {
                voltage: 3.3,
                current: 0.5
            })
        );
        assert!(!r.over_limit);
    }

    #[test]
    fn rail_at_limit_alerts() {
        // 5 V * 0.2 A = 1 W, exactly the board limit.
        let r = check_rail(Rail::Board, reading(5.0, 0.2), &mut PowerAverage::new());
        assert!(r.over_limit);
    }

    #[test]
    fn each_rail_has_its_own_limit() {
        // 8 W: over the radio's limit, under the Jetson's.
        let radio = check_rail(Rail::Radio, reading(8.0, 1.0), &mut PowerAverage::new());
        let jetson = check_rail(Rail::Jetson, reading(8.0, 1.0), &mut PowerAverage::new());
        assert!(radio.over_limit);
        assert!(!jetson.over_limit);
    }

    #[test]
    fn failed_read_gives_no_value_and_no_alert() {
        let mut avg = PowerAverage::new();
        check_rail(Rail::Board, reading(5.0, 1.0), &mut avg);
        let r = check_rail(Rail::Board, Err(SensorFault), &mut avg);
        assert_eq!(r.value, None);
        assert!(!r.over_limit);
    }

    #[test]
    fn nan_reading_does_not_alert() {
        let r = check_rail(
            Rail::Board,
            reading(f32::NAN, 1.0),
            &mut PowerAverage::new(),
        );
        assert!(!r.over_limit);
    }

    #[test]
    fn average_follows_latest_readings() {
        let mut avg = PowerAverage::new();
        assert_eq!(avg.update(2.0), 2.0);
        // With a one-reading window the average is the latest reading.
        if POWER_ALERT_WINDOW == 1 {
            assert_eq!(avg.update(4.0), 4.0);
        }
    }

    #[test]
    fn flag_low_at_and_below_entry() {
        for flag in [PowerFlag::Unknown, PowerFlag::LowPower, PowerFlag::Nominal] {
            assert_eq!(next_power_flag(flag, 0.0), Some(PowerFlag::LowPower));
            assert_eq!(next_power_flag(flag, 30.0), Some(PowerFlag::LowPower));
        }
    }

    #[test]
    fn flag_nominal_at_and_above_exit() {
        for flag in [PowerFlag::Unknown, PowerFlag::LowPower, PowerFlag::Nominal] {
            assert_eq!(next_power_flag(flag, 40.0), Some(PowerFlag::Nominal));
            assert_eq!(next_power_flag(flag, 100.0), Some(PowerFlag::Nominal));
        }
    }

    #[test]
    fn flag_holds_in_hysteresis_band() {
        assert_eq!(
            next_power_flag(PowerFlag::LowPower, 35.0),
            Some(PowerFlag::LowPower)
        );
        assert_eq!(
            next_power_flag(PowerFlag::Nominal, 35.0),
            Some(PowerFlag::Nominal)
        );
        assert_eq!(
            next_power_flag(PowerFlag::Unknown, 35.0),
            Some(PowerFlag::Nominal)
        );
        assert_eq!(
            next_power_flag(PowerFlag::LowPower, 39.9),
            Some(PowerFlag::LowPower)
        );
        assert_eq!(
            next_power_flag(PowerFlag::Nominal, 30.1),
            Some(PowerFlag::Nominal)
        );
    }

    #[test]
    fn flag_rejects_invalid_soc() {
        assert_eq!(next_power_flag(PowerFlag::Nominal, -1.0), None);
        assert_eq!(next_power_flag(PowerFlag::Nominal, 100.5), None);
        assert_eq!(next_power_flag(PowerFlag::Nominal, f32::NAN), None);
    }
}
