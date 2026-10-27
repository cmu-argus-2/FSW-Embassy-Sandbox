//! Boot power-up sequence.
//!
//! Ported from `ArgusV4Power` in `flight/hal/argus_v4.py`. In Python the sequence ran as a side
//! effect of importing the module; here it is the explicit async function [`BoardPower::up`], and
//! the bus constructors require the [`PoweredUp`] proof it returns, so a bus cannot be created
//! before its devices have power.

use embassy_rp::gpio::{Level, Output};
use embassy_time::Timer;

use crate::PowerPins;

/// Time for the 3.3 V peripheral line to settle before I2C access (argus_v4.py: `time.sleep(0.1)`).
pub const PERIPH_SETTLE_MS: u64 = 100;

/// Proof that [`BoardPower::up`] has run. Bus and device constructors take `&PoweredUp`.
pub struct PoweredUp {
    _private: (),
}

/// The power line enable outputs, held after the boot sequence.
///
/// Keep this alive for the whole program: dropping an embassy-rp `Output` resets the pin, which
/// would switch the line off.
pub struct BoardPower {
    _periph: Output<'static>,
    _gps: Output<'static>,
    _radio: Output<'static>,
    _coils: Output<'static>,
    /// `MAIN_PWR_RST`: high cuts main power to the MCU, so it is held low.
    _main_reset: Output<'static>,
    pub powered_up: PoweredUp,
}

impl BoardPower {
    /// Boot power-up sequence, in the same order as `ArgusV4Power`: peripheral line on, settle
    /// 100 ms, then GPS, radio and coil lines on.
    ///
    /// One deliberate difference: the main power reset line is driven low first, instead of being
    /// left unconfigured until after the settle delay.
    pub async fn up(r: PowerPins) -> Self {
        let main_reset = Output::new(r.main_pwr_rst, Level::Low);

        let periph = Output::new(r.periph_en, Level::High);
        Timer::after_millis(PERIPH_SETTLE_MS).await;

        let gps = Output::new(r.gps_en, Level::High);
        let radio = Output::new(r.radio_en, Level::High);
        let coils = Output::new(r.coil_en, Level::High);

        Self {
            _periph: periph,
            _gps: gps,
            _radio: radio,
            _coils: coils,
            _main_reset: main_reset,
            powered_up: PoweredUp { _private: () },
        }
    }
}
