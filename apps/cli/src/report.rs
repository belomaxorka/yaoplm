//! Human-readable output for every subcommand.

use std::fmt::Write;
use std::path::Path;

use yaoplm_api::art::{ArtLogState, Attribution};
use yaoplm_api::devices::{DeviceInfo, DriveBus, DriveKind, FileSystem, PartitionTable};
use yaoplm_api::jobs::{ExecutionReport, JobLogEntry, TxOutcome};
use yaoplm_api::library::{
    GameFormat, ImageDetails, Library, LoaderProfile, Media, NameFormat, Notice, Platform,
};
use yaoplm_api::plans::{NoticeLevel, PlanNotice, PlanStep, PlanView, RemovalKind, StepOp};

pub fn profile_name(profile: LoaderProfile) -> &'static str {
    match profile {
        LoaderProfile::Opl120Beta => "OPL 1.2.0 beta",
        LoaderProfile::Opl110 => "OPL 1.1.0",
        LoaderProfile::RiptOpl => "RiptOPL",
    }
}

/// Binary units, one decimal: `531.2 MiB`.
pub fn size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn media(media: Media) -> &'static str {
    match media {
        Media::CD => "CD",
        Media::DVD => "DVD",
    }
}

fn format(format: GameFormat) -> &'static str {
    match format {
        GameFormat::ISO => "ISO",
        GameFormat::ZSO => "ZSO",
        GameFormat::UL => "UL",
    }
}

/// `path` relative to `root` when it is inside it.
fn relative<'a>(path: &'a str, root: &str) -> &'a str {
    Path::new(path)
        .strip_prefix(root)
        .ok()
        .and_then(|p| p.to_str())
        .unwrap_or(path)
}

fn notices(out: &mut String, indent: &str, notices: &[Notice]) {
    for notice in notices {
        let _ = writeln!(out, "{indent}! {}", notice.message);
    }
}

pub fn library(lib: &Library) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "Library: {} ({})", lib.root, profile_name(lib.profile));
    let folders: Vec<String> = lib
        .folders
        .iter()
        .map(|f| format!("{}{}", f.name, if f.present { "" } else { " (missing)" }))
        .collect();
    let _ = writeln!(out, "Folders: {}", folders.join(", "));

    let _ = writeln!(out, "\nGames ({}):", lib.games.len());
    if !lib.games.is_empty() {
        let _ = writeln!(
            out,
            "  {:<5} {:<4} {:<12} {:>10}  TITLE",
            "MEDIA", "FMT", "ID", "SIZE"
        );
    }
    for game in &lib.games {
        let mut title = game.title.clone();
        if !game.parts.is_empty() {
            let parts = game.parts.len();
            let _ = write!(
                title,
                " ({parts} part{})",
                if parts == 1 { "" } else { "s" }
            );
        }
        let _ = writeln!(
            out,
            "  {:<5} {:<4} {:<12} {:>10}  {}",
            media(game.media),
            format(game.format),
            game.startup,
            size(game.size),
            title
        );
        notices(&mut out, "      ", &game.warnings);
    }

    if !lib.apps.is_empty() {
        let _ = writeln!(out, "\nApps ({}):", lib.apps.len());
        for app in &lib.apps {
            let _ = writeln!(out, "  {}  (APPS/{}/{})", app.title, app.folder, app.boot);
            notices(&mut out, "      ", &app.warnings);
        }
    }
    if !lib.skipped.is_empty() {
        let _ = writeln!(out, "\nNot listed by OPL ({}):", lib.skipped.len());
        for skipped in &lib.skipped {
            let _ = writeln!(
                out,
                "  {}: {}",
                relative(&skipped.path, &lib.root),
                skipped.reason.message
            );
        }
    }
    if !lib.warnings.is_empty() {
        let _ = writeln!(out, "\nWarnings:");
        notices(&mut out, "  ", &lib.warnings);
    }
    out
}

