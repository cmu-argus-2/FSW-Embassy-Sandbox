//! Watchdogs.

/// External hardware watchdog: resets the MCU unless it is serviced often enough.
///
/// The methods are synchronous because servicing is a pin toggle; there is nothing to wait for.
pub trait Watchdog {
    /// Service the watchdog, restarting its timeout.
    fn kick(&mut self);

    /// Connect (`true`) or disconnect (`false`) the watchdog's reset output from the MCU. While
    /// disconnected the watchdog keeps running but cannot reset anything.
    fn set_enabled(&mut self, enabled: bool);

    /// Whether the reset output is connected.
    fn is_enabled(&self) -> bool;
}
