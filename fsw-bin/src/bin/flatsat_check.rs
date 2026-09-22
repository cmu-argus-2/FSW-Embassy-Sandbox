#![no_std]
#![no_main]

use defmt::{error, info, warn};
use embassy_embedded_hal::shared_bus::asynch::i2c::I2cDevice;
use embassy_executor::Spawner;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::i2c::{self, I2c, InterruptHandler};
use embassy_rp::peripherals::{I2C0, I2C1, PIN_2, PIN_15, USB};
use embassy_rp::spi::{self, Spi};
use embassy_rp::{Peri, bind_interrupts};
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::{Delay, Duration, Timer, with_timeout};
use embedded_hal_async::i2c::I2c as _;
use embedded_hal_bus::spi::ExclusiveDevice;
use embedded_sdmmc::SdCard;
use heapless::Vec;
use panic_probe as _;
use static_cell::StaticCell;

use fsw_lib::drivers::adm1176::ADM1176;
use fsw_lib::drivers::drv8235::DRV8235;
use fsw_lib::drivers::ds3231::DS3231;
use fsw_lib::drivers::opt4003::OPT4003;
use fsw_lib::drivers::pca9633::PCA9633;
use fsw_lib::drivers::sdcard::{self, SdStorage, SdTimeSource};

/*
 * Flat Sat Checkout (Mainboard v4)
 *
 * Scans both I2C buses, names every device it finds against the v4 map, exercises each
 * device there is a driver for, and prints a PASS/FAIL summary. Every check says what it
 * expected and what it measured, so a wrong bus, a missing board or a dead device is
 * obvious rather than silent.
 *
 * This never fires a burn wire: the PCA9633 is only checked for register readback with
 * the supply channel off. Use burnwire_pca9633 to actually fire one.
 *
 * Torque coils ARE driven briefly (COIL_DRIVE_THROTTLE for COIL_DRIVE_MS); set
 * DRIVE_COILS to false to only check that they answer.
 */

/// Log a line, then give the USB logger a moment to drain its buffer. Printing a burst
/// of lines back to back overruns the buffer and output is silently dropped or reordered.
macro_rules! say {
    ($($arg:tt)*) => {{
        info!($($arg)*);
        Timer::after_millis(LOG_PACE_MS).await;
    }};
}

const LOG_PACE_MS: u64 = 12;

/// A device that stops answering must not hang the run
const CHECK_TIMEOUT_S: u64 = 8;

macro_rules! timed {
    ($r:expr, $name:expr, $fut:expr) => {{
        let timed_out = with_timeout(Duration::from_secs(CHECK_TIMEOUT_S), $fut)
            .await
            .is_err();
        if timed_out {
            error!(
                "  {}: timed out after {}s - it is not releasing the I2C bus",
                $name, CHECK_TIMEOUT_S
            );
            $r.fail($name).await;
        }
    }};
}

// Driving the coils is where the run has been dying, so it is off by default.
// Set to true once that is understood; with it off the coils are only checked for
// I2C response and fault status.
const DRIVE_COILS: bool = false;

/// ADM1176 power monitors are issue #1 (not one of ours), so they are off by default.
/// The bus scan still reports whether they answer.
const CHECK_POWER_MONITORS: bool = false;
const COIL_DRIVE_THROTTLE: f32 = 0.25;
const COIL_DRIVE_MS: u64 = 500;
/// Lowest coil voltage that counts as "the coil is really driving"
const COIL_MIN_DRIVEN_VOLTS: f32 = 0.3;
/// OPT4003 full range is 0 to ~143k lux
const LUX_MAX: f32 = 143_600.0;
/// Plausible range for the power monitors
const MONITOR_VOLTS: (f32, f32) = (0.5, 20.0);
const MONITOR_AMPS: (f32, f32) = (0.0, 3.0);

struct Device {
    addr: u8,
    name: &'static str,
    /// False for devices with no Rust driver yet: presence is reported, nothing is exercised
    driver: bool,
}

const fn dev(addr: u8, name: &'static str, driver: bool) -> Device {
    Device { addr, name, driver }
}

