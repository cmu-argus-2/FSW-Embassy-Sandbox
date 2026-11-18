//! Fuel gauge service: the MAX17205 on the battery board and the task that owns it.
//!
//! ```text
//! EPS task                           fuel gauge task (run)
//! ────────                           ─────────────────────
//! FuelGaugeClient::battery()
//!   ├─ send BatteryRequest{slot} ───▶ receive from BATTERY channel
//!   │                                 read every register              (I2C reads)
//!   └─ wait on slot ◀──────────────── slot.signal(result)
//! ```
//!
//! The driver reports mV, mA, mAh and centi-°C; the service converts to the `fsw-hal` units (V, A,
//! Ah, °C).

use defmt::{error, info};
use embassy_embedded_hal::shared_bus::I2cDeviceError;
use embassy_rp::i2c;
use fsw_hal::SensorFault;
use fsw_hal::sensors::BatteryReading;
use fsw_lib::drivers::max17205::MAX17205;

use super::{BatteryChannel, BatteryRequest};
use crate::addresses;
use crate::i2c_bus::I2c1Device;

// ── Device ──────────────────────────────────────────────────────────────────────────────────────

/// The MAX17205 on I2C1.
pub type Gauge = MAX17205<I2c1Device>;

/// Set up the MAX17205 on I2C1. It needs no configuration, so this only checks that it answers. If
/// it does not, the driver is still returned and every read reports [`SensorFault`], so the
/// board still boots.
pub async fn init(i2c: I2c1Device) -> Gauge {
    let mut gauge = MAX17205::new(
        i2c,
        addresses::i2c1::FUEL_GAUGE_1,
        addresses::i2c1::FUEL_GAUGE_2,
    );
    match gauge.read_soc().await {
        Ok(soc) => info!("MAX17205 up, SOC {=f32} %", soc),
        Err(e) => error!("MAX17205 init failed: {}", e),
    }
    gauge
}

// ── Service task ────────────────────────────────────────────────────────────────────────────────

/// Own the fuel gauge and answer battery requests one at a time, forever.
///
/// A binary wraps this in an `#[embassy_executor::task]` (task functions must live where the
/// concrete types are known) and spawns it on whichever executor the fuel gauge should run on.
pub async fn run(mut gauge: Gauge, requests: &'static BatteryChannel) -> ! {
    loop {
        let BatteryRequest { reply } = requests.receive().await;
        let result = read(&mut gauge).await.map_err(|e| {
            error!("fuel gauge read failed: {}", e);
            SensorFault
        });
        reply.signal(result);
    }
}

/// Read every battery value, stopping at the first failed read.
async fn read(gauge: &mut Gauge) -> Result<BatteryReading, I2cDeviceError<i2c::Error>> {
    Ok(BatteryReading {
        soc: gauge.read_soc().await?,
        capacity: gauge.read_capacity().await? / 1000.0,
        current: gauge.read_current().await? / 1000.0,
        voltage: gauge.read_voltage().await? / 1000.0,
        midpoint_voltage: gauge.read_midvoltage().await? / 1000.0,
        time_to_empty: gauge.read_tte().await?,
        time_to_full: gauge.read_ttf().await?,
        temperature: gauge.read_temperature().await? / 100.0,
        temperature_ain1: gauge.read_temperature_ain1().await? / 100.0,
        temperature_ain2: gauge.read_temperature_ain2().await? / 100.0,
        temperature_die: gauge.read_temperature_die().await? / 100.0,
    })
}
