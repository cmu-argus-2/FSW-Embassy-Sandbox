//! ADCS task loop.
//!
//! For now each cycle reads and checks the gyro; the rest of the task (magnetometer, sun vector,
//! modes, control, coils) comes later.
//!
//! The task is generic over the `fsw-hal` traits and a delay, so it has no idea which board or chip
//! it runs on. Embassy `#[task]` functions cannot be generic, so a binary wraps it in a small task
//! with concrete types.

use fsw_hal::DelayNs;
use fsw_hal::sensors::Gyro;

use crate::adcs::Reading;
use crate::adcs::sensors::check_gyro;

/// ADCS runs at 1 Hz.
pub const PERIOD_MS: u32 = 1000;

pub struct AdcsTask<G, D> {
    gyro: G,
    delay: D,
}

impl<G: Gyro, D: DelayNs> AdcsTask<G, D> {
    pub fn new(gyro: G, delay: D) -> Self {
        Self { gyro, delay }
    }

    /// Read and check the gyro once.
    pub async fn step(&mut self) -> Reading {
        check_gyro(self.gyro.angular_rate().await)
    }

    /// Run forever: one [`Self::step`] per period, handing each reading to `on_reading` (logging,
    /// telemetry). The period is a plain delay after each step, so it drifts by the step time.
    pub async fn run(&mut self, mut on_reading: impl FnMut(&Reading)) -> ! {
        loop {
            let reading = self.step().await;
            on_reading(&reading);
            self.delay.delay_ms(PERIOD_MS).await;
        }
    }
}
