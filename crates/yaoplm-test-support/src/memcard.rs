//! Small but valid PS2 memory card images for fixtures (ADR-0048): an 8 MiB
//! card in genvmc's layout (research/15 §2.1) holding the folders and files
//! a test names, each written in consecutive clusters. Only for tests and
//! the fixture library: writing a user's card is LDR-5's, in
//! `yaoplm-formats`. mymcplusplus `check` passes on these cards
//! (`crates/yaoplm-formats/tests/memcard_oracles.rs`).

const CLUSTER: usize = 1024;
const CLUSTERS: u32 = 8192;
const IFC: u32 = 8;
const FAT_START: u32 = 9;
const ALLOC_OFFSET: u32 = 41;
const ALLOC_END: u32 = 8135;
const ENTRY: usize = 512;
const MODE_DIR: u16 = 0x8427;
const MODE_FILE: u16 = 0x8417;
const MODE_DOTDOT: u16 = 0xA426;
const FAT_FREE: u32 = 0x7FFF_FFFF;
const FAT_END: u32 = 0xFFFF_FFFF;

/// A time the console's clock could show: 2026-10-26 12:00:00.
const TOD: [u8; 8] = [0, 0, 0, 12, 26, 10, 0xEA, 0x07];

struct Folder<'a> {
    name: &'a str,
    files: Vec<(&'a str, &'a [u8])>,
}

/// A directory entry. A folder's `.` entry names where the folder's own
/// entry is: its parent's first cluster and its index there (`dir_entry`),
/// as mymcplusplus checks.
fn entry(card: &mut [u8], at: usize, mode: u16, length: u32, cluster: u32, name: &str) {
    entry_in(card, at, mode, length, cluster, 0, name);
}

fn entry_in(
    card: &mut [u8],
    at: usize,
    mode: u16,
    length: u32,
    cluster: u32,
    dir_entry: u32,
    name: &str,
) {
    let e = &mut card[at..at + ENTRY];
    e.fill(0);
    e[0..2].copy_from_slice(&mode.to_le_bytes());
    e[4..8].copy_from_slice(&length.to_le_bytes());
    e[8..16].copy_from_slice(&TOD);
    e[16..20].copy_from_slice(&cluster.to_le_bytes());
    e[20..24].copy_from_slice(&dir_entry.to_le_bytes());
    e[24..32].copy_from_slice(&TOD);
    e[0x40..0x40 + name.len()].copy_from_slice(name.as_bytes());
}

