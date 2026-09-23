use core::sync::atomic::{AtomicU32, Ordering};

use chrono::{Datelike, Timelike};
use embedded_sdmmc::{
    BlockDevice, DirEntry, Error, Mode, RawDirectory, RawFile, RawVolume, TimeSource, Timestamp,
    VolumeIdx, VolumeManager,
};

/*
 * SD Card Storage Driver
 *
 * Ported from FSW-mainboard flight/hal/drivers/sdcard.py, which mounts the card's FAT
 * filesystem once at boot. SdStorage opens the first partition (FAT16 or FAT32) and its
 * root directory once, then provides whole-file operations in the root directory.
 * File names must be 8.3 short names, e.g. "LOG00001.TXT".
 */

/// Unix time in seconds used for file timestamps; 0 means not set yet.
static UNIX_TIME: AtomicU32 = AtomicU32::new(0);

/// Used until `set_unix_time` is called: 2024-01-01 00:00:00.
const DEFAULT_TIMESTAMP: Timestamp = Timestamp {
    year_since_1970: 54,
    zero_indexed_month: 0,
    zero_indexed_day: 0,
    hours: 0,
    minutes: 0,
    seconds: 0,
};

/// FAT timestamps cannot represent years before 1980.
const FAT_MIN_YEAR: i32 = 1980;

/// Set the time used for file timestamps, e.g. from `DS3231::unix_time()` or GPS.
pub fn set_unix_time(ts: u32) {
    UNIX_TIME.store(ts, Ordering::Relaxed);
}

/// Timestamps files with the time from `set_unix_time`, or 2024-01-01 00:00:00 until
/// a time has been set.
pub struct SdTimeSource;

impl TimeSource for SdTimeSource {
    fn get_timestamp(&self) -> Timestamp {
        let ts = UNIX_TIME.load(Ordering::Relaxed);
        if ts == 0 {
            return DEFAULT_TIMESTAMP;
        }
        chrono::DateTime::from_timestamp(ts as i64, 0)
            .filter(|dt| dt.year() >= FAT_MIN_YEAR)
            .and_then(|dt| {
                Timestamp::from_calendar(
                    dt.year() as u16,
                    dt.month() as u8,
                    dt.day() as u8,
                    dt.hour() as u8,
                    dt.minute() as u8,
                    dt.second() as u8,
                )
                .ok()
            })
            .unwrap_or(DEFAULT_TIMESTAMP)
    }
}

pub struct SdStorage<D: BlockDevice, T: TimeSource> {
    mgr: VolumeManager<D, T>,
    volume: RawVolume,
    root: RawDirectory,
}

impl<D: BlockDevice, T: TimeSource> SdStorage<D, T> {
    /// Mount the first partition on the card and open its root directory.
    pub fn mount(block_device: D, time_source: T) -> Result<Self, Error<D::Error>> {
        let mgr = VolumeManager::new(block_device, time_source);
        let volume = mgr.open_raw_volume(VolumeIdx(0))?;
        let root = mgr.open_root_dir(volume)?;
        Ok(Self { mgr, volume, root })
    }

    /// Close the root directory and volume, and hand back the block device and time source.
    pub fn unmount(self) -> Result<(D, T), Error<D::Error>> {
        self.mgr.close_dir(self.root)?;
        self.mgr.close_volume(self.volume)?;
        Ok(self.mgr.free())
    }

    /// Create a file, or replace an existing file's contents, with `data`.
    pub fn write_file(&self, name: &str, data: &[u8]) -> Result<(), Error<D::Error>> {
        self.with_file(name, Mode::ReadWriteCreateOrTruncate, |file| {
            self.mgr.write(file, data)
        })
    }

    /// Append `data` to a file, creating it if needed.
    pub fn append_file(&self, name: &str, data: &[u8]) -> Result<(), Error<D::Error>> {
        self.with_file(name, Mode::ReadWriteCreateOrAppend, |file| {
            self.mgr.write(file, data)
        })
    }

    /// Read a file from the start into `buf` and return the number of bytes read.
    /// Reads less than the whole file if `buf` is smaller than the file.
    pub fn read_file(&self, name: &str, buf: &mut [u8]) -> Result<usize, Error<D::Error>> {
        self.with_file(name, Mode::ReadOnly, |file| {
            let mut total = 0;
            while total < buf.len() {
                let n = self.mgr.read(file, &mut buf[total..])?;
                if n == 0 {
                    break;
                }
                total += n;
            }
            Ok(total)
        })
    }

    /// Size, modified/created times and attributes of a file.
    pub fn metadata(&self, name: &str) -> Result<DirEntry, Error<D::Error>> {
        self.mgr.find_directory_entry(self.root, name)
    }

    pub fn exists(&self, name: &str) -> Result<bool, Error<D::Error>> {
        match self.metadata(name) {
            Ok(_) => Ok(true),
            Err(Error::NotFound) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Delete a file. embedded-sdmmc 0.9 does not free a deleted file's clusters, so the file
    /// is truncated first, which frees all but its first cluster. That one cluster stays
    /// allocated until the card is repaired with fsck, so prefer overwriting files to deleting them.
    pub fn delete_file(&self, name: &str) -> Result<(), Error<D::Error>> {
        self.with_file(name, Mode::ReadWriteTruncate, |_| Ok(()))?;
        self.mgr.delete_file_in_dir(self.root, name)
    }

    /// Open a file, run `f` on it and always close it. Returns the first error.
    fn with_file<R>(
        &self,
        name: &str,
        mode: Mode,
        f: impl FnOnce(RawFile) -> Result<R, Error<D::Error>>,
    ) -> Result<R, Error<D::Error>> {
        let file = self.mgr.open_file_in_dir(self.root, name, mode)?;
        let result = f(file);
        let closed = self.mgr.close_file(file);
        let value = result?;
        closed?;
        Ok(value)
    }
}
