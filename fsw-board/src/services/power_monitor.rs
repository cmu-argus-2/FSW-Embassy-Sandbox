//! Power monitor service: one ADM1176 and the task that owns it. There is one ADM1176 per monitored
//! rail, so a binary runs one task per rail, each serving that rail's power channel.
//!
//! ```text
//! EPS task                           power monitor task (run), one per rail
//! ────────                           ──────────────────────────────────────
//! PowerMonitorClient::power()
//!   ├─ send PowerRequest{slot} ─────▶ receive from the rail's channel
//!   │                                 monitor.read_voltage_current().await   (I2C read)
//!   └─ wait on slot ◀──────────────── slot.signal(result)
//! ```

use defmt::{error, info};
use fsw_hal::SensorFault;
use fsw_hal::sensors::PowerReading;
use fsw_lib::drivers::adm1176::ADM1176;

use super::{PowerChannel, PowerRequest};
use crate::i2c_bus::I2c1Device;

// ── Device ──────────────────────────────────────────────────────────────────────────────────────

/// An ADM1176 on I2C1, with its address for log messages (the driver keeps its own private).
pub struct Monitor {
    driver: ADM1176<I2c1Device>,
    addr: u8,
}

/// Set up the ADM1176 at `addr` on I2C1 to convert voltage and current continuously. If that
/// fails, the driver is still returned and the task's reads report [`SensorFault`] for as long as
/// the device does not answer, so the board still boots.
pub async fn init(i2c: I2c1Device, addr: u8) -> Monitor {
    let mut driver = ADM1176::new(i2c, addr);
    match driver.config(&["V_CONT", "I_CONT"]).await {
        Ok(()) => info!("ADM1176 {=u8:#x} up", addr),
        Err(e) => error!("ADM1176 {=u8:#x} init failed: {}", addr, e),
    }
    Monitor { driver, addr }
}

// ── Service task ────────────────────────────────────────────────────────────────────────────────

/// Own one rail's monitor and answer that rail's power requests one at a time, forever.
///
/// A binary wraps this in an `#[embassy_executor::task]` (task functions must live where the
/// concrete types are known) and spawns it on whichever executor the monitors should run on.
pub async fn run(mut monitor: Monitor, requests: &'static PowerChannel) -> ! {
    loop {
        let PowerRequest { reply } = requests.receive().await;
        let result = monitor
            .driver
            .read_voltage_current()
            .await
            .map(|(voltage, current)| PowerReading { voltage, current })
            .map_err(|e| {
                error!("ADM1176 {=u8:#x} read failed: {}", monitor.addr, e);
                SensorFault
            });
        reply.signal(result);
    }
}
