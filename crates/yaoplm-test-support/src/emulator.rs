//! A synthetic memory card emulator's SD (ADR-0048): the layout of the
//! owner's factory card (research/19 §2). The boot cards are real card
//! images built by [`memcard`]: FreeMcBoot's folders with an OPL build with
//! MMCE, PS2BBL's with one without; the numbered cards are placeholders
//! that only carry the superblock's magic. Real boot cards (FreeMcBoot) are
//! never committed.

use yaoplm_device::FsKind;

use crate::memcard;
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

/// The MMCE fork's beta 2 as a test ELF: its version, OPL's settings file
/// and the MMCE setting (research/16 §3).
pub fn mmce_elf() -> Vec<u8> {
    crate::loaders::plain_elf(&[
        "conf_opl.cfg",
        "mmce_mode",
        "Open PS2 Loader %s",
        "v1.2.0-Beta-2216-bb415d2",
    ])
}

/// Boot card 1: FreeMcBoot's folders and an OPL build with MMCE.
pub fn fmcb_card() -> Vec<u8> {
    let title = memcard::icon_sys("FreeMcBoot");
    let mmce = mmce_elf();
    memcard::card(&[
        ("BAEXEC-SYSTEM/icon.sys", &title),
        ("BAEXEC-SYSTEM/osdmain.elf", b"\x7fELF placeholder"),
        ("SYS-CONF/FREEMCB.CNF", b"OSDSYS_Skip_Logo = 1\r\n"),
        ("APPS/OPL-MMCE-BETA2.ELF", &mmce),
    ])
}

/// Boot card 2: PS2BBL's folders and an upstream beta without MMCE.
pub fn ps2bbl_card() -> Vec<u8> {
    let title = memcard::icon_sys("PS2BBL");
    let beta = crate::loaders::opl_elf("v1.2.0-Beta-2049-513e9b3");
    memcard::card(&[
        ("BAEXEC-SYSTEM/icon.sys", &title),
        (
            "SYS-CONF/PS2BBL.INI",
            b"LK_AUTO_E1 = mc?:/APPS/OPNPS2LD.ELF\n",
        ),
        ("APPS/OPNPS2LD.ELF", &beta),
    ])
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
        ("MemoryCards/PS2/BOOT/BootCard-1.mcd", fmcb_card()),
        ("MemoryCards/PS2/BOOT/BootCard-2.mcd", ps2bbl_card()),
        ("MemoryCards/PS2/Card1/Card1-1.mcd", image.clone()),
        ("MemoryCards/PS2/SLUS-20062/SLUS-20062-1.mcd", image.clone()),
        (
            "MemoryCards/PS1/BOOT/BootCard.ini",
            PS1_BOOT_INI.as_bytes().to_vec(),
        ),
        ("MemoryCards/PS1/BOOT/BootCard-1.mcd", memcard::ps1_card()),
        ("MemoryCards/PS1/Card1/Card1-1.mcd", memcard::ps1_card()),
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
