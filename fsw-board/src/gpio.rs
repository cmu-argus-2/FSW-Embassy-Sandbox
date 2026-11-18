//! Simple GPIO-controlled hardware: external watchdog, battery heaters, fuel gauge alert.

use embassy_rp::gpio::{Input, Level, Output, Pull};

use crate::{EpsPins, HeaterPins, WatchdogPins};

/// External hardware watchdog (`WDT_EN`, `WDT_WDI`).
///
/// Enable is active high. The "WDT_ON" CircuitPython firmware variant drives `WDT_EN` high in
/// `board_init()`, before any Python runs, and the Python watchdog driver also enables it when it
/// is constructed. A flight binary should create this as early as possible with `enabled = true`
/// and start kicking it; bench binaries that never kick it should pass `false`.
pub struct Watchdog {
    enable: Output<'static>,
    wdi: Output<'static>,
}

impl Watchdog {
    pub fn new(r: WatchdogPins, enabled: bool) -> Self {
        let level = if enabled { Level::High } else { Level::Low };
        Self {
            enable: Output::new(r.enable, level),
            wdi: Output::new(r.wdi, Level::Low),
        }
    }

    /// Service the watchdog: pulse `WDI` low then high, as the Python watchdog task does.
    pub fn kick(&mut self) {
        self.wdi.set_low();
        self.wdi.set_high();
    }

    /// Enable or disable the watchdog.
    ///
    /// The Python driver disables it by releasing the pin (`deinit()`, leaving it floating) rather
    /// than driving it low. Driving low should be equivalent, but confirm with the hardware team.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enable
            .set_level(if enabled { Level::High } else { Level::Low });
    }

    pub fn is_enabled(&self) -> bool {
        self.enable.is_set_high()
    }
}

/// Battery heater control lines, all active high and starting off (as in batteryheaters.py).
pub struct HeaterHw {
    pub enable: Output<'static>,
    pub heater0: Output<'static>,
    pub heater1: Output<'static>,
}

pub fn heaters(r: HeaterPins) -> HeaterHw {
    HeaterHw {
        enable: Output::new(r.heat_en, Level::Low),
        heater0: Output::new(r.heat0_on, Level::Low),
        heater1: Output::new(r.heat1_on, Level::Low),
    }
}

/// Fuel gauge alert line (`BATT_ALRT`).
///
/// The MAX17205 alert output is open-drain and active low, so the internal pull-up is enabled.
/// Not used by the Python flight software.
pub fn battery_alert(r: EpsPins) -> Input<'static> {
    Input::new(r.batt_alert, Pull::Up)
}
