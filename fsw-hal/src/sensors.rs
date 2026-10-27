//! Sensors. Python: `CubeSat.IMU`.

use crate::{SensorFault, Vec3};

/// Angular rate source.
#[allow(async_fn_in_trait)]
pub trait Gyro {
    /// Angular rate in rad/s, body frame.
    async fn angular_rate(&mut self) -> Result<Vec3, SensorFault>;
}
