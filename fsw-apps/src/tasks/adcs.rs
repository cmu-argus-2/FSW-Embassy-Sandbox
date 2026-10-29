//! ADCS task loop.
//!
//! For now each cycle reads and checks the gyro and the magnetometer; the rest of the task (sun
//! vector, modes, control, coils) comes later.
//!
//! The task is generic over the `fsw-hal` traits and a delay, so it has no idea which board or chip
//! it runs on: the gyro and the magnetometer may be one chip (v4) or two. Embassy `#[task]` functions cannot be generic, so a binary wraps it in a small task
//! with concrete types.

use fsw_hal::DelayNs;
use fsw_hal::sensors::{Gyro, Magnetometer};

use crate::adcs::sensors::{check_gyro, check_mag};
use crate::adcs::{MagCalibration, Reading};

/// ADCS runs at 1 Hz.
pub const PERIOD_MS: u32 = 1000;

/// One cycle's checked readings.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Readings {
    /// Angular rate in rad/s.
    pub gyro: Reading,
    /// Magnetic field in T, calibrated.
    pub mag: Reading,
}

pub struct AdcsTask<G, M, D> {
    gyro: G,
    mag: M,
    mag_cal: MagCalibration,
    delay: D,
}

impl<G: Gyro, M: Magnetometer, D: DelayNs> AdcsTask<G, M, D> {
    pub fn new(gyro: G, mag: M, mag_cal: MagCalibration, delay: D) -> Self {
        Self {
            gyro,
            mag,
            mag_cal,
            delay,
        }
    }

    /// Read and check the gyro and the magnetometer once.
    pub async fn step(&mut self) -> Readings {
        let gyro = check_gyro(self.gyro.angular_rate().await);
        let mag = check_mag(self.mag.magnetic_field().await, &self.mag_cal);
        Readings { gyro, mag }
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
