//! Devices: the board's chips as `fsw-hal` traits.
//!
//! Each file adapts one `fsw-lib` driver to the trait apps use (e.g. [`imu::Bmx160Imu`] is a
//! `Gyro`). This is the only place that knows which chip sits behind each trait and how it is
//! configured on this board.

pub mod imu;
