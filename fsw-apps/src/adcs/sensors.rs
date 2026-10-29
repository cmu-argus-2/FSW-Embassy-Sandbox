//! Sensor reading checks.
//!
//! The reading itself is done by the task through the `fsw-hal` sensor traits.

use fsw_hal::{SensorFault, Vec3};

use super::{MagCalibration, Reading, Status};

/// Anything above the BMX160's 2000 deg/s full scale is a faulty reading.
pub const MAX_GYRO_NORM: f32 = 2000.0 * core::f32::consts::PI / 180.0;

/// Smallest plausible field, in T (1 µT; the field in orbit is about 40 µT).
pub const MIN_MAG_NORM: f32 = 1.0e-6;
/// Largest plausible field, in T (2500 µT; the field at sea level is about 60 µT).
pub const MAX_MAG_NORM: f32 = 2.5e-3;

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
            // Written as `!(x <= max)` so NaN fails.
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

/// Validity check from `read_magnetometer`: converts µT to T, applies the calibration, and turns a
/// failed read or a field outside [`MIN_MAG_NORM`, `MAX_MAG_NORM`] into `MagFail` with a zero
/// vector.
pub fn check_mag(raw: Result<Vec3, SensorFault>, cal: &MagCalibration) -> Reading {
    let failed = Reading {
        status: Status::MagFail,
        value: [0.0; 3],
    };
    match raw {
        Ok(ut) => {
            let mut b = [0.0; 3];
            for i in 0..3 {
                b[i] = (ut[i] * 1e-6 - cal.bias[i]) * cal.scale[i];
            }
            let norm_sq = b[0] * b[0] + b[1] * b[1] + b[2] * b[2];
            // `contains` is false for NaN, so NaN fails.
            if !(MIN_MAG_NORM * MIN_MAG_NORM..=MAX_MAG_NORM * MAX_MAG_NORM).contains(&norm_sq) {
                return failed;
            }
            Reading {
                status: Status::Ok,
                value: b,
            }
        }
        Err(SensorFault) => failed,
    }
}
