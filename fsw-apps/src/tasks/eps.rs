//! EPS task loop.
//!
//! Each cycle reads the power rails and checks them against their power limits; every
//! [`FUEL_GAUGE_EVERY`]th cycle it also reads the fuel gauge and updates the power flag. Battery
//! heaters, coil monitoring and logging to storage come later.
//!
//! The task is generic over the `fsw-hal` traits and a delay, so it has no idea which board or chip
//! it runs on. Embassy `#[task]` functions cannot be generic, so a binary wraps it in a small task
//! with concrete types.

use fsw_hal::sensors::{BatteryReading, FuelGauge, PowerMonitor};
use fsw_hal::{DelayNs, SensorFault};

use crate::eps::power::{PowerAverage, check_rail, next_power_flag};
use crate::eps::{PowerFlag, Rail, RailReading};

/// EPS runs at 1 Hz.
pub const PERIOD_MS: u32 = 1000;
/// The fuel gauge is read every this many cycles (every 5 s), starting with the first.
pub const FUEL_GAUGE_EVERY: u32 = 5;

/// One cycle's checked readings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Readings {
    /// Indexed by [`Rail`].
    pub rails: [RailReading; Rail::COUNT],
    /// `None` on cycles that skip the fuel gauge.
    pub battery: Option<Result<BatteryReading, SensorFault>>,
    /// The power flag after this cycle. It only changes on a valid fuel gauge reading.
    pub flag: PowerFlag,
}

pub struct EpsTask<F, P, D> {
    fuel_gauge: F,
    /// Indexed by [`Rail`].
    monitors: [P; Rail::COUNT],
    averages: [PowerAverage; Rail::COUNT],
    flag: PowerFlag,
    cycle: u32,
    delay: D,
}

impl<F: FuelGauge, P: PowerMonitor, D: DelayNs> EpsTask<F, P, D> {
    /// `monitors` is indexed by [`Rail`]: `monitors[Rail::Radio as usize]` is the radio's.
    pub fn new(fuel_gauge: F, monitors: [P; Rail::COUNT], delay: D) -> Self {
        Self {
            fuel_gauge,
            monitors,
            averages: [PowerAverage::new(); Rail::COUNT],
            flag: PowerFlag::Unknown,
            cycle: 0,
            delay,
        }
    }

    /// Read and check every rail once, and the fuel gauge if this cycle is due.
    pub async fn step(&mut self) -> Readings {
        let mut rails = Rail::ALL.map(|rail| RailReading {
            rail,
            value: None,
            over_limit: false,
        });
        for rail in Rail::ALL {
            let i = rail as usize;
            let raw = self.monitors[i].power().await;
            rails[i] = check_rail(rail, raw, &mut self.averages[i]);
        }

        let battery = if self.cycle.is_multiple_of(FUEL_GAUGE_EVERY) {
            let raw = self.fuel_gauge.battery().await;
            if let Ok(b) = &raw {
                // An invalid state of charge keeps the current flag.
                if let Some(flag) = next_power_flag(self.flag, b.soc) {
                    self.flag = flag;
                }
            }
            Some(raw)
        } else {
            None
        };
        self.cycle = self.cycle.wrapping_add(1);

        Readings {
            rails,
            battery,
            flag: self.flag,
        }
    }