/// An 8 MiB card holding `files`: paths of one or two parts (`icon.sys` at
/// the root is not allowed by the console; `APPS/OPNPS2LD.ELF`), folders
/// made from them in order of first use.
pub fn card(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut folders: Vec<Folder> = Vec::new();
    for (path, bytes) in files {
        let (folder, name) = path.split_once('/').expect("FOLDER/file");
        match folders.iter_mut().find(|f| f.name == folder) {
            Some(f) => f.files.push((name, bytes)),
            None => folders.push(Folder {
                name: folder,
                files: vec![(name, bytes)],
            }),
        }
    }
    let mut card = vec![0xFFu8; CLUSTERS as usize * CLUSTER];
    let mut fat = vec![FAT_FREE; ALLOC_END as usize];
    let mut next = 0u32;
    let mut take = |count: u32, fat: &mut Vec<u32>| -> u32 {
        let first = next;
        for i in 0..count {
            let c = first + i;
            fat[c as usize] = if i + 1 == count {
                FAT_END
            } else {
                0x8000_0000 | (c + 1)
            };
        }
        next += count;
        first
    };
    let clusters_for = |bytes: usize| (bytes.div_ceil(CLUSTER) as u32).max(1);
    let root_entries = 2 + folders.len();
    let root = take(clusters_for(root_entries * ENTRY), &mut fat);
    let at =
        |cluster: u32, index: usize| (ALLOC_OFFSET + cluster) as usize * CLUSTER + index * ENTRY;
    let root_at = |index: usize| {
        // Two entries a cluster, the root's clusters in a row.
        at(root + (index / 2) as u32, index % 2)
    };
    entry(
        &mut card,
        root_at(0),
        MODE_DIR,
        root_entries as u32,
        root,
        ".",
    );
    entry(&mut card, root_at(1), MODE_DOTDOT, 2, root, "..");
    for (i, folder) in folders.iter().enumerate() {
        let count = 2 + folder.files.len();
        let dir = take(clusters_for(count * ENTRY), &mut fat);
        entry(
            &mut card,
            root_at(2 + i),
            MODE_DIR,
            count as u32,
            dir,
            folder.name,
        );
        let dir_at = |index: usize| at(dir + (index / 2) as u32, index % 2);
        entry_in(
            &mut card,
            dir_at(0),
            MODE_DIR,
            count as u32,
            root,
            (2 + i) as u32,
            ".",
        );
        entry(&mut card, dir_at(1), MODE_DOTDOT, 0, root, "..");
        for (j, (name, bytes)) in folder.files.iter().enumerate() {
            let first = take(clusters_for(bytes.len()), &mut fat);
            let start = (ALLOC_OFFSET + first) as usize * CLUSTER;
            card[start..start + bytes.len()].copy_from_slice(bytes);
            entry(
                &mut card,
                dir_at(2 + j),
                MODE_FILE,
                bytes.len() as u32,
                first,
                name,
            );
        }
    }
    // The FAT from cluster 9, 256 entries a cluster, zeros past the end;
    // the indirect FAT in cluster 8 lists the FAT clusters.
    let fat_clusters = ALLOC_END.div_ceil(256);
    for c in 0..fat_clusters {
        let base = (FAT_START + c) as usize * CLUSTER;
        for k in 0..256u32 {
            let n = (c * 256 + k) as usize;
            let value = fat.get(n).copied().unwrap_or(0);
            card[base + k as usize * 4..base + k as usize * 4 + 4]
                .copy_from_slice(&value.to_le_bytes());
        }
    }
    let ifc = IFC as usize * CLUSTER;
    card[ifc..ifc + CLUSTER].fill(0);
    for c in 0..fat_clusters {
        card[ifc + c as usize * 4..ifc + c as usize * 4 + 4]
            .copy_from_slice(&(FAT_START + c).to_le_bytes());
    }
    // The superblock.
    let sb = &mut card[..CLUSTER];
    sb[..0x154].fill(0);
    sb[..28].copy_from_slice(b"Sony PS2 Memory Card Format ");
    sb[28..35].copy_from_slice(b"1.2.0.0");
    sb[0x28..0x2A].copy_from_slice(&512u16.to_le_bytes());
    sb[0x2A..0x2C].copy_from_slice(&2u16.to_le_bytes());
    sb[0x2C..0x2E].copy_from_slice(&16u16.to_le_bytes());
    sb[0x30..0x34].copy_from_slice(&CLUSTERS.to_le_bytes());
    sb[0x34..0x38].copy_from_slice(&ALLOC_OFFSET.to_le_bytes());
    sb[0x38..0x3C].copy_from_slice(&ALLOC_END.to_le_bytes());
    sb[0x3C..0x40].copy_from_slice(&0u32.to_le_bytes());
    sb[0x40..0x44].copy_from_slice(&1023u32.to_le_bytes());
    sb[0x44..0x48].copy_from_slice(&1022u32.to_le_bytes());
    sb[0x50..0xD0].fill(0xFF);
    sb[0x50..0x54].copy_from_slice(&IFC.to_le_bytes());
    sb[0xD0..0x150].fill(0xFF);
    sb[0x150] = 2;
    sb[0x151] = 0x2B;
    card
}

/// An `icon.sys` with `title` (full-width text is fine) on one line.
pub fn icon_sys(title: &str) -> Vec<u8> {
    let mut bytes = vec![0u8; 964];
    bytes[..4].copy_from_slice(b"PS2D");
    let encoded = encode_title(title);
    bytes[6..8].copy_from_slice(&(encoded.len() as u16).to_le_bytes());
    bytes[0xC0..0xC0 + encoded.len()].copy_from_slice(&encoded);
    bytes[0x104..0x10C].copy_from_slice(b"list.ico");
    bytes
}

/// ASCII as the console's full-width Shift-JIS letters (`ＦＲＥＥ`).
fn encode_title(title: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for c in title.chars() {
        match c {
            ' ' => out.extend_from_slice(&[0x81, 0x40]),
            '0'..='9' => out.extend_from_slice(&[0x82, 0x4F + (c as u8 - b'0')]),
            'A'..='Z' => out.extend_from_slice(&[0x82, 0x60 + (c as u8 - b'A')]),
            'a'..='z' => out.extend_from_slice(&[0x82, 0x81 + (c as u8 - b'a')]),
            other => out.push(other as u8),
        }
    }
    out
}

/// A formatted PS1 card without saves.
pub fn ps1_card() -> Vec<u8> {
    let mut card = vec![0u8; 128 * 1024];
    card[..2].copy_from_slice(b"MC");
    card[127] = 0x0E;
    for i in 1..16 {
        let frame = 128 * i;
        card[frame] = 0xA0;
        card[frame + 8] = 0xFF;
        card[frame + 9] = 0xFF;
        card[frame + 127] = 0xA0;
    }
    card
}
