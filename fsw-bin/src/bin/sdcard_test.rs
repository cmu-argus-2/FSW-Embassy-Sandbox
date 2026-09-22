#![no_std]
#![no_main]

use defmt::{Debug2Format, error, info};
use embassy_executor::Spawner;
use embassy_rp::Peri;
use embassy_rp::bind_interrupts;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::peripherals::{PIN_0, USB};
use embassy_rp::spi::{self, Spi};
use embassy_time::Timer;
use panic_probe as _;

use embedded_hal_bus::spi::ExclusiveDevice;
use embedded_sdmmc::{Mode, SdCard, VolumeIdx, VolumeManager};
use fsw_lib::drivers::sdcard::SdTimeSource;

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => embassy_rp::usb::InterruptHandler<USB>;
});

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    // CRITICAL: Configure clocks for 12MHz Crystal (Required for USB)
    let mut config = embassy_rp::config::Config::default();
    config.clocks = embassy_rp::clocks::ClockConfig::crystal(12_000_000);
    let p = embassy_rp::init(config);

    // Heartbeat on GP0 (NEOPIXEL data). GP25 is SCL0 on Mainboard v4, not an LED
    let _ = spawner.spawn(heartbeat(p.PIN_0));

    // USB Logger Setup
    let _ = spawner.spawn(defmtusb_wrapper(p.USB));
    Timer::after_secs(3).await;

    info!("--- SD Card Presentation Test ---");

    let mut _pwr = Output::new(p.PIN_42, Level::High);
    Timer::after_millis(200).await;

    let mut spi_config = spi::Config::default();
    spi_config.frequency = 400_000;
    let mut spi_bus = Spi::new_blocking(p.SPI1, p.PIN_10, p.PIN_11, p.PIN_12, spi_config);
    let mut cs = Output::new(p.PIN_13, Level::High);

    // 1. Initial Identity
    let init_success = {
        let spi_device = ExclusiveDevice::new(&mut spi_bus, &mut cs, embassy_time::Delay);
        let sdcard = SdCard::new(spi_device, embassy_time::Delay);
        match sdcard.num_bytes() {
            Ok(size) => {
                info!("SD Card found. Size: {} MB", (size / 1024 / 1024) as u32);
                true
            }
            Err(e) => {
                error!("Init Error: {:?}", Debug2Format(&e));
                false
            }
        }
    };

    if !init_success {
        loop {
            Timer::after_secs(1).await;
        }
    }

    // 2. High-Speed R/W Loop
    spi_bus.set_frequency(12_000_000);

    let spi_device = ExclusiveDevice::new(&mut spi_bus, &mut cs, embassy_time::Delay);
    let sdcard = SdCard::new(spi_device, embassy_time::Delay);
    let volume_mgr = VolumeManager::new(sdcard, SdTimeSource);

    match volume_mgr.open_volume(VolumeIdx(0)) {
        Ok(volume) => match volume.open_root_dir() {
            Ok(root_dir) => {
                info!("Writing PRESENT.TXT...");
                match root_dir.open_file_in_dir("PRESENT.TXT", Mode::ReadWriteCreateOrAppend) {
                    Ok(file) => {
                        let msg = "Argus Satellite: SD R/W Loop Verified.\n";
                        let _ = file.write(msg.as_bytes());
                        let _ = file.flush();
                        info!("Write successful.");
                    }
                    Err(e) => {
                        error!("Write Error: {:?}", Debug2Format(&e));
                    }
                }

                info!("Reading PRESENT.TXT back...");
                match root_dir.open_file_in_dir("PRESENT.TXT", Mode::ReadOnly) {
                    Ok(file) => {
                        let mut buffer = [0u8; 64];
                        match file.read(&mut buffer) {
                            Ok(n) => {
                                info!(
                                    "Read back {} bytes: {}",
                                    n,
                                    core::str::from_utf8(&buffer[..n]).unwrap_or("Data error")
                                );
                            }
                            Err(e) => {
                                error!("Read Error: {:?}", Debug2Format(&e));
                            }
                        }
                    }
                    Err(e) => {
                        error!("Open for Read Error: {:?}", Debug2Format(&e));
                    }
                }
            }
            Err(e) => {
                error!("Dir Error: {:?}", Debug2Format(&e));
            }
        },
        Err(e) => {
            error!("Volume Error: {:?}", Debug2Format(&e));
        }
    }

    info!("--- SD Presentation Test Complete ---");
    loop {
        Timer::after_secs(10).await;
    }
}

#[embassy_executor::task]
async fn heartbeat(pin: Peri<'static, PIN_0>) {
    let mut led = Output::new(pin, Level::Low);
    loop {
        led.set_high();
        Timer::after_millis(500).await;
        led.set_low();
        Timer::after_millis(500).await;
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