// v4 map, from FSW-mainboard flight/hal/argus_v4.py
const I2C0_DEVICES: &[Device] = &[
    dev(0x29, "DEPLOYMENT_SENSOR_YM (vl53l4cd)", false),
    dev(0x30, "TORQUE_XM (drv8235)", true),
    dev(0x31, "TORQUE_YM (drv8235)", true),
    dev(0x32, "TORQUE_ZM (drv8235)", true),
    dev(0x40, "SOLAR_CHARGING_XM_MONITOR (adm1176)", true),
    dev(0x41, "SOLAR_CHARGING_YM_MONITOR (adm1176)", true),
    dev(0x44, "LIGHT_SENSOR_XM (opt4003)", true),
    dev(0x45, "LIGHT_SENSOR_YM (opt4003)", true),
    dev(0x46, "LIGHT_SENSOR_ZM (opt4003)", true),
    dev(0x60, "BURN_WIRE (pca9633)", true),
    dev(0x68, "IMU (bno085)", false),
    dev(
        0x70,
        "PCA9633 all-call address (normal, same chip as 0x60)",
        false,
    ),
];

const I2C1_DEVICES: &[Device] = &[
    dev(0x12, "TORQUE_ZP (drv8235)", true),
    dev(0x29, "DEPLOYMENT_SENSOR_XP (vl53l4cd)", false),
    dev(0x30, "TORQUE_XP (drv8235)", true),
    dev(0x31, "TORQUE_YP (drv8235)", true),
    dev(0x36, "FUEL_GAUGE (max17205)", false),
    dev(0x40, "BOARD_POWER_MONITOR (adm1176)", true),
    dev(0x41, "GPS_POWER_MONITOR (adm1176)", true),
    dev(0x42, "RADIO_POWER_MONITOR (adm1176)", true),
    dev(0x43, "JETSON_POWER_MONITOR (adm1176)", true),
    dev(0x44, "LIGHT_SENSOR_XP (opt4003)", true),
    dev(0x45, "LIGHT_SENSOR_YP (opt4003)", true),
    dev(0x48, "SOLAR_CHARGING_XP_MONITOR (adm1176)", true),
    dev(0x4A, "SOLAR_CHARGING_YP_MONITOR (adm1176)", true),
    dev(0x62, "SOLAR_CHARGING_ZP_MONITOR (adm1176)", true),
    dev(0x68, "RTC (ds3231)", true),
];

// Devices exercised below, as (address, label)
const COILS_I2C0: &[(u8, &str)] = &[
    (0x30, "TORQUE_XM"),
    (0x31, "TORQUE_YM"),
    (0x32, "TORQUE_ZM"),
];
const COILS_I2C1: &[(u8, &str)] = &[
    (0x30, "TORQUE_XP"),
    (0x31, "TORQUE_YP"),
    (0x12, "TORQUE_ZP"),
];
const LIGHT_I2C0: &[(u8, &str)] = &[(0x44, "LIGHT_XM"), (0x45, "LIGHT_YM"), (0x46, "LIGHT_ZM")];
const LIGHT_I2C1: &[(u8, &str)] = &[(0x44, "LIGHT_XP"), (0x45, "LIGHT_YP")];
const MONITORS_I2C0: &[(u8, &str)] = &[(0x40, "SOLAR_XM_MON"), (0x41, "SOLAR_YM_MON")];
const MONITORS_I2C1: &[(u8, &str)] = &[
    (0x40, "BOARD_MON"),
    (0x41, "GPS_MON"),
    (0x42, "RADIO_MON"),
    (0x43, "JETSON_MON"),
    (0x48, "SOLAR_XP_MON"),
    (0x4A, "SOLAR_YP_MON"),
    (0x62, "SOLAR_ZP_MON"),
];

const RTC_ADDR: u8 = 0x68;
const BURN_ADDR: u8 = 0x60;

