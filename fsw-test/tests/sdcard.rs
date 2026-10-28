use std::cell::RefCell;
use std::convert::Infallible;

use embedded_sdmmc::{Block, BlockCount, BlockDevice, BlockIdx, Error, Timestamp};
use fsw_lib::drivers::sdcard::{self, SdStorage, SdTimeSource};

/// In-memory SD card.
struct RamDisk(RefCell<Vec<[u8; Block::LEN]>>);

impl BlockDevice for RamDisk {
    type Error = Infallible;

    fn read(&self, blocks: &mut [Block], start: BlockIdx) -> Result<(), Infallible> {
        let disk = self.0.borrow();
        for (i, block) in blocks.iter_mut().enumerate() {
            block.contents = disk[start.0 as usize + i];
        }
        Ok(())
    }

    fn write(&self, blocks: &[Block], start: BlockIdx) -> Result<(), Infallible> {
        let mut disk = self.0.borrow_mut();
        for (i, block) in blocks.iter().enumerate() {
            disk[start.0 as usize + i] = block.contents;
        }
        Ok(())
    }

    fn num_blocks(&self) -> Result<BlockCount, Infallible> {
        Ok(BlockCount(self.0.borrow().len() as u32))
    }
}

const DISK_BLOCKS: u32 = 32_768; // 16 MiB
const PART_START: u32 = 2048;
const PART_BLOCKS: u32 = DISK_BLOCKS - PART_START;
const FAT_BLOCKS: u16 = 120;
const ROOT_ENTRIES: u16 = 512;

fn blank_disk() -> RamDisk {
    RamDisk(RefCell::new(vec![[0u8; Block::LEN]; DISK_BLOCKS as usize]))
}

/// A 16 MiB card with an MBR and one empty FAT16 partition
/// (1 block per cluster, 2 FATs of 120 blocks, 512 root directory entries).
fn formatted_disk() -> RamDisk {
    let disk = blank_disk();
    {
        let mut blocks = disk.0.borrow_mut();

        let mbr = &mut blocks[0];
        let partition = &mut mbr[446..462];
        partition[4] = 0x0E; // FAT16 (LBA)
        partition[8..12].copy_from_slice(&PART_START.to_le_bytes());
        partition[12..16].copy_from_slice(&PART_BLOCKS.to_le_bytes());
        mbr[510..512].copy_from_slice(&[0x55, 0xAA]);

        let boot = &mut blocks[PART_START as usize];
        boot[0..3].copy_from_slice(&[0xEB, 0x3C, 0x90]);
        boot[3..11].copy_from_slice(b"MSWIN4.1");
        boot[11..13].copy_from_slice(&512u16.to_le_bytes()); // bytes per block
        boot[13] = 1; // blocks per cluster
        boot[14..16].copy_from_slice(&1u16.to_le_bytes()); // reserved blocks
        boot[16] = 2; // number of FATs
        boot[17..19].copy_from_slice(&ROOT_ENTRIES.to_le_bytes());
        boot[19..21].copy_from_slice(&(PART_BLOCKS as u16).to_le_bytes());
        boot[21] = 0xF8; // fixed disk
        boot[22..24].copy_from_slice(&FAT_BLOCKS.to_le_bytes());
        boot[24..26].copy_from_slice(&63u16.to_le_bytes()); // blocks per track
        boot[26..28].copy_from_slice(&255u16.to_le_bytes()); // heads
        boot[28..32].copy_from_slice(&PART_START.to_le_bytes()); // hidden blocks
        boot[36] = 0x80; // drive number
        boot[38] = 0x29; // extended boot signature
        boot[39..43].copy_from_slice(&0x2024_0001u32.to_le_bytes()); // volume ID
        boot[43..54].copy_from_slice(b"ARGUS      ");
        boot[54..62].copy_from_slice(b"FAT16   ");
        boot[510..512].copy_from_slice(&[0x55, 0xAA]);

        // Reserved FAT entries 0 and 1 in both FAT copies
        for fat in 0..2 {
            let first_fat_block = PART_START + 1 + fat * FAT_BLOCKS as u32;
            blocks[first_fat_block as usize][0..4].copy_from_slice(&[0xF8, 0xFF, 0xFF, 0xFF]);
        }
    }
    disk
}