pub fn image(info: &ImageDetails) -> String {
    let mut out = String::new();
    let line = |out: &mut String, label: &str, value: &str| {
        let _ = writeln!(out, "{label:<16}{value}");
    };
    line(&mut out, "File:", &info.path);
    line(
        &mut out,
        "Size:",
        &format!("{} ({} bytes)", size(info.file_size), info.file_size),
    );
    let mut container = format(info.format).to_owned();
    if let Some(zso) = &info.zso {
        let _ = write!(
            container,
            " (v{}, align {}, {} uncompressed)",
            zso.version,
            zso.align,
            size(zso.total_bytes)
        );
    }
    line(&mut out, "Format:", &container);
    match &info.startup {
        Some(id) => {
            line(&mut out, "Game ID:", id);
            if let Some(title) = &info.known_title {
                line(&mut out, "Known as:", title);
            }
        }
        None => line(
            &mut out,
            "Game ID:",
            &format!(
                "none, OPL hides this image: {}",
                info.hidden_reason.as_deref().unwrap_or("?")
            ),
        ),
    }
    if let Some(name) = &info.file_name {
        let how = match (name.format, &name.startup) {
            (NameFormat::Old, Some(id)) => format!("old style, OPL takes ID {id} from the name"),
            _ => "new style, OPL reads SYSTEM.CNF".to_owned(),
        };
        line(&mut out, "File name:", &how);
    }
    let platform = match info.platform {
        Platform::PS2 => "PS2",
        Platform::PS1 => "PS1 (use POPStarter, not OPL)",
        Platform::Unknown => "unknown",
    };
    line(&mut out, "Platform:", platform);
    line(
        &mut out,
        "Media:",
        &format!(
            "{} (YAOPLM's guess; OPL takes it from the folder)",
            media(info.suggested_media)
        ),
    );
    if let Some(volume) = &info.volume_id {
        line(&mut out, "Volume:", volume);
    }
    if let Some(sectors) = info.volume_sectors {
        line(&mut out, "Volume sectors:", &sectors.to_string());
    }
    if let Some(start) = info.layer1_start {
        line(
            &mut out,
            "Layers:",
            &format!("dual (DVD-9), layer 1 at sector {start}"),
        );
    }
    for (label, value) in [
        ("BOOT2:", &info.boot2),
        ("BOOT:", &info.boot),
        ("VER:", &info.ver),
        ("VMODE:", &info.vmode),
    ] {
        if let Some(value) = value {
            line(&mut out, label, value);
        }
    }
    if !info.warnings.is_empty() {
        let _ = writeln!(out, "Warnings:");
        notices(&mut out, "  ", &info.warnings);
    }
    out
}

fn file_system(fs: FileSystem) -> &'static str {
    match fs {
        FileSystem::FAT32 => "FAT32",
        FileSystem::FAT16 => "FAT16",
        FileSystem::ExFat => "exFAT",
        FileSystem::NTFS => "NTFS",
        FileSystem::Other => "other",
    }
}

fn mark(level: NoticeLevel) -> &'static str {
    match level {
        NoticeLevel::Info => "i",
        NoticeLevel::Warning => "!",
        NoticeLevel::Error => "x",
    }
}

fn plan_notices(out: &mut String, indent: &str, notices: &[PlanNotice]) {
    for notice in notices {
        let _ = writeln!(out, "{indent}{} {}", mark(notice.level), notice.message);
    }
}

fn device_line(device: &DeviceInfo) -> String {
    let mut line = format!(
        "{}  {}  {} free of {}",
        device.root,
        file_system(device.file_system),
        size(device.free_bytes),
        size(device.total_bytes)
    );
    if let Some(cluster) = device.cluster_size {
        let _ = write!(line, ", cluster {}", size(u64::from(cluster)));
    }
    if let Some(serial) = &device.serial {
        let _ = write!(line, ", serial {serial}");
    }
    let mut what = Vec::new();
    if let Some(bus) = device.bus {
        what.push(match bus {
            DriveBus::Usb => "USB",
            DriveBus::Sd => "SD",
            DriveBus::Ieee1394 => "iLink",
            DriveBus::Sata => "SATA",
            DriveBus::Nvme => "NVMe",
            DriveBus::Other => "other bus",
        });
    }
    what.push(match device.kind {
        DriveKind::Removable => "removable",
        DriveKind::Fixed => "fixed",
        DriveKind::Remote => "network",
        DriveKind::Optical => "optical",
        DriveKind::Unknown => "unknown",
    });
    if device.system {
        what.push("system");
    }
    let _ = write!(line, ", {}", what.join(" "));
    if let Some(label) = &device.label {
        let _ = write!(line, ", label {label}");
    }
    line
}