    /// Run forever: one [`Self::step`] per period, handing each cycle's readings to `on_readings`
    /// (logging, telemetry). The period is a plain delay after each step, so it drifts by the step
    /// time.
    pub async fn run(&mut self, mut on_readings: impl FnMut(&Readings)) -> ! {
        loop {
            let readings = self.step().await;
            on_readings(&readings);
            self.delay.delay_ms(PERIOD_MS).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use embassy_futures::block_on;
    use fsw_hal::sensors::PowerReading;

    use super::*;

    struct FakeMonitor(Result<PowerReading, SensorFault>);

    impl PowerMonitor for FakeMonitor {
        async fn power(&mut self) -> Result<PowerReading, SensorFault> {
            self.0
        }
    }

    /// Returns its readings in order, then repeats the last one; counts reads.
    struct FakeGauge {
        socs: &'static [Option<f32>],
        reads: usize,
    }

    impl FuelGauge for FakeGauge {
        async fn battery(&mut self) -> Result<BatteryReading, SensorFault> {
            let soc = self.socs[self.reads.min(self.socs.len() - 1)];
            self.reads += 1;
            soc.map(battery).ok_or(SensorFault)
        }
    }

    struct NoDelay;

    impl DelayNs for NoDelay {
        async fn delay_ns(&mut self, _ns: u32) {}
    }

    fn battery(soc: f32) -> BatteryReading {
        BatteryReading {
            soc,
            capacity: 2.0,
            current: -0.1,
            voltage: 7.4,
            midpoint_voltage: 3.7,
            time_to_empty: 3600.0,
            time_to_full: 0.0,
            temperature: 20.0,
            temperature_ain1: 20.0,
            temperature_ain2: 20.0,
            temperature_die: 25.0,
        }
    }

    fn rail(voltage: f32, current: f32) -> FakeMonitor {
        FakeMonitor(Ok(PowerReading { voltage, current }))
    }

    fn task(socs: &'static [Option<f32>]) -> EpsTask<FakeGauge, FakeMonitor, NoDelay> {
        EpsTask::new(
            FakeGauge { socs, reads: 0 },
            [
                rail(5.0, 0.1),
                FakeMonitor(Err(SensorFault)),
                rail(3.3, 2.0),
                rail(12.0, 0.5),
            ],
            NoDelay,
        )
    }

    #[test]
    fn reads_every_rail_in_order() {
        let r = block_on(task(&[Some(80.0)]).step());
        assert_eq!(r.rails.map(|r| r.rail), Rail::ALL);
        assert_eq!(
            r.rails[Rail::Board as usize].value,
            Some(PowerReading {
                voltage: 5.0,
                current: 0.1
            })
        );
        assert_eq!(r.rails[Rail::Gps as usize].value, None);
        // 6.6 W is over the radio's 3.3 W limit; 6 W is under the Jetson's 16 W.
        assert!(r.rails[Rail::Radio as usize].over_limit);
        assert!(!r.rails[Rail::Jetson as usize].over_limit);
    }

    #[test]
    fn reads_fuel_gauge_every_fifth_cycle() {
        let mut t = task(&[Some(80.0)]);
        let read: [bool; 11] = core::array::from_fn(|_| block_on(t.step()).battery.is_some());
        assert_eq!(
            read,
            [
                true, false, false, false, false, true, false, false, false, false, true
            ]
        );
        assert_eq!(t.fuel_gauge.reads, 3);
    }

    #[test]
    fn flag_follows_state_of_charge() {
        let mut t = task(&[Some(80.0), Some(25.0), Some(35.0), Some(45.0)]);
        let mut flags = [PowerFlag::Unknown; 4];
        for flag in &mut flags {
            *flag = block_on(t.step()).flag;
            for _ in 1..FUEL_GAUGE_EVERY {
                block_on(t.step());
            }
        }
        assert_eq!(
            flags,
            [
                PowerFlag::Nominal,
                PowerFlag::LowPower,
                PowerFlag::LowPower,
                PowerFlag::Nominal
            ]
        );
    }

    #[test]
    fn failed_or_invalid_battery_reading_keeps_flag() {
        let mut t = task(&[Some(25.0), None, Some(150.0)]);
        assert_eq!(block_on(t.step()).flag, PowerFlag::LowPower);
        for expected_battery_ok in [false, true] {
            for _ in 1..FUEL_GAUGE_EVERY {
                block_on(t.step());
            }
            let r = block_on(t.step());
            assert_eq!(r.battery.map(|b| b.is_ok()), Some(expected_battery_ok));
            assert_eq!(r.flag, PowerFlag::LowPower);
        }
    }
}
