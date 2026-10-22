//! Fixture builders for tests: tiny ISO9660 images with `SYSTEM.CNF`,
//! BIN/CUE, ZSO, Logiqx DATs, FAT32 images and virtual large files, plus
//! test devices (in memory, FAT32 image) and in-memory import sources.
//!
//! Fixtures are generated at test time. Real game images are never committed.

pub mod bincue;
mod content;
pub mod fatimg;
pub mod fixtures;
pub mod iso;
pub mod memfs;
pub mod sources;

pub use content::{Content, VirtualFile};
pub use fatimg::FatImage;
pub use fixtures::Variant;
pub use iso::{IsoBuilder, VirtualIso};
pub use memfs::MemFs;
pub use sources::VirtualSources;

#[cfg(test)]
mod tests {
    use std::io::{Read, Seek, SeekFrom};

    use super::*;

    fn le32(b: &[u8]) -> u32 {
        u32::from_le_bytes([b[0], b[1], b[2], b[3]])
    }

    #[test]
    fn layout_is_standard_iso9660() {
        let iso = Variant::Retail.build().to_vec();
        let pvd = &iso[16 * 2048..17 * 2048];
        assert_eq!(&pvd[..7], b"\x01CD001\x01");
        assert_eq!(&pvd[8..19], b"PLAYSTATION");
        assert_eq!(le32(&pvd[80..]) as usize * 2048, iso.len());
        assert_eq!(&pvd[84..88], &(iso.len() as u32 / 2048).to_be_bytes());
        assert_eq!(&iso[17 * 2048..17 * 2048 + 6], b"\xFFCD001");
        // Root directory: ".", "..", then SYSTEM.CNF;1 with a zero pad byte.
        let root = le32(&pvd[158..]) as usize * 2048;
        assert_eq!(iso[root + 33], 0);
        assert_eq!(iso[root + 34 + 33], 1);
        let cnf = root + 68;
        assert_eq!(&iso[cnf + 33..cnf + 45], b"SYSTEM.CNF;1");
        assert_eq!(iso[cnf + 45], 0);
        let data = le32(&iso[cnf + 2..]) as usize * 2048;
        let size = le32(&iso[cnf + 10..]) as usize;
        assert!(iso[data..data + size].starts_with(b"BOOT2 = cdrom0:\\SLUS_200.62;1\r\n"));
    }

    #[test]
    fn subdirectories_and_path_tables() {
        let iso = Variant::SubfolderElf.build().to_vec();
        let l_table = &iso[18 * 2048..];
        // Root, then DATA with parent 1.
        assert_eq!(&l_table[..2], &[1, 0]);
        assert_eq!(&l_table[10..16], &[4, 0, 21, 0, 0, 0]);
        assert_eq!(&l_table[16..22], &[1, 0, b'D', b'A', b'T', b'A']);
    }

    #[test]
    fn dual_layer_has_a_second_descriptor() {
        let iso = Variant::Dvd9.build().to_vec();
        let layer0 = Variant::DVD9_LAYER0_SECTORS as usize;
        assert_eq!(le32(&iso[16 * 2048 + 80..]) as usize, layer0 + 16);
        assert_eq!(&iso[(layer0 + 16) * 2048..][..6], b"\x01CD001");
    }

    #[test]
    fn large_images_are_virtual() {
        let mut iso =
            fixtures::boot_iso(fixtures::placeholder_elf(), "YAOP_000.01", 4_500_000_000).build();
        assert_eq!(iso.len(), 4_500_000_000u64.div_ceil(2048) * 2048);
        iso.seek(SeekFrom::Start(4 << 30)).unwrap();
        let mut buf = [1u8; 4096];
        iso.read_exact(&mut buf).unwrap();
        assert!(buf.iter().all(|&b| b == 0));
        let mut head = vec![0u8; 20 * 2048];
        iso.seek(SeekFrom::Start(0)).unwrap();
        iso.read_exact(&mut head).unwrap();
        let mut sector = [0u8; 2048];
        for lba in 0..20 {
            iso.sector(lba, &mut sector);
            assert_eq!(head[lba as usize * 2048..][..2048], sector);
        }
    }
}
