//! A synthetic memory card emulator's SD (ADR-0048): the layout of the
//! owner's factory card (research/19 §2) with placeholder images. The images
//! only carry the superblock's magic: tests that read the card file system
//! format their own. Real boot cards (FreeMcBoot) are never committed.

use yaoplm_device::FsKind;

use crate::memfs::MemFs;

/// `.sd2psx/settings.ini` as the factory card has it: LF, `CardSize=64`.
pub const SETTINGS_INI: &str = "[General]\nMode=PS2\nFlippedScreen=OFF\n[PS1]\nAutoboot=OFF\nGameID=ON\n\
[PS2]\nAutoboot=ON\nGameID=ON\nCardSize=64\nVariant=RETAIL\n";
/// The PS2 boot folder's `BootCard.ini` in SD Builder's form.
pub const PS2_BOOT_INI: &str =
    "[ChannelName]\n1=FMCB 1.966\n2=PS2BBL\n[Settings]\nMaxChannels=2\nCardSize=8\n";
pub const PS1_BOOT_INI: &str = "[ChannelName]\n1=Unirom\n";

/// A placeholder card image of `len` bytes.
pub fn card_image(len: usize) -> Vec<u8> {
    let mut image = vec![0xFF; len];
    let magic = b"Sony PS2 Memory Card Format ";
    image[..magic.len()].copy_from_slice(magic);
    image
}

/// Every file of the SD, relative to its root.
pub fn files() -> Vec<(String, Vec<u8>)> {
    let image = card_image(16 * 1024);
    [
        (".sd2psx/settings.ini", SETTINGS_INI.as_bytes().to_vec()),
        (
            "MemoryCards/PS2/BOOT/BootCard.ini",
            PS2_BOOT_INI.as_bytes().to_vec(),
        ),
        ("MemoryCards/PS2/BOOT/BootCard-1.mcd", image.clone()),
        ("MemoryCards/PS2/BOOT/BootCard-2.mcd", image.clone()),
        ("MemoryCards/PS2/Card1/Card1-1.mcd", image.clone()),
        ("MemoryCards/PS2/SLUS-20062/SLUS-20062-1.mcd", image.clone()),
        (
            "MemoryCards/PS1/BOOT/BootCard.ini",
            PS1_BOOT_INI.as_bytes().to_vec(),
        ),
        ("MemoryCards/PS1/BOOT/BootCard-1.mcd", b"MC".to_vec()),
        ("MemoryCards/PS1/Card1/Card1-1.mcd", b"MC".to_vec()),
    ]
    .into_iter()
    .map(|(path, bytes)| (path.to_owned(), bytes))
    .collect()
}

/// The SD in memory, on exFAT as the factory card.
pub fn memfs() -> MemFs {
    let fs = MemFs::new(FsKind::ExFat);
    for (path, bytes) in files() {
        fs.insert_file(&path, &bytes);
    }
    fs
}

/// Writes the SD into the folder `root` on the host.
pub fn write_to(root: &std::path::Path) -> std::io::Result<()> {
    for (path, bytes) in files() {
        let target = root.join(path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(target, bytes)?;
    }
    Ok(())
}