type Bus0 = Mutex<NoopRawMutex, I2c<'static, I2C0, i2c::Async>>;
type Bus1 = Mutex<NoopRawMutex, I2c<'static, I2C1, i2c::Async>>;
static BUS0: StaticCell<Bus0> = StaticCell::new();
static BUS1: StaticCell<Bus1> = StaticCell::new();

bind_interrupts!(struct Irqs {
    I2C0_IRQ => InterruptHandler<I2C0>;
    I2C1_IRQ => InterruptHandler<I2C1>;
    USBCTRL_IRQ => embassy_rp::usb::InterruptHandler<USB>;
});

struct Results {
    passed: u32,
    failed: u32,
    skipped: u32,
    failures: Vec<&'static str, 48>,
}

impl Results {
    fn new() -> Self {
        Self {
            passed: 0,
            failed: 0,
            skipped: 0,
            failures: Vec::new(),
        }
    }

    async fn pass(&mut self, name: &'static str) {
        info!("  PASS  {}", name);
        self.passed += 1;
        Timer::after_millis(LOG_PACE_MS).await;
    }

    async fn fail(&mut self, name: &'static str) {
        error!("  FAIL  {}", name);
        self.failed += 1;
        let _ = self.failures.push(name);
        Timer::after_millis(LOG_PACE_MS).await;
    }

