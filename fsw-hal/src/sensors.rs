//! Sensors.

use crate::{SensorFault, Vec3};

/// Angular rate source.
#[allow(async_fn_in_trait)]
pub trait Gyro {
    /// Angular rate in rad/s, body frame.
    async fn angular_rate(&mut self) -> Result<Vec3, SensorFault>;
}

/// Magnetic field source.
#[allow(async_fn_in_trait)]
pub trait Magnetometer {
    /// Magnetic field in µT, body frame.
    async fn magnetic_field(&mut self) -> Result<Vec3, SensorFault>;
}

/// One power rail's voltage and current.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PowerReading {
    /// Rail voltage in V.
    pub voltage: f32,
    /// Current drawn from the rail in A.
    pub current: f32,
}

/// Voltage and current monitor on one power rail.
#[allow(async_fn_in_trait)]
pub trait PowerMonitor {
    /// The rail's voltage and current, read together.
    async fn power(&mut self) -> Result<PowerReading, SensorFault>;
}

/// Battery pack state, as reported by the fuel gauge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BatteryReading {
    /// Reported state of charge in %, 0-100.
    pub soc: f32,
    /// Reported remaining capacity in Ah.
    pub capacity: f32,
    /// Pack current in A; positive is charging.
    pub current: f32,
    /// Pack voltage in V.
    pub voltage: f32,
    /// Voltage of the lowest cell in V.
    pub midpoint_voltage: f32,
    /// Time to empty in s.
    pub time_to_empty: f32,
    /// Time to full in s.
    pub time_to_full: f32,
    /// Pack temperature in °C.
    pub temperature: f32,
    /// Thermistor 1 (AIN1) temperature in °C.
    pub temperature_ain1: f32,
    /// Thermistor 2 (AIN2) temperature in °C.
    pub temperature_ain2: f32,
    /// Fuel gauge die temperature in °C.
    pub temperature_die: f32,
}

/// Battery fuel gauge.
#[allow(async_fn_in_trait)]
pub trait FuelGauge {
    /// Every battery value, read together.
    async fn battery(&mut self) -> Result<BatteryReading, SensorFault>;
}