fn read_all(sd: &SdStorage<RamDisk, SdTimeSource>, name: &str) -> Vec<u8> {
    let mut buf = vec![0u8; 8192];
    let n = sd.read_file(name, &mut buf).unwrap();
    buf.truncate(n);
    buf
}

#[test]
fn writes_appends_overwrites_and_reads_back() {
    let sd = SdStorage::mount(formatted_disk(), SdTimeSource).unwrap();

    sd.write_file("LOG.TXT", b"hello\n").unwrap();
    assert_eq!(read_all(&sd, "LOG.TXT"), b"hello\n");

    sd.append_file("LOG.TXT", b"world\n").unwrap();
    assert_eq!(read_all(&sd, "LOG.TXT"), b"hello\nworld\n");
    assert_eq!(sd.metadata("LOG.TXT").unwrap().size, 12);

    // A buffer smaller than the file gets the start of the file
    let mut small = [0u8; 5];
    assert_eq!(sd.read_file("LOG.TXT", &mut small).unwrap(), 5);
    assert_eq!(&small, b"hello");

    sd.write_file("LOG.TXT", b"new").unwrap();
    assert_eq!(read_all(&sd, "LOG.TXT"), b"new");
    assert_eq!(sd.metadata("LOG.TXT").unwrap().size, 3);

    // Larger than one cluster, so the file spans several clusters
    let big: Vec<u8> = (0..3000u32).map(|i| (i % 251) as u8).collect();
    sd.write_file("BIG.BIN", &big).unwrap();
    assert_eq!(read_all(&sd, "BIG.BIN"), big);
}

#[test]
fn exists_and_delete() {
    let sd = SdStorage::mount(formatted_disk(), SdTimeSource).unwrap();

    assert!(!sd.exists("DATA.BIN").unwrap());
    sd.write_file("DATA.BIN", &[1, 2, 3]).unwrap();
    assert!(sd.exists("DATA.BIN").unwrap());

    sd.delete_file("DATA.BIN").unwrap();
    assert!(!sd.exists("DATA.BIN").unwrap());
    assert!(matches!(
        sd.read_file("DATA.BIN", &mut [0u8; 4]),
        Err(Error::NotFound)
    ));
}

#[test]
fn files_survive_unmount_and_remount() {
    let sd = SdStorage::mount(formatted_disk(), SdTimeSource).unwrap();
    sd.write_file("KEEP.TXT", b"still here").unwrap();
    let (disk, time_source) = sd.unmount().unwrap();

    let sd = SdStorage::mount(disk, time_source).unwrap();
    assert_eq!(read_all(&sd, "KEEP.TXT"), b"still here");
}

#[test]
fn rejects_bad_names_and_unformatted_cards() {
    let sd = SdStorage::mount(formatted_disk(), SdTimeSource).unwrap();
    assert!(matches!(
        sd.write_file("TOOLONGNAME.TXT", b"x"),
        Err(Error::FilenameError(_))
    ));

    assert!(matches!(
        SdStorage::mount(blank_disk(), SdTimeSource),
        Err(Error::FormatError(_))
    ));
}

// The only test that calls set_unix_time, since it sets a global.
#[test]
fn timestamps_files_with_set_unix_time() {
    let sd = SdStorage::mount(formatted_disk(), SdTimeSource).unwrap();

    sd.write_file("BEFORE.TXT", b"x").unwrap();
    let before = sd.metadata("BEFORE.TXT").unwrap().mtime;
    assert_eq!(
        before,
        Timestamp::from_calendar(2024, 1, 1, 0, 0, 0).unwrap(),
        "default before a time is set"
    );

    sdcard::set_unix_time(1_757_870_000); // 2025-09-14T17:13:20Z
    sd.write_file("AFTER.TXT", b"x").unwrap();
    let after = sd.metadata("AFTER.TXT").unwrap().mtime;
    assert_eq!(
        after,
        Timestamp::from_calendar(2025, 9, 14, 17, 13, 20).unwrap()
    );
}
