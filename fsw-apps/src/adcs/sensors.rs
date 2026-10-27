//! Sensor reading checks. Ported from FSW-mainboard `flight/apps/adcs/sensors.py`.
//!
//! The reading itself is done by the task through the `fsw-hal` sensor traits.

use fsw_hal::{SensorFault, Vec3};

use super::{Reading, Status};

/// Anything above the BMX160's 2000 deg/s full scale is a faulty reading.
pub const MAX_GYRO_NORM: f32 = 2000.0 * core::f32::consts::PI / 180.0;

/// Validity check from `read_gyro`: a failed read or an implausible rate becomes `GyroFail` with a
/// zero vector.
pub fn check_gyro(raw: Result<Vec3, SensorFault>) -> Reading {
    let failed = Reading {
        status: Status::GyroFail,
        value: [0.0; 3],
    };
    match raw {
        Ok(w) => {
            let norm_sq = w[0] * w[0] + w[1] * w[1] + w[2] * w[2];
            // Written as `!(x <= max)` so NaN fails, like the Python check.
            if !(norm_sq <= MAX_GYRO_NORM * MAX_GYRO_NORM) {
                return failed;
            }
            Reading {
                status: Status::Ok,
                value: w,
            }
        }
        Err(SensorFault) => failed,
    }
}
