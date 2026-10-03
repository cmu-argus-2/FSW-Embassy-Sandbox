//! Board support for the Argus mainboard v4 (RP2350B).
//!
//! This crate is the only place (besides the final binaries) that knows the flight software runs on
//! an RP2350. It describes the *circuit board*: which pin does what, how the buses are set up, the
//! power-up sequence, and which I2C address each device answers on. It does not contain mission
//! logic (fault policy, task rates, radio link parameters); those live in `fsw-apps`, `fsw-lib` and
//! the binaries.
//!
//! Pin assignments are taken from the CircuitPython board definition in the Python repository:
//! `FSW-mainboard/firmware/Mainboard v4/pins.c`. Bus speeds, buffer sizes and device addresses are
//! taken from `flight/hal/argus_v4.py`.
//!
//! # Usage
//!
//! ```ignore
//! let r = fsw_board::init();
//! let power = fsw_board::power::BoardPower::up(r.power).await;
//! let i2c1 = fsw_board::i2c_bus::i2c1(r.i2c1, &power.powered_up);
//! ```
//!
//! Binaries must use [`Irqs`] from this crate rather than declaring their own `bind_interrupts!`
//! for the same interrupts, otherwise the handlers are defined twice and linking fails.
#![no_std]

use assign_resources::assign_resources;
use embassy_rp::{Peri, bind_interrupts, i2c, peripherals, uart, usb};

pub mod devices;
pub mod gpio;
pub mod i2c_bus;
pub mod power;
pub mod spi_bus;
pub mod uart_bus;
#[cfg(feature = "usb-log")]
pub mod usb_log;

// Every pin and peripheral can be placed in at most one group, so the compiler rejects two drivers
// claiming the same resource. Peripherals not listed here are not available after `init()`: add a
// group when a new one is needed (e.g. FLASH for NVM flags).
assign_resources! {
    /// Power switch enables, fault outputs and main power reset (see `power`).
    power: PowerPins {
        periph_en: PIN_42,
        periph_flt: PIN_41,
        gps_en: PIN_38,
        gps_flt: PIN_39,
        radio_en: PIN_36,
        radio_flt: PIN_37,
        coil_en: PIN_1,
        main_pwr_rst: PIN_43,
    }
    /// I2C0: IMU, -X/-Y/-Z torquers and light sensors, -Y deployment sensor, burn wires.
    i2c0: I2c0Pins {
        i2c: I2C0,
        sda: PIN_24,
        scl: PIN_25,
    }
    /// I2C1: power monitors, RTC, fuel gauge, +X/+Y/+Z torquers and light sensors, +X deployment sensor.
    i2c1: I2c1Pins {
        i2c: I2C1,
        sda: PIN_46,
        scl: PIN_47,
    }
    /// SX1262 LoRa radio on SPI0.
    radio: RadioPins {
        spi: SPI0,
        clk: PIN_34,
        mosi: PIN_35,
        miso: PIN_32,
        cs: PIN_33,
        reset: PIN_19,
        busy: PIN_20,
        dio1: PIN_21,
        rx_en: PIN_22,
        tx_en: PIN_23,
        tx_dma: DMA_CH0,
        rx_dma: DMA_CH1,
    }
    /// SD card on SPI1.
    sd: SdPins {
        spi: SPI1,
        clk: PIN_10,
        mosi: PIN_11,
        miso: PIN_12,
        cs: PIN_13,
        tx_dma: DMA_CH2,
        rx_dma: DMA_CH3,
    }
    /// GPS receiver on UART0.
    gps: GpsPins {
        uart: UART0,
        tx: PIN_44,
        rx: PIN_45,
    }
    /// Jetson payload: UART1 plus its power control lines.
    payload: PayloadPins {
        uart: UART1,
        tx: PIN_8,
        rx: PIN_9,
        jetson_en: PIN_7,
        dcdc_en: PIN_18,
    }
    /// Battery heater control lines.
    heaters: HeaterPins {
        heat_en: PIN_6,
        heat0_on: PIN_4,
        heat1_on: PIN_5,
    }
    /// External hardware watchdog.
    watchdog: WatchdogPins {
        enable: PIN_15,
        wdi: PIN_2,
    }
    /// Fuel gauge alert line.
    eps: EpsPins {
        batt_alert: PIN_3,
    }
    /// Status neopixel (WS2812, needs PIO + DMA).
    status: StatusPins {
        neopixel: PIN_0,
        pio: PIO0,
        dma: DMA_CH4,
    }
    /// Pins wired on the board but not used by the flight software. Kept here so nothing claims them
    /// by accident.
    reserved: ReservedPins {
        /// Reaction wheel enable (`RW_EN`); defined in argus_v4.py but never used.
        rw_en: PIN_31,
    }
    /// USB controller (bench logging).
    usb: UsbPins {
        usb: USB,
    }
}

bind_interrupts!(pub struct Irqs {
    I2C0_IRQ => i2c::InterruptHandler<peripherals::I2C0>;
    I2C1_IRQ => i2c::InterruptHandler<peripherals::I2C1>;
    UART0_IRQ => uart::BufferedInterruptHandler<peripherals::UART0>;
    UART1_IRQ => uart::BufferedInterruptHandler<peripherals::UART1>;
    USBCTRL_IRQ => usb::InterruptHandler<peripherals::USB>;
});

/// Initialise the RP2350 and split its peripherals into the board's resource groups.
///
/// This does not touch any board hardware. Call [`power::BoardPower::up`] next.
pub fn init() -> AssignedResources {
    let p = embassy_rp::init(Default::default());
    split_resources!(p)
}