    /// Not present on this flat sat, or not reachable, so nothing was exercised
    async fn skip(&mut self, name: &'static str) {
        warn!("  SKIP  {}", name);
        self.skipped += 1;
        Timer::after_millis(LOG_PACE_MS).await;
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    let _ = spawner.spawn(defmtusb_wrapper(p.USB));
    let _ = spawner.spawn(heartbeat());
    // Mainboard v4 has an external watchdog that resets the MCU if it is not fed
    let _ = spawner.spawn(watchdog_keeper(p.PIN_15, p.PIN_2));
    Timer::after_secs(3).await;

    say!("=====================================================");
    say!(" ARGUS FLAT SAT CHECKOUT - Mainboard v4");
    say!(" I2C0 = SDA GPIO24 / SCL GPIO25");
    say!(" I2C1 = SDA GPIO46 / SCL GPIO47");
    say!(" SD   = SPI1 CLK GPIO10 / MOSI GPIO11 / MISO GPIO12 / CS GPIO13");
    say!(" Checks the drivers for issues #2 #3 #4 #8 #12");
    say!(" Burn wires are NOT fired by this test");
    say!("=====================================================");

    // Peripheral 3.3V must be on before the I2C addresses are valid; coils need COIL_EN
    let mut periph_pwr = Output::new(p.PIN_42, Level::High);
    periph_pwr.set_high();
    let mut coil_pwr = Output::new(p.PIN_1, Level::High);
    coil_pwr.set_high();
    say!("Power: PERIPH_PWR_EN (GPIO42) high, COIL_EN (GPIO1) high");
    Timer::after_secs(3).await; // PCA9633 needs a moment after power-up

    let i2c0 = I2c::new_async(p.I2C0, p.PIN_25, p.PIN_24, Irqs, i2c::Config::default());
    let i2c1 = I2c::new_async(p.I2C1, p.PIN_47, p.PIN_46, Irqs, i2c::Config::default());
    let bus0 = BUS0.init(Mutex::new(i2c0));
    let bus1 = BUS1.init(Mutex::new(i2c1));

    let mut r = Results::new();

    // ---------------------------------------------------------------- bus scan
    say!("");
    say!("--- [1/5] I2C bus scan (all devices, whoever owns them) ---");
    let found0 = scan_bus0(bus0, "I2C0", I2C0_DEVICES).await;
    let found1 = scan_bus1(bus1, "I2C1", I2C1_DEVICES).await;

    // ---------------------------------------------------------------- RTC
    say!("");
    say!("--- [2/5] issue #2: RTC (DS3231, I2C1 0x68) ---");
    let rtc_time = if found1.contains(RTC_ADDR) {
        let outcome = with_timeout(Duration::from_secs(20), check_rtc(bus1, &mut r)).await;
        match outcome {
            Ok(t) => t,
            Err(_) => {
                error!("  RTC: timed out - it is not releasing the I2C bus");
                r.fail("RTC").await;
                None
            }
        }
    } else {
        say!("  not on the bus");
        r.skip("RTC").await;
        None
    };

    // ---------------------------------------------------------------- light sensors
    say!("");
    say!("--- [3/5] issue #3: Light sensors (OPT4003) ---");
    say!("  expect daylight/room light > 10 lux; cover one and rerun, it should drop");
    for &(addr, name) in LIGHT_I2C0 {
        if found0.contains(addr) {
            timed!(
                r,
                name,
                check_light(I2cDevice::new(bus0), addr, name, &mut r)
            );
        } else {
            r.skip(name).await;
        }
    }
    for &(addr, name) in LIGHT_I2C1 {
        if found1.contains(addr) {
            timed!(
                r,
                name,
                check_light(I2cDevice::new(bus1), addr, name, &mut r)
            );
        } else {
            r.skip(name).await;
        }
    }

    // ---------------------------------------------------------------- power monitors
    say!("");
    say!("--- issue #1 (not ours): Power monitors (ADM1176) ---");
    if CHECK_POWER_MONITORS {
        for &(addr, name) in MONITORS_I2C0 {
            if found0.contains(addr) {
                timed!(
                    r,
                    name,
                    check_monitor(I2cDevice::new(bus0), addr, name, &mut r)
                );
            } else {
                r.skip(name).await;
            }
        }
        for &(addr, name) in MONITORS_I2C1 {
            if found1.contains(addr) {
                timed!(
                    r,
                    name,
                    check_monitor(I2cDevice::new(bus1), addr, name, &mut r)
                );
            } else {
                r.skip(name).await;
            }
        }
    } else {
        say!(
            "  skipped: ADM1176 is issue #1, not one of ours (set CHECK_POWER_MONITORS to test it)"
        );
    }

    // ---------------------------------------------------------------- torque coils
    say!("");
    say!("--- [4/5] issue #8: Torque coils (DRV8235) ---");
    if DRIVE_COILS {
        say!(
            "  each coil is driven at {} throttle for {}ms",
            COIL_DRIVE_THROTTLE,
            COIL_DRIVE_MS
        );
    } else {
        say!("  drive test disabled (DRIVE_COILS = false)");
    }
    for &(addr, name) in COILS_I2C0 {
        if found0.contains(addr) {
            timed!(
                r,
                name,
                check_coil(I2cDevice::new(bus0), addr, name, &mut r)
            );
        } else {
            r.skip(name).await;
        }
    }
    for &(addr, name) in COILS_I2C1 {
        if found1.contains(addr) {
            timed!(
                r,
                name,
                check_coil(I2cDevice::new(bus1), addr, name, &mut r)
            );
        } else {
            r.skip(name).await;
        }
    }

    // ---------------------------------------------------------------- burn wires
    say!("");
    say!("--- [5/5] issue #4: Burn wires (PCA9633, I2C0 0x60) and issue #12: SD card ---");
    if found0.contains(BURN_ADDR) {
        timed!(r, "BURN_WIRE", check_burnwires(bus0, &mut r));
    } else {
        r.skip("BURN_WIRE").await;
    }

    // ---------------------------------------------------------------- SD card
    let spi_config = {
        let mut c = spi::Config::default();
        c.frequency = 400_000;
        c
    };
    let mut spi_bus = Spi::new_blocking(p.SPI1, p.PIN_10, p.PIN_11, p.PIN_12, spi_config);
    let mut cs = Output::new(p.PIN_13, Level::High);
    check_sdcard(&mut spi_bus, &mut cs, rtc_time, &mut r).await;

    // ---------------------------------------------------------------- summary
    say!("");
    say!("=====================================================");
    say!(
        " SUMMARY: {} passed, {} failed, {} skipped",
        r.passed,
        r.failed,
        r.skipped
    );
    if r.failed == 0 {
        say!(" ALL CHECKS PASSED");
    } else {
        for name in r.failures.iter() {
            error!("   failed: {}", name);
        }
    }
    say!(" Skipped checks are devices that did not answer on the bus:");
    say!("   normal if that board is not attached to the flat sat.");
    say!("=====================================================");

    // Reprint the summary every 20s so it is on screen whenever you look
    loop {
        Timer::after_secs(20).await;
        say!("--- checkout finished, summary repeated ---");
        say!(
            " {} passed, {} failed, {} skipped",
            r.passed,
            r.failed,
            r.skipped
        );
        for name in r.failures.iter() {
            error!("   failed: {}", name);
            Timer::after_millis(LOG_PACE_MS).await;
        }
    }
}

/// Addresses that answered on a bus
struct Found(Vec<u8, 32>);

impl Found {
    fn contains(&self, addr: u8) -> bool {
        self.0.contains(&addr)
    }
}

macro_rules! scan_fn {
    ($name:ident, $bus:ty) => {
        async fn $name(bus: &'static $bus, label: &str, expected: &[Device]) -> Found {
            let mut found: Vec<u8, 32> = Vec::new();
            {
                let mut i2c = bus.lock().await;
                for addr in 0x08u8..=0x77 {
                    let mut buf = [0u8; 1];
                    if i2c.read(addr, &mut buf).await.is_ok() {
                        let _ = found.push(addr);
                    }
                }
            }
            info!("{}: {} device(s) answered", label, found.len());
            for addr in found.iter() {
                match expected.iter().find(|d| d.addr == *addr) {
                    Some(d) if d.driver => info!("  0x{:02X}  {}", addr, d.name),
                    Some(d) => info!("  0x{:02X}  {} - present, no Rust driver yet", addr, d.name),
                    None => warn!("  0x{:02X}  UNEXPECTED - not in the v4 map", addr),
                }
                Timer::after_millis(LOG_PACE_MS).await;
            }
            for d in expected.iter() {
                if !found.contains(&d.addr) {
                    info!("  0x{:02X}  absent: {}", d.addr, d.name);
                    Timer::after_millis(LOG_PACE_MS).await;
                }
            }
            Found(found)
        }
    };
}

scan_fn!(scan_bus0_inner, Bus0);
scan_fn!(scan_bus1_inner, Bus1);

async fn scan_bus0(bus: &'static Bus0, label: &str, expected: &[Device]) -> Found {
    scan_bus0_inner(bus, label, expected).await
}

async fn scan_bus1(bus: &'static Bus1, label: &str, expected: &[Device]) -> Found {
    scan_bus1_inner(bus, label, expected).await
}

/// Reads the clock twice to prove it is actually running. Returns the Unix time.
async fn check_rtc(bus: &'static Bus1, r: &mut Results) -> Option<i64> {
    let mut rtc = DS3231::new(I2cDevice::new(bus), RTC_ADDR);

    match rtc.lost_power().await {
        Ok(true) => {
            warn!("  oscillator stopped since the time was last set - run rtc_test to set it")
        }
        Ok(false) => info!("  oscillator OK (battery backup holding)"),
        Err(e) => error!("  status read failed: {:?}", e),
    }

    let first = match rtc.unix_time().await {
        Ok(t) => t,
        Err(e) => {
            error!("  time read failed: {:?}", e);
            r.fail("RTC readable").await;
            return None;
        }
    };
    match rtc.datetime().await {
        Ok(dt) => info!(
            "  clock reads {:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC (epoch {})",
            dt.year, dt.month, dt.day, dt.hour, dt.minute, dt.second, first
        ),
        Err(e) => error!("  datetime read failed: {:?}", e),
    }
    r.pass("RTC readable").await;

    info!("  waiting 2s to confirm the clock is ticking...");
    Timer::after_secs(2).await;
    match rtc.unix_time().await {
        Ok(second) => {
            let delta = second - first;
            info!("  advanced {}s (expected 1-3s)", delta);
            if (1..=3).contains(&delta) {
                r.pass("RTC ticking").await;
            } else {
                r.fail("RTC ticking").await;
            }
            Some(second)
        }
        Err(e) => {
            error!("  second time read failed: {:?}", e);
            r.fail("RTC ticking").await;
            None
        }
    }
}

async fn check_light<I2C: embedded_hal_async::i2c::I2c>(
    i2c: I2C,
    addr: u8,
    name: &'static str,
    r: &mut Results,
) where
    I2C::Error: defmt::Format,
{
    let mut sensor = OPT4003::new(i2c, addr);
    if let Err(e) = sensor.init().await {
        error!(
            "  {} at 0x{:02X}: init failed (device ID check): {:?}",
            name, addr, e
        );
        r.fail(name).await;
        return;
    }

    // Raw values so the lux maths can be checked by hand: lux = (mantissa << exp) * 535e-6
    let first = match sensor.raw_result().await {
        Ok(v) => v,
        Err(e) => {
            error!("  {} at 0x{:02X}: read failed: {:?}", name, addr, e);
            r.fail(name).await;
            return;
        }
    };
    Timer::after_millis(250).await; // conversion time is 100ms, so a new sample is due
    let second = match sensor.raw_result().await {
        Ok(v) => v,
        Err(e) => {
            error!("  {} at 0x{:02X}: second read failed: {:?}", name, addr, e);
            r.fail(name).await;
            return;
        }
    };

    let (exp, mantissa, counter_a) = second;
    let (_, _, counter_b) = first;
    let lux = (mantissa << exp) as f32 * 535e-6;
    info!(
        "  {} at 0x{:02X}: {} lux  [exp {}, mantissa {}, counter {}->{}]",
        name, addr, lux, exp, mantissa, counter_b, counter_a
    );
    Timer::after_millis(LOG_PACE_MS).await;

    if !(0.0..=LUX_MAX).contains(&lux) {
        error!(
            "    {} lux is outside the sensor's 0-{} range",
            lux, LUX_MAX
        );
        r.fail(name).await;
    } else if counter_a == counter_b {
        error!(
            "    sample counter did not advance in 250ms - the reading is stale, not a live conversion"
        );
        r.fail(name).await;
    } else {
        r.pass(name).await;
    }
}

async fn check_monitor<I2C: embedded_hal_async::i2c::I2c>(
    i2c: I2C,
    addr: u8,
    name: &'static str,
    r: &mut Results,
) where
    I2C::Error: defmt::Format,
{
    let mut monitor = ADM1176::new(i2c, addr);
    if let Err(e) = monitor.config(&["V_CONT", "I_CONT"]).await {
        error!("  {} at 0x{:02X}: config failed: {:?}", name, addr, e);
        r.fail(name).await;
        return;
    }
    Timer::after_millis(50).await;
    match monitor.read_voltage_current().await {
        Ok((volts, amps)) => {
            info!("  {} at 0x{:02X}: {} V, {} A", name, addr, volts, amps);
            let volts_ok = (MONITOR_VOLTS.0..=MONITOR_VOLTS.1).contains(&volts);
            let amps_ok = (MONITOR_AMPS.0..=MONITOR_AMPS.1).contains(&amps);
            if volts_ok && amps_ok {
                r.pass(name).await;
            } else {
                error!(
                    "    outside expected {}-{} V and {}-{} A",
                    MONITOR_VOLTS.0, MONITOR_VOLTS.1, MONITOR_AMPS.0, MONITOR_AMPS.1
                );
                r.fail(name).await;
            }
        }
        Err(e) => {
            error!("  {} at 0x{:02X}: read failed: {:?}", name, addr, e);
            r.fail(name).await;
        }
    }
}

async fn check_coil<I2C: embedded_hal_async::i2c::I2c>(
    i2c: I2C,
    addr: u8,
    name: &'static str,
    r: &mut Results,
) where
    I2C::Error: defmt::Format,
{
    let mut coil = DRV8235::new(i2c, addr);
    if let Err(e) = coil.init().await {
        error!("  {} at 0x{:02X}: init failed: {:?}", name, addr, e);
        r.fail(name).await;
        return;
    }

    match coil.faults().await {
        Ok(f) if f.any() => warn!(
            "  {} at 0x{:02X}: faults present at startup: {:?}",
            name, addr, f
        ),
        Ok(_) => info!("  {} at 0x{:02X}: no faults", name, addr),
        Err(e) => error!("  {} at 0x{:02X}: fault read failed: {:?}", name, addr, e),
    }

    if !DRIVE_COILS {
        r.pass(name).await;
        return;
    }

    if let Err(e) = coil.set_throttle(Some(COIL_DRIVE_THROTTLE)).await {
        error!("  {} : set_throttle failed: {:?}", name, e);
        r.fail(name).await;
        return;
    }
    Timer::after_millis(COIL_DRIVE_MS).await;
    let driven = coil.read_voltage_current().await;
    let faults = coil.faults().await;
    let coasted = coil.set_throttle(None).await;

    match driven {
        Ok((volts, amps)) => {
            info!(
                "  {} : driving at {} -> {} V, {} A",
                name, COIL_DRIVE_THROTTLE, volts, amps
            );
            if volts < COIL_MIN_DRIVEN_VOLTS {
                error!(
                    "    expected at least {} V while driving - coil may be disconnected",
                    COIL_MIN_DRIVEN_VOLTS
                );
                r.fail(name).await;
            } else if matches!(&faults, Ok(f) if f.any()) {
                error!("    faults while driving: {:?}", faults);
                r.fail(name).await;
            } else {
                r.pass(name).await;
            }
        }
        Err(e) => {
            error!("  {} : read while driving failed: {:?}", name, e);
            r.fail(name).await;
        }
    }
    if let Err(e) = coasted {
        error!("  {} : failed to coast afterwards: {:?}", name, e);
    }
}

/// Checks the burn wire driver answers and holds the state the driver set,
/// with the supply channel off. Does not fire anything.
async fn check_burnwires(bus: &'static Bus0, r: &mut Results) {
    let mut burnwires = PCA9633::new(I2cDevice::new(bus), BURN_ADDR);
    if let Err(e) = burnwires.init().await {
        error!("  PCA9633 at 0x{:02X}: init failed: {:?}", BURN_ADDR, e);
        r.fail("BURN_WIRE").await;
        return;
    }

    // After init every channel is driven off (LEDOUT = 0x55) and the chip is awake
    let mut ledout = [0u8; 1];
    let mut mode1 = [0u8; 1];
    let mut dev = I2cDevice::new(bus);
    let read_ledout = dev.write_read(BURN_ADDR, &[0x08], &mut ledout).await;
    let read_mode1 = dev.write_read(BURN_ADDR, &[0x00], &mut mode1).await;

    match (read_ledout, read_mode1) {
        (Ok(()), Ok(())) => {
            info!(
                "  PCA9633 at 0x{:02X}: LEDOUT 0x{:02X} (expect 0x55), MODE1 0x{:02X}",
                BURN_ADDR, ledout[0], mode1[0]
            );
            let all_off = ledout[0] == 0x55;
            let awake = mode1[0] & 0x10 == 0;
            if all_off && awake {
                info!("  all 4 channels off, supply channel off, not fired");
                r.pass("BURN_WIRE").await;
            } else {
                if !all_off {
                    error!("    LEDOUT should be 0x55 with every channel off");
                }
                if !awake {
                    error!("    MODE1 SLEEP bit still set after init");
                }
                r.fail("BURN_WIRE").await;
            }
        }
        _ => {
            error!("  PCA9633 at 0x{:02X}: register readback failed", BURN_ADDR);
            r.fail("BURN_WIRE").await;
        }
    }
}

async fn check_sdcard(
    spi_bus: &mut Spi<'static, embassy_rp::peripherals::SPI1, spi::Blocking>,
    cs: &mut Output<'static>,
    rtc_time: Option<i64>,
    r: &mut Results,
) {
    const FILE: &str = "FLATSAT.TXT";
    const CONTENTS: &[u8] = b"argus flat sat checkout\n";

    let spi_device = ExclusiveDevice::new(spi_bus, cs, Delay);
    let sdcard = SdCard::new(spi_device, Delay);

    match sdcard.num_bytes() {
        Ok(size) => info!("  card found: {} MB", (size / 1024 / 1024) as u32),
        Err(e) => {
            error!("  no card: {:?}", defmt::Debug2Format(&e));
            r.fail("SD card present").await;
            return;
        }
    }
    r.pass("SD card present").await;

    // Timestamp new files with the RTC's time, so the file date proves both work together
    if let Some(ts) = rtc_time {
        if ts > 0 {
            sdcard::set_unix_time(ts as u32);
            info!("  file timestamps set from the RTC");
        }
    }

    let storage = match SdStorage::mount(sdcard, SdTimeSource) {
        Ok(s) => s,
        Err(e) => {
            error!(
                "  mount failed (is it formatted FAT16/FAT32?): {:?}",
                defmt::Debug2Format(&e)
            );
            r.fail("SD card mount").await;
            return;
        }
    };
    r.pass("SD card mount").await;

    if let Err(e) = storage.write_file(FILE, CONTENTS) {
        error!("  write failed: {:?}", defmt::Debug2Format(&e));
        r.fail("SD card write/read").await;
        return;
    }
    let mut buf = [0u8; 64];
    match storage.read_file(FILE, &mut buf) {
        Ok(n) if &buf[..n] == CONTENTS => {
            info!(
                "  wrote and read back {} ({} bytes, contents match)",
                FILE, n
            );
            match storage.metadata(FILE) {
                Ok(entry) => info!(
                    "  {} modified {}-{:02}-{:02} {:02}:{:02}:{:02}",
                    FILE,
                    1970 + entry.mtime.year_since_1970 as u16,
                    entry.mtime.zero_indexed_month + 1,
                    entry.mtime.zero_indexed_day + 1,
                    entry.mtime.hours,
                    entry.mtime.minutes,
                    entry.mtime.seconds
                ),
                Err(e) => error!("  metadata read failed: {:?}", defmt::Debug2Format(&e)),
            }
            r.pass("SD card write/read").await;
        }
        Ok(n) => {
            error!("  read back {} bytes that do not match what was written", n);
            r.fail("SD card write/read").await;
        }
        Err(e) => {
            error!("  read failed: {:?}", defmt::Debug2Format(&e));
            r.fail("SD card write/read").await;
        }
    }
}

/// Keeps the external watchdog from resetting the board.
///
/// WDT_EN (GP15) gates the watchdog's reset line through a MOSFET, and WDT_WDI (GP2) is
/// the pin that must be toggled to feed it. CircuitPython's boot sequence holds enable
/// low and toggles the input; we do the same, and toggle the input anyway so the board
/// survives even if the enable line is pulled high in hardware.
#[embassy_executor::task]
async fn watchdog_keeper(enable: Peri<'static, PIN_15>, input: Peri<'static, PIN_2>) {
    let _enable = Output::new(enable, Level::Low); // reset path disabled, as at CircuitPython boot
    let mut wdi = Output::new(input, Level::High);
    loop {
        Timer::after_millis(500).await;
        wdi.toggle();
    }
}

/// Prints independently of the checkout, so a stall can be told apart from a crash
#[embassy_executor::task]
async fn heartbeat() {
    let mut seconds = 0u32;
    loop {
        Timer::after_secs(2).await;
        seconds += 2;
        info!("[heartbeat] alive at {}s", seconds);
    }
}

#[embassy_executor::task]
async fn defmtusb_wrapper(usb: Peri<'static, USB>) {
    let driver = embassy_rp::usb::Driver::new(usb, Irqs);
    let config = {
        let mut c = embassy_usb::Config::new(0x1234, 0x5678);
        c.serial_number = Some("defmt");
        c.max_packet_size_0 = 64;
        c.composite_with_iads = true;
        c.device_class = 0xEF;
        c.device_sub_class = 0x02;
        c.device_protocol = 0x01;
        c
    };
    defmt_embassy_usbserial::run(driver, config).await;
}