pub fn devices(devices: &[DeviceInfo], all: bool) -> String {
    let mut out = String::new();
    if devices.is_empty() {
        let _ = writeln!(
            out,
            "No {}volumes found.",
            if all { "" } else { "USB or SD " }
        );
        return out;
    }
    for device in devices {
        let _ = writeln!(out, "{}", device_line(device));
        if let Some(model) = &device.model {
            let _ = writeln!(out, "    {model}");
        }
        plan_notices(&mut out, "    ", &device.advice);
    }
    out
}

pub fn device(device: &DeviceInfo) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "{}", device_line(device));
    if let Some(model) = &device.model {
        let _ = writeln!(out, "Disk:       {model}");
    }
    if let Some(sector) = device.sector_size {
        let _ = writeln!(out, "Sector:     {sector} bytes");
    }
    if let Some(p) = &device.partition {
        let table = match p.table {
            Some(PartitionTable::Mbr) => "MBR",
            Some(PartitionTable::Gpt) => "GPT",
            Some(PartitionTable::None) => "none",
            None => "unknown",
        };
        let mut line = format!("{table}, partition {} at byte {}", p.number, p.offset);
        if let Some(kind) = p.mbr_type {
            let _ = write!(line, ", type 0x{kind:02X}");
        }
        let _ = writeln!(out, "Partition:  {line}");
    }
    for id in &device.unfinished {
        let _ = writeln!(out, "x unfinished operation {id}: run `yaoplm recover`");
    }
    if device.advice.is_empty() {
        let _ = writeln!(out, "No problems found.");
    }
    plan_notices(&mut out, "", &device.advice);
    out
}

fn step_line(step: &PlanStep) -> String {
    let bytes = step
        .bytes
        .map(|b| format!("  {}", size(b)))
        .unwrap_or_default();
    let to = step.to.as_deref().unwrap_or("?");
    match step.op {
        StepOp::CreateDir => format!("+ create folder {}", step.path),
        StepOp::RemoveDir => format!("- remove empty folder {}", step.path),
        StepOp::WriteFile => {
            let from = step
                .source
                .as_deref()
                .map(|s| format!("  <- {s}"))
                .unwrap_or_default();
            format!("+ write {}{bytes}{from}", step.path)
        }
        StepOp::PutFile => format!("~ update {}{bytes}", step.path),
        StepOp::CopyFile => format!("+ copy {} to {to}", step.path),
        StepOp::Rename => format!("~ rename {} to {to}", step.path),
        StepOp::Remove => match step.removal {
            Some(RemovalKind::UserData) => format!("- DELETE {}{bytes}", step.path),
            Some(RemovalKind::Cache) => format!("- remove {} (OPL cache)", step.path),
            _ => format!("- remove {}", step.path),
        },
        StepOp::RemoveSource => format!("- DELETE source {}{bytes}", step.path),
        StepOp::DropJournal => format!("- close journal {}", step.path),
    }
}

pub fn plan(plan: &PlanView) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "Plan: {} on {}", plan.title, plan.root);
    let _ = write!(
        out,
        "Writes {}, {} free",
        size(plan.write_bytes),
        size(plan.free_bytes)
    );
    if plan.delete_bytes > 0 {
        let _ = write!(out, "; DELETES {}", size(plan.delete_bytes));
    }
    let _ = writeln!(out);
    for (n, tx) in plan.transactions.iter().enumerate() {
        let mut title = format!("{}. {}", n + 1, tx.title);
        if let Some(game) = &tx.game {
            let _ = write!(
                title,
                "  [{} {} {}]",
                media(game.media),
                format(game.format),
                game.startup
            );
        }
        let _ = writeln!(out);
        let _ = writeln!(out, "{title}");
        for step in &tx.steps {
            let _ = writeln!(out, "     {}", step_line(step));
        }
        plan_notices(&mut out, "     ", &tx.notices);
    }
    if !plan.skipped.is_empty() {
        let _ = writeln!(out);
        let _ = writeln!(out, "Skipped ({}):", plan.skipped.len());
        for skipped in &plan.skipped {
            let _ = writeln!(out, "  {}: {}", skipped.path, skipped.reason.message);
        }
    }
    if !plan.notices.is_empty() {
        let _ = writeln!(out);
        plan_notices(&mut out, "", &plan.notices);
    }
    out
}

