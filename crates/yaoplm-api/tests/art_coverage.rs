//! How many games get a picture of each type (ADR-0035, research/14 §6),
//! measured with the code that picks sources (per-type order, region
//! policy, OPL Manager's numbering) against lists of the sources' files:
//! the xlenore tree, the libretro directory indexes and the table of
//! contents of `OPLM_ART_2024_09`. ScreenScraper is not counted: it needs
//! a developer key.
//!
//! Real data, run by hand; the lists come from `scripts/art-source-lists.py`
//! and `scripts/oplm-backup-toc.py`:
//!
//! ```text
//! YAOPLM_TESTDATA=<test data folder> cargo test -p yaoplm-api --test art_coverage -- --ignored --nocapture
//! ```

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use yaoplm_core::LoaderProfile;
use yaoplm_core::titles::{self, Entry};
use yaoplm_formats::art::{ArtKind, ArtSubject, recommended_size};
use yaoplm_formats::serial;
use yaoplm_net::oplm::Index;
use yaoplm_net::region::{self, Family};
use yaoplm_net::remote_zip::Member;
use yaoplm_net::sources::{self, SourceId};
use yaoplm_net::{MAX_LIBRETRO_CANDIDATES, MAX_OTHER_RELEASES};

fn testdata() -> Option<PathBuf> {
    std::env::var_os("YAOPLM_TESTDATA").map(PathBuf::from)
}

fn lines(path: &PathBuf) -> HashSet<String> {
    std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .lines()
        .map(str::to_owned)
        .filter(|l| !l.is_empty())
        .collect()
}

/// `name, size, compressed size, method, CRC-32, offset` per line.
fn oplm_index(path: &PathBuf) -> Index {
    let members = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            Some(Member {
                name: f.first()?.to_string(),
                size: f.get(1)?.parse().ok()?,
                compressed_size: f.get(2)?.parse().ok()?,
                method: f.get(3)?.parse().ok()?,
                crc32: u32::from_str_radix(f.get(4)?, 16).ok()?,
                offset: f.get(5)?.parse().ok()?,
            })
        })
        .collect();
    Index::from_members(members, 0)
}

struct Lists {
    /// File paths per source, in the source's layout.
    files: HashMap<SourceId, HashSet<String>>,
    oplm: Index,
}

/// The source that would give `entry` its picture of `kind`, and whether a
/// source had one only for another market's release; for a type shared
/// across markets, the game's other releases after its own serial, as
/// `ArtService::fetch_cover` asks them.
fn first_source(
    lists: &Lists,
    entry: &Entry,
    kind: ArtKind,
    without: &[SourceId],
) -> (Option<SourceId>, bool) {
    let (found, mut other_market) =
        own_source(lists, entry, kind, without, MAX_LIBRETRO_CANDIDATES);
    if found.is_some() || region::region_specific(kind) {
        return (found, other_market);
    }
    let mut others = titles::database().same_title(entry);
    others.sort_by_key(|o| match region::of_id(&o.serial) {
        Some(Family::America) => 0,
        Some(Family::Europe) => 1,
        Some(Family::Asia) => 2,
        None => 3,
    });
    let mut without = without.to_vec();
    without.push(SourceId::ScreenScraper);
    for other in others.into_iter().take(MAX_OTHER_RELEASES) {
        let (found, other) = own_source(lists, other, kind, &without, 1);
        other_market |= other;
        if found.is_some() {
            return (found, false);
        }
    }
    (None, other_market)
}

fn own_source(
    lists: &Lists,
    entry: &Entry,
    kind: ArtKind,
    without: &[SourceId],
    max_names: usize,
) -> (Option<SourceId>, bool) {
    let size = recommended_size(
        LoaderProfile::Opl120Beta,
        ArtSubject::Game { startup: "" },
        kind,
    );
    let names: Vec<String> = entry
        .redump_names_by_region()
        .iter()
        .take(max_names)
        .map(|n| n.name.clone())
        .collect();
    let mut other_market = false;
    for source in sources::order(kind, size) {
        if source == SourceId::ScreenScraper || without.contains(&source) {
            continue;
        }
        let found = match source {
            SourceId::Oplm => {
                let pictures =
                    lists
                        .oplm
                        .pictures(&serial::serial_to_opl(&entry.serial), kind, false);
                let fits = |p: &&yaoplm_net::oplm::Picture| {
                    !(region::region_specific(kind) && p.shared_across_markets)
                };
                other_market |= !pictures.is_empty() && !pictures.iter().any(fits);
                pictures.iter().any(fits)
            }
            _ => {
                let all = sources::candidates(source, kind, &entry.serial, &names, false);
                all.iter().any(|c| lists.files[&source].contains(&c.path))
            }
        };
        if found {
            return (Some(source), false);
        }
    }
    (None, other_market)
}

