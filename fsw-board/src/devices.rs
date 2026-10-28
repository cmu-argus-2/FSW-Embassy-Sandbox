//! I2C device addresses, by bus (from `ArgusV4Components` in `flight/hal/argus_v4.py`).
//!
//! Each bus has an `ALL` list, used for the compile-time duplicate check below and by bring-up
//! tools (e.g. an I2C scan can report expected devices that did not answer).

/// Devices on I2C0.
pub mod i2c0 {
    pub const IMU: u8 = 0x68;
    pub const TORQUE_XM: u8 = 0x30;
    pub const TORQUE_YM: u8 = 0x31;
    pub const TORQUE_ZM: u8 = 0x33;
    pub const LIGHT_XM: u8 = 0x44;
    pub const LIGHT_YM: u8 = 0x45;
    pub const LIGHT_ZM: u8 = 0x46;
    pub const DEPLOYMENT_YM: u8 = 0x29;
    pub const BURN_WIRES: u8 = 0x60;
    /// Solar charging power monitors: present on the board but not used by the Python flight software.
    pub const SOLAR_PWR_XM: u8 = 0x40;
    pub const SOLAR_PWR_YM: u8 = 0x41;

    pub const ALL: &[(&str, u8)] = &[
        ("IMU", IMU),
        ("TORQUE_XM", TORQUE_XM),
        ("TORQUE_YM", TORQUE_YM),
        ("TORQUE_ZM", TORQUE_ZM),
        ("LIGHT_XM", LIGHT_XM),
        ("LIGHT_YM", LIGHT_YM),
        ("LIGHT_ZM", LIGHT_ZM),
        ("DEPLOYMENT_YM", DEPLOYMENT_YM),
        ("BURN_WIRES", BURN_WIRES),
        ("SOLAR_PWR_XM", SOLAR_PWR_XM),
        ("SOLAR_PWR_YM", SOLAR_PWR_YM),
    ];
}

/// Devices on I2C1.
pub mod i2c1 {
    pub const BOARD_PWR: u8 = 0x40;
    pub const GPS_PWR: u8 = 0x41;
    pub const RADIO_PWR: u8 = 0x42;
    pub const JETSON_PWR: u8 = 0x46;
    pub const RTC: u8 = 0x68;
    /// The MAX17205 fuel gauge answers on two addresses.
    pub const FUEL_GAUGE_1: u8 = 0x36;
    pub const FUEL_GAUGE_2: u8 = 0x0B;
    pub const TORQUE_XP: u8 = 0x30;
    pub const TORQUE_YP: u8 = 0x31;
    pub const TORQUE_ZP: u8 = 0x33;
    pub const LIGHT_XP: u8 = 0x44;
    pub const LIGHT_YP: u8 = 0x45;
    pub const LIGHT_ZP_XP: u8 = 0x54;
    pub const LIGHT_ZP_YM: u8 = 0x55;
    pub const LIGHT_ZP_XM: u8 = 0x56;
    pub const LIGHT_ZP_YP: u8 = 0x57;
    pub const DEPLOYMENT_XP: u8 = 0x29;
    /// Solar charging power monitors: present on the board but not used by the Python flight software.
    pub const SOLAR_PWR_XP: u8 = 0x48;
    pub const SOLAR_PWR_YP: u8 = 0x4A;
    pub const SOLAR_PWR_ZP: u8 = 0x49;

    pub const ALL: &[(&str, u8)] = &[
        ("BOARD_PWR", BOARD_PWR),
        ("GPS_PWR", GPS_PWR),
        ("RADIO_PWR", RADIO_PWR),
        ("JETSON_PWR", JETSON_PWR),
        ("RTC", RTC),
        ("FUEL_GAUGE_1", FUEL_GAUGE_1),
        ("FUEL_GAUGE_2", FUEL_GAUGE_2),
        ("TORQUE_XP", TORQUE_XP),
        ("TORQUE_YP", TORQUE_YP),
        ("TORQUE_ZP", TORQUE_ZP),
        ("LIGHT_XP", LIGHT_XP),
        ("LIGHT_YP", LIGHT_YP),
        ("LIGHT_ZP_XP", LIGHT_ZP_XP),
        ("LIGHT_ZP_YM", LIGHT_ZP_YM),
        ("LIGHT_ZP_XM", LIGHT_ZP_XM),
        ("LIGHT_ZP_YP", LIGHT_ZP_YP),
        ("DEPLOYMENT_XP", DEPLOYMENT_XP),
        ("SOLAR_PWR_XP", SOLAR_PWR_XP),
        ("SOLAR_PWR_YP", SOLAR_PWR_YP),
        ("SOLAR_PWR_ZP", SOLAR_PWR_ZP),
    ];
}

/// Returns true if two devices in `list` share an address.
const fn has_duplicate_address(list: &[(&str, u8)]) -> bool {
    let mut i = 0;
    while i < list.len() {
        let mut j = i + 1;
        while j < list.len() {
            if list[i].1 == list[j].1 {
                return true;
            }
            j += 1;
        }
        i += 1;
    }
    false
}

const _: () = assert!(
    !has_duplicate_address(i2c0::ALL),
    "two devices share an address on I2C0"
);
const _: () = assert!(
    !has_duplicate_address(i2c1::ALL),
    "two devices share an address on I2C1"
);

/// Name of the expected device at `addr` on a bus, if any.
pub fn expected(bus: &[(&'static str, u8)], addr: u8) -> Option<&'static str> {
    bus.iter().find(|(_, a)| *a == addr).map(|(name, _)| *name)
}
