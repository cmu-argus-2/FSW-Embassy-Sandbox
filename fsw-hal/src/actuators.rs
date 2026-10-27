//! Actuators. Python: `CubeSat.TORQUE_DRIVERS`, `TORQUE_DRIVERS_AVAILABLE`,
//! `APPLY_MAGNETIC_CONTROL`, and `turn_on_device`/`turn_off_device` for the coil power line.

/// Magnetorquer coils, in the order Python's `_MCM_FACES` uses: two coils per body axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum Coil {
    Xp,
    Xm,
    Yp,
    Ym,
    Zp,
    Zm,
}

/// An actuator command could not be applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActuatorFault;

/// The satellite's magnetorquer coils and their shared power line.
#[allow(async_fn_in_trait)]
pub trait Magnetorquers {
    /// Whether each coil (in [`Coil`] order) can be commanded: present, not dead, and powered.
    fn available(&self) -> [bool; 6];

    /// Drive one coil. `throttle` is -1.0 (full reverse) to 1.0 (full forward) of the coil's rated
    /// voltage; `0.0` brakes. The Python FSW drives both coils of an axis with the same value, so
    /// a positive throttle is taken to push the dipole toward +axis for both (`Xp` and `Xm` both
    /// toward +X). Check this against the coil wiring during hardware testing.
    async fn set_throttle(&mut self, coil: Coil, throttle: f32) -> Result<(), ActuatorFault>;

    /// Cut power to the coil drivers (no quiescent draw). No coil is available until
    /// [`Self::power_on`].
    async fn power_off(&mut self);

    /// Restore power and re-initialise the drivers.
    async fn power_on(&mut self);
}