fn pct(part: usize, whole: usize) -> String {
    if whole == 0 {
        return "—".into();
    }
    format!("{:.1}%", 100.0 * part as f64 / whole as f64)
}

#[test]
#[ignore = "real data: YAOPLM_TESTDATA with art-lists and oplm-art"]
fn coverage_per_type() {
    let Some(data) = testdata() else {
        eprintln!("YAOPLM_TESTDATA is not set");
        return;
    };
    let art = data.join("art-lists");
    let mut files = HashMap::new();
    files.insert(
        SourceId::Xlenore,
        lines(&art.join("xlenore_default.txt"))
            .into_iter()
            .map(|f| format!("covers/default/{f}"))
            .collect::<HashSet<_>>(),
    );
    let mut libretro = HashSet::new();
    for folder in [
        "Named_Boxarts",
        "Named_Snaps",
        "Named_Titles",
        "Named_Logos",
    ] {
        for file in lines(&art.join(format!("libretro_{folder}.txt"))) {
            libretro.insert(format!("{folder}/{file}"));
        }
    }
    files.insert(SourceId::Libretro, libretro);
    files.insert(SourceId::Oplm, HashSet::new());
    let lists = Lists {
        files,
        oplm: oplm_index(&data.join("oplm-art").join("OPLM_ART_2024_09.tsv")),
    };

    let db = titles::database();
    let all: Vec<&Entry> = db.entries().collect();
    let named: Vec<&Entry> = all
        .iter()
        .copied()
        .filter(|e| !e.redump.is_empty())
        .collect();
    let families = [
        ("NTSC-U", Some(Family::America)),
        ("PAL", Some(Family::Europe)),
        ("NTSC-J/K", Some(Family::Asia)),
    ];
    println!(
        "Universe: {} serials, {} with a Redump name; OPL Manager index: {} games\n",
        all.len(),
        named.len(),
        lists.oplm.games()
    );
    println!(
        "| Type | All serials | Redump-named | without OPLM | NTSC-U | PAL | NTSC-J/K | wrong region left out | first source (Redump-named) |"
    );
    println!("|---|---|---|---|---|---|---|---|---|");
    let mut named_coverage = HashMap::new();
    for kind in ArtKind::ALL {
        let found_all = all
            .iter()
            .filter(|e| first_source(&lists, e, kind, &[]).0.is_some())
            .count();
        let mut wins: HashMap<SourceId, usize> = HashMap::new();
        let mut found_named = 0;
        let mut refused = 0;
        let mut without_oplm = 0;
        let mut by_family: HashMap<Option<Family>, (usize, usize)> = HashMap::new();
        for entry in &named {
            let (source, other) = first_source(&lists, entry, kind, &[]);
            let family = region::of_id(&entry.serial);
            let slot = by_family.entry(family).or_default();
            slot.1 += 1;
            match source {
                Some(source) => {
                    found_named += 1;
                    slot.0 += 1;
                    *wins.entry(source).or_default() += 1;
                }
                None => refused += usize::from(other),
            }
            without_oplm += usize::from(
                first_source(&lists, entry, kind, &[SourceId::Oplm])
                    .0
                    .is_some(),
            );
        }
        let regions: Vec<String> = families
            .iter()
            .map(|(_, f)| {
                let (found, total) = by_family.get(f).copied().unwrap_or_default();
                pct(found, total)
            })
            .collect();
        let mut wins: Vec<(SourceId, usize)> = wins.into_iter().collect();
        wins.sort_by_key(|(s, _)| {
            sources::order(
                kind,
                recommended_size(
                    LoaderProfile::Opl120Beta,
                    ArtSubject::Game { startup: "" },
                    kind,
                ),
            )
            .iter()
            .position(|o| o == s)
        });
        let wins: Vec<String> = wins
            .iter()
            .map(|(s, n)| format!("{} {n}", s.as_str()))
            .collect();
        println!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            kind.suffix(),
            pct(found_all, all.len()),
            pct(found_named, named.len()),
            pct(without_oplm, named.len()),
            regions[0],
            regions[1],
            regions[2],
            refused,
            wins.join(", ")
        );
        named_coverage.insert(kind, found_named as f64 / named.len() as f64);
    }
    // The lists of 2026-10-24 gave these; a much lower number means a
    // broken list or a broken rule, not a source that shrank.
    assert!(named_coverage[&ArtKind::Cov] > 0.85, "{named_coverage:?}");
    assert!(named_coverage[&ArtKind::Ico] > 0.45, "{named_coverage:?}");
    assert!(named_coverage[&ArtKind::Bg] > 0.60, "{named_coverage:?}");
}
