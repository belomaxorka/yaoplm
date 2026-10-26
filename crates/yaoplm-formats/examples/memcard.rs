//! What a PS2 memory card image holds, read only (ADR-0048): every entry
//! with its mode bits and clusters, clusters shared by several entries
//! (cross-links), the backup blocks and the free space.
//!
//! `cargo run -p yaoplm-formats --example memcard -- <image.mcd>...`

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

use yaoplm_formats::ps2mc::Card;

fn main() {
    for path in std::env::args().skip(1) {
        println!("== {path}");
        let len = std::fs::metadata(&path).unwrap().len();
        let mut card = Card::open(File::open(&path).unwrap(), len).unwrap();
        let g = card.geometry().clone();
        println!(
            "clusters {} alloc_offset {} alloc_end {} root {} backup blocks {} {} ifc {:?} spare {}",
            g.clusters_per_card,
            g.alloc_offset,
            g.alloc_end,
            g.rootdir_cluster,
            g.backup_block1,
            g.backup_block2,
            g.ifc,
            g.spare
        );
        let mut owners: BTreeMap<u32, Vec<String>> = BTreeMap::new();
        let root = card.root().unwrap();
        for c in card.clusters(&root[0]).unwrap() {
            owners.entry(c).or_default().push("/".into());
        }
        for (path, entry) in card.walk(8).unwrap() {
            let clusters = if entry.length == 0 && entry.is_file() {
                Vec::new()
            } else {
                card.clusters(&entry).unwrap_or_default()
            };
            println!(
                "{:04x} attr {:08x} len {:>8} first {:>5} clusters {:>4}  {}",
                entry.mode,
                entry.attr,
                entry.length,
                entry.cluster,
                clusters.len(),
                path
            );
            for c in clusters {
                owners.entry(c).or_default().push(path.clone());
            }
        }
        let shared: Vec<_> = owners.iter().filter(|(_, o)| o.len() > 1).collect();
        println!(
            "used clusters {}, shared by more than one entry: {}",
            owners.len(),
            shared.len()
        );
        // The console's driver allocates only below 1000 * (clusters / 1000)
        // + 1 (ps2sdk mcman main.c, mcman_setdevinfos); the rest replaces
        // worn backup blocks. Small cards end at alloc_end first.
        let limit = (1000 * (g.clusters_per_card / 1000) + 1).min(g.alloc_end);
        let beyond = owners.keys().filter(|&&c| c >= limit).count();
        println!(
            "console allocates below {limit}: free there {}, used beyond {beyond}, highest used {:?}",
            limit as usize - owners.keys().filter(|&&c| c < limit).count(),
            owners.keys().next_back()
        );
        for (cluster, who) in shared.iter().take(20) {
            println!("  cluster {cluster}: {who:?}");
        }
        // The backup blocks, raw.
        let block = u64::from(g.pages_per_block) * u64::from(g.page_size + g.spare);
        let mut f = File::open(&path).unwrap();
        for (name, b) in [
            ("backup block 1", g.backup_block1),
            ("backup block 2", g.backup_block2),
        ] {
            let mut buf = vec![0u8; block as usize];
            f.seek(SeekFrom::Start(u64::from(b) * block)).unwrap();
            f.read_exact(&mut buf).unwrap();
            let ff = buf.iter().filter(|&&x| x == 0xFF).count();
            let first: Vec<String> = buf[..16].iter().map(|b| format!("{b:02x}")).collect();
            println!(
                "{name} (block {b}): {ff}/{} bytes 0xFF, starts {}",
                buf.len(),
                first.join(" ")
            );
        }
    }
}