pub fn execution(plan: &PlanView, report: &ExecutionReport) -> String {
    let mut out = String::new();
    for (tx, outcome) in plan.transactions.iter().zip(&report.outcomes) {
        let state = match outcome {
            TxOutcome::Done => "done",
            TxOutcome::RolledBack => "undone",
            TxOutcome::NotStarted => "not started",
            TxOutcome::Interrupted => "INTERRUPTED",
        };
        let _ = writeln!(out, "{state:>12}  {}", tx.title);
    }
    if report.cancelled {
        let _ = writeln!(out, "Cancelled: the game in progress was undone.");
    }
    if let Some(error) = &report.error {
        let _ = writeln!(out, "Error: {error}");
    }
    if let Some(journal) = &report.journal {
        let _ = writeln!(
            out,
            "An operation is half done (journal {journal}). Run `yaoplm recover <root>` to undo it, or add --resume to finish it."
        );
    }
    out
}

/// One line of a cover search: `SLUS_200.62  found      xlenore  Retail`.
pub fn art_line(entry: &JobLogEntry) -> String {
    match entry {
        JobLogEntry::Art {
            startup,
            title,
            state,
            source,
            detail,
            ..
        } => {
            let state = match state {
                ArtLogState::Found => "found",
                ArtLogState::Cached => "cached",
                ArtLogState::NotFound => "not found",
                ArtLogState::Failed => "FAILED",
            };
            let via = match (source, detail) {
                (Some(source), _) => source.clone(),
                (None, Some(detail)) => detail.clone(),
                (None, None) => String::new(),
            };
            format!("{startup:<12} {state:<10} {via:<10} {title}")
        }
        _ => String::new(),
    }
}

/// The About screen as text.
pub fn about(about: &Attribution) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{} {} ({}) {}",
        about.name, about.version, about.license, about.repository
    );
    let _ = writeln!(
        out,
        "Title database: {} serials, built {}, Redump names from libretro-database {}",
        about.titles.entries, about.titles.built, about.titles.libretro_version
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "Bundled data:");
    for entry in &about.data {
        let _ = writeln!(
            out,
            "  {}: {} ({})",
            entry.name,
            entry.license.as_deref().unwrap_or("no licence"),
            entry.url
        );
        let _ = writeln!(out, "    {}", entry.credits);
    }
    let _ = writeln!(out);
    let _ = writeln!(out, "Cover sources (fetched on request, never bundled):");
    for entry in &about.art {
        let _ = writeln!(
            out,
            "  {}: {} ({})",
            entry.name,
            entry
                .license
                .as_deref()
                .unwrap_or("no licence stated; used on request only"),
            entry.url
        );
        let _ = writeln!(out, "    {}", entry.credits);
    }
    out
}

/// `1.2 GiB / 4.1 GiB (29%), 18.3 MiB/s  Game title`
pub fn progress(done: u64, total: Option<u64>, rate: Option<u64>, item: Option<&str>) -> String {
    let mut line = size(done);
    if let Some(total) = total.filter(|&t| t > 0) {
        let _ = write!(line, " / {} ({}%)", size(total), done * 100 / total);
    }
    if let Some(rate) = rate {
        let _ = write!(line, ", {}/s", size(rate));
    }
    if let Some(item) = item {
        let _ = write!(line, "  {item}");
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(size(0), "0 B");
        assert_eq!(size(1023), "1023 B");
        assert_eq!(size(1536), "1.5 KiB");
        assert_eq!(size(4_700_372_992), "4.4 GiB");
    }
}
