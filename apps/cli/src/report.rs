//! Human-readable output for every subcommand.

use std::fmt::Write;
use std::path::Path;

use yaoplm_api::art::{ArtLogState, Attribution};
use yaoplm_api::devices::{
    ActiveMode, ActiveProfile, DeviceInfo, DeviceMode, DriveBus, DriveKind, FileSystem, ModeSource,
    PartitionTable, ProfileSource,
};
use yaoplm_api::jobs::{ExecutionReport, JobLogEntry, TxOutcome};
use yaoplm_api::library::{
    Fragments, GameFormat, ImageDetails, Library, LoaderProfile, Media, NameFormat, Notice,
    Platform,
};
use yaoplm_api::loaders::{
    LoaderCatalog, LoaderChecksumSource, LoaderFile, LoaderOffer, LoaderPublisher, LoaderTarget,
};
use yaoplm_api::plans::{NoticeLevel, PlanNotice, PlanStep, PlanView, RemovalKind, StepOp};
use yaoplm_api::settings::{CfgView, VmcCard};
use yaoplm_api::verify::{
    GameVerification, RedumpInfo, UnknownReason, VerificationReport, VerifyStatus,
};

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

const FRAGMENTS_UNKNOWN: &str =
    "unknown: the system does not say where files lie on this device (on Linux, only root can ask)";

/// Pieces of a game on a FAT32 or exFAT device and OPL's rule (IMP-7b).
fn fragments(fragments: &Fragments) -> String {
    match fragments {
        Fragments::Known {
            total,
            limit: Some(limit),
            ..
        } => format!("{total} (OPL loads up to {limit} per game)"),
        Fragments::Known {
            total,
            files,
            limit: None,
        } if files.iter().all(|&n| n <= 1) => {
            format!("{total}: each file in one piece, as OPL 1.1.0 needs")
        }
        Fragments::Known {
            total, limit: None, ..
        } => format!("{total}: not every file is in one piece, and OPL 1.1.0 needs each to be"),
        Fragments::Unknown => FRAGMENTS_UNKNOWN.to_owned(),
    }
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
    // A column of pieces on the device, on FAT32 and exFAT only (IMP-7b).
    let pieces = lib.games.iter().any(|g| g.fragments.is_some());
    if !lib.games.is_empty() {
        let _ = write!(
            out,
            "  {:<5} {:<4} {:<12} {:>10}  ",
            "MEDIA", "FMT", "ID", "SIZE"
        );
        if pieces {
            let _ = write!(out, "{:>6}  ", "PIECES");
        }
        let _ = writeln!(out, "TITLE");
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
        let _ = write!(
            out,
            "  {:<5} {:<4} {:<12} {:>10}  ",
            media(game.media),
            format(game.format),
            game.startup,
            size(game.size),
        );
        if pieces {
            let count = match &game.fragments {
                Some(Fragments::Known { total, .. }) => total.to_string(),
                Some(Fragments::Unknown) => "?".to_owned(),
                None => String::new(),
            };
            let _ = write!(out, "{count:>6}  ");
        }
        let _ = writeln!(out, "{title}");
        notices(&mut out, "      ", &game.warnings);
    }
    if lib
        .games
        .iter()
        .any(|g| g.fragments == Some(Fragments::Unknown))
    {
        let _ = writeln!(out, "  Pieces ?: {FRAGMENTS_UNKNOWN}");
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
    if let Some(pieces) = &info.fragments {
        line(&mut out, "Pieces:", &fragments(pieces));
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

/// The loader profile in force and why (LIB-1).
pub fn loader_line(loader: &ActiveProfile) -> String {
    let name = profile_name(loader.profile);
    match (loader.source, &loader.detected) {
        (ProfileSource::Override, _) => format!("{name} (chosen for this device)"),
        (ProfileSource::Explicit, _) => format!("{name} (--profile)"),
        (ProfileSource::Detected, Some(detected)) => {
            format!("{name}: {} (auto)", detected.reason.english())
        }
        (ProfileSource::Detected, None) => format!("{name} (auto, the device could not be read)"),
    }
}

/// How OPL reads the device and why (ADR-0045).
pub fn mode_line(mode: &ActiveMode) -> String {
    let name = match mode.mode {
        DeviceMode::Ps2 => "plugged into the PS2",
        DeviceMode::Smb => "over the network (SMB)",
    };
    match mode.source {
        ModeSource::Override => format!("{name} (chosen for this device)"),
        ModeSource::Explicit => format!("{name} (--mode)"),
        ModeSource::Detected => format!("{name}: {} (auto)", mode.place_text),
    }
}

/// The question the interface asks when nothing on the device tells the
/// loader (ADR-0041, ADR-0045); a folder read over SMB is asked about the
/// OPL on the console.
fn profile_question(mode: DeviceMode) -> String {
    let question = match mode {
        DeviceMode::Ps2 => "Which OPL do you start with this device?",
        DeviceMode::Smb => "Which OPL reads this folder over the network?",
    };
    format!(
        "? {question} Nothing on it tells: pass --profile opl-1.2, opl-1.1 or riptopl \
         (opl-1.1 if you do not know)"
    )
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
        let _ = writeln!(out, "    Mode: {}", mode_line(&device.mode));
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
    let _ = writeln!(out, "Mode:       {}", mode_line(&device.mode));
    let _ = writeln!(out, "Loader:     {}", loader_line(&device.loader));
    if let Some(detected) = &device.loader.detected {
        for loader in &detected.loaders {
            let mut found = loader.family.clone();
            if let Some(version) = &loader.version {
                let _ = write!(found, " \u{201c}{version}\u{201d}");
            }
            if loader.exact {
                found.push_str(" (by checksum)");
            }
            let _ = writeln!(out, "Found:      {found} at {}", loader.path);
        }
        plan_notices(&mut out, "", &detected.notices);
    }
    if device.ask_profile {
        let _ = writeln!(out, "{}", profile_question(device.mode.mode));
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
        StepOp::WriteZso => {
            let from = step
                .source
                .as_deref()
                .map(|s| format!("  <- {s}"))
                .unwrap_or_default();
            let at_most = step
                .bytes
                .map(|b| format!("  at most {}", size(b)))
                .unwrap_or_default();
            format!("+ compress to {}{at_most}{from}", step.path)
        }
        StepOp::WriteVmc => format!("+ format memory card {}{bytes}", step.path),
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
        for change in &tx.cfg_changes {
            let _ = writeln!(
                out,
                "     {}: {} -> {}",
                change.key,
                change.before.as_deref().unwrap_or("(none)"),
                change.after.as_deref().unwrap_or("(removed)")
            );
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
    plan_notices(&mut out, "", &report.notices);
    if let Some(journal) = &report.journal {
        let _ = writeln!(
            out,
            "An operation is half done (journal {journal}). Run `yaoplm recover <root>` to undo it, or add --resume to finish it."
        );
    }
    if !report.checks.is_empty() {
        let _ = writeln!(out, "Dump checks, from the bytes as they were written:");
        for game in &report.checks {
            verification_game(&mut out, game);
        }
        verification_counts(&mut out, &report.checks);
    }
    out
}

/// OPL's compatibility modes by bit (research/02 §6.3; RiptOPL adds 7, 8).
const MODES: [&str; 8] = [
    "accurate reads",
    "synchronous mode",
    "unhook syscalls",
    "skip videos",
    "emulate DVD-DL",
    "disable IGR",
    "RiptOPL: Neutrino buffer fix",
    "RiptOPL: slow reads",
];

fn on_off(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}

/// `yaoplm cfg <root> <game>`.
pub fn cfg(view: &CfgView) -> String {
    let mut out = String::new();
    let state = match (view.exists, view.editable) {
        (false, _) => " (not there yet: OPL uses its defaults)",
        (true, false) => " (wOPL libconfig: shown only, never rewritten)",
        (true, true) => "",
    };
    let _ = writeln!(out, "{}  {}{state}", view.startup, view.path);
    let s = &view.settings;
    let modes: Vec<String> = (0..8)
        .filter(|bit| s.compatibility & (1 << bit) != 0)
        .map(|bit| format!("{} ({})", bit + 1, MODES[bit as usize]))
        .collect();
    let _ = writeln!(
        out,
        "Modes:        {}",
        if modes.is_empty() {
            "none".to_owned()
        } else {
            modes.join(", ")
        }
    );
    for (slot, name) in s.vmc.iter().enumerate() {
        let shown = if name.is_empty() { "-" } else { name.as_str() };
        let _ = writeln!(out, "VMC slot {}:   {shown}", slot + 1);
    }
    if !s.alt_startup.is_empty() {
        let _ = writeln!(out, "Alt. startup: {}", s.alt_startup);
    }
    if let Some(dma) = s.dma {
        let _ = writeln!(out, "DMA:          {dma} (HDD)");
    }
    let gsm = &s.gsm;
    let _ = writeln!(
        out,
        "GSM:          {}",
        if gsm.per_game {
            format!(
                "{} mode {}, offsets {}/{}, field fix {}",
                on_off(gsm.enabled),
                gsm.vmode,
                gsm.x_offset,
                gsm.y_offset,
                on_off(gsm.field_fix)
            )
        } else {
            "global".to_owned()
        }
    );
    let cheats = &s.cheats;
    let _ = writeln!(
        out,
        "Cheats:       {}",
        if cheats.per_game {
            format!(
                "{}, {}",
                on_off(cheats.enabled),
                if cheats.mode == 0 {
                    "all codes"
                } else {
                    "pick at launch"
                }
            )
        } else {
            "global".to_owned()
        }
    );
    let pad = &s.pad_emu;
    let _ = writeln!(
        out,
        "PADEMU:       {}",
        if pad.per_game {
            format!("{}, settings 0x{:08X}", on_off(pad.enabled), pad.settings)
        } else {
            "global".to_owned()
        }
    );
    if !view.entries.is_empty() {
        let _ = writeln!(out, "Keys:");
        for entry in &view.entries {
            let _ = writeln!(out, "  {}={}", entry.key, entry.value);
        }
    }
    out
}

/// `yaoplm vmc ls <root>`.
pub fn vmcs(cards: &[VmcCard]) -> String {
    let mut out = String::new();
    if cards.is_empty() {
        let _ = writeln!(out, "No memory cards in VMC/.");
        return out;
    }
    for card in cards {
        let state = match (card.size_mb, &card.problem) {
            (Some(mb), _) => {
                let saves = card
                    .saves
                    .map(|n| format!(", {n} saves"))
                    .unwrap_or_default();
                format!("{mb} MiB{saves}")
            }
            (None, Some(problem)) => format!("{}: OPL refuses it: {problem}", size(card.size)),
            (None, None) => size(card.size),
        };
        let users: Vec<String> = card
            .used_by
            .iter()
            .map(|u| format!("{} slot {}", u.startup, u.slot + 1))
            .collect();
        let used = if users.is_empty() {
            String::new()
        } else {
            format!("  used by {}", users.join(", "))
        };
        let _ = writeln!(out, "{}  {state}{used}", card.name);
    }
    out
}

/// One line of a cover search: `SLUS_200.62  found      xlenore  Retail`.
pub fn art_line(entry: &JobLogEntry) -> String {
    match entry {
        JobLogEntry::Art {
            startup,
            title,
            art,
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
            format!("{startup:<12} {art:<4} {state:<10} {via:<10} {title}")
        }
        _ => String::new(),
    }
}

/// The About screen as text.
fn target_name(target: LoaderTarget) -> &'static str {
    match target {
        LoaderTarget::Usb => "usb",
        LoaderTarget::Mx4sio => "mx4sio",
    }
}

/// `yaoplm loader ls`: one block per build, texts in `lang`.
pub fn loaders(catalog: &LoaderCatalog, lang: &str) -> String {
    let mut out = String::new();
    for build in &catalog.builds {
        let text = |texts: &std::collections::BTreeMap<String, String>| {
            texts
                .get(lang)
                .or_else(|| texts.get("en"))
                .cloned()
                .unwrap_or_default()
        };
        let publisher = match build.publisher {
            LoaderPublisher::Official => "official",
            LoaderPublisher::Fork => "fork",
        };
        let channel = if build.channel.floating {
            "rebuilt in place"
        } else {
            "fixed"
        };
        let _ = writeln!(
            out,
            "{}  {}{}",
            build.id,
            build.name,
            if build.recommended {
                "  [recommended]"
            } else {
                ""
            }
        );
        let _ = writeln!(
            out,
            "  {publisher}, {} @ {} ({channel}), profile {}",
            build.repo,
            build.channel.tag,
            profile_name(build.profile)
        );
        let _ = writeln!(out, "  {}", text(&build.summary));
        let _ = writeln!(out, "  Limits: {}", text(&build.limits));
        for preset in &build.presets {
            let devices: Vec<&str> = preset.devices.iter().map(|d| target_name(*d)).collect();
            let defaults: Vec<&str> = build
                .default_presets
                .iter()
                .filter(|d| d.preset == preset.id)
                .map(|d| target_name(d.device))
                .collect();
            let _ = write!(
                out,
                "  Preset {}: {} ({})",
                preset.id,
                preset.path,
                devices.join(", ")
            );
            if let Some(title) = &preset.title {
                let _ = write!(out, ", title.cfg \"{title}\"");
            }
            if !defaults.is_empty() {
                let _ = write!(out, ", default on {}", defaults.join(", "));
            }
            let _ = writeln!(out);
        }
        let _ = writeln!(
            out,
            "  Licence: {}, {} ({})",
            build.license.spdx, build.license.authors, build.license.url
        );
        let _ = writeln!(out);
    }
    out
}

fn checksum_source(source: LoaderChecksumSource) -> &'static str {
    match source {
        LoaderChecksumSource::Digest => "GitHub digest",
        LoaderChecksumSource::Sums => "the release's SHA256SUMS",
        LoaderChecksumSource::Pinned => "the catalogue",
    }
}

/// `yaoplm loader release`: what a download would be.
pub fn loader_offer(offer: &LoaderOffer) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{} {} (release {}{})",
        offer.build,
        offer.version,
        offer.tag,
        offer
            .date
            .as_deref()
            .map(|d| format!(", {d}"))
            .unwrap_or_default()
    );
    let file = &offer.file;
    let _ = write!(out, "  File: {}, {}", file.name, size(file.size));
    if let Some(member) = &offer.member {
        let _ = write!(out, ", the ELF is {member}");
    }
    let _ = writeln!(out);
    match &file.sha256 {
        Some(sha) => {
            let sources: Vec<&str> = file.sources.iter().map(|s| checksum_source(*s)).collect();
            let _ = writeln!(out, "  SHA-256: {sha} (from {})", sources.join(" and "));
        }
        None => {
            let _ = writeln!(
                out,
                "  SHA-256: not published; YAOPLM shows the one it computes"
            );
        }
    }
    let _ = writeln!(
        out,
        "  Licence: {}, {} ({})",
        offer.license.spdx, offer.license.authors, offer.license.url
    );
    let _ = writeln!(out, "  Release: {}", offer.release_url);
    if let Some(commit) = &offer.commit_url {
        let _ = writeln!(out, "  Commit: {commit}");
    }
    if offer.changelog.is_empty() {
        let _ = writeln!(out, "  No change list was published.");
    } else {
        let _ = writeln!(out, "  Changes:");
        for item in &offer.changelog {
            match &item.link {
                Some(link) => {
                    let _ = writeln!(out, "    - {} ({link})", item.text);
                }
                None => {
                    let _ = writeln!(out, "    - {}", item.text);
                }
            }
        }
        if offer.changelog_more {
            let _ = writeln!(out, "    … and more on the release page");
        }
    }
    if offer.offline {
        let _ = writeln!(
            out,
            "  GitHub could not be reached: shown from the last stored answer."
        );
    }
    if let Some(sha) = &offer.stored {
        let _ = writeln!(out, "  Already stored: {sha}");
    }
    out
}

/// One stored loader ELF.
pub fn loader_file(file: &LoaderFile) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{} {} ({})",
        file.build.as_deref().unwrap_or("no catalogue build"),
        file.version.as_deref().unwrap_or("version unknown"),
        size(file.size)
    );
    let _ = writeln!(out, "  SHA-256: {}", file.sha256);
    let from = match &file.member {
        Some(member) => format!("{member} in {}", file.file),
        None => file.file.clone(),
    };
    let _ = writeln!(out, "  From: {from} ({})", file.source);
    let trust = match (&file.tag, file.confirmed, &file.stable) {
        (Some(_), true, _) => "confirmed by its release before the download".to_owned(),
        (Some(_), false, _) => "the release published no checksum for it".to_owned(),
        (None, _, Some(stable)) => format!("your own file; it is the official {stable}"),
        (None, _, None) => "your own file; nothing vouches for it".to_owned(),
    };
    let _ = writeln!(out, "  {trust}");
    out
}

/// `yaoplm loader files`.
pub fn loader_files(files: &[LoaderFile]) -> String {
    if files.is_empty() {
        return "No loader stored yet.\n".to_owned();
    }
    files.iter().map(loader_file).collect::<Vec<_>>().join("\n")
}

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

fn verify_status(status: VerifyStatus, reason: Option<UnknownReason>) -> &'static str {
    match (status, reason) {
        (VerifyStatus::Verified, _) => "verified",
        (VerifyStatus::BadDump, _) => "BAD DUMP",
        (VerifyStatus::Failed, _) => "FAILED",
        (VerifyStatus::Unknown, Some(UnknownReason::NoDat)) => "no DAT",
        (VerifyStatus::Unknown, Some(UnknownReason::CdImage)) => "CD ISO",
        (VerifyStatus::Unknown, _) => "unknown",
    }
}

/// One live line of a `VerifyGames` job.
pub fn verify_line(entry: &JobLogEntry) -> String {
    match entry {
        JobLogEntry::Verify {
            startup,
            title,
            status,
            reason,
            redump,
            cached,
            error,
            ..
        } => {
            let detail = redump
                .clone()
                .or_else(|| error.clone())
                .unwrap_or_else(|| title.clone());
            let cached = if *cached { " (cached)" } else { "" };
            format!(
                "{startup:<12} {:<9} {detail}{cached}",
                verify_status(*status, *reason)
            )
        }
        _ => String::new(),
    }
}

pub fn redump(dat: Option<&RedumpInfo>) -> String {
    let Some(dat) = dat else {
        return "No Redump DAT yet: `yaoplm redump --update` downloads one (at most once a week), `yaoplm redump --import FILE` uses one you have.\n".to_owned();
    };
    let mut out = String::new();
    let _ = writeln!(out, "{}", dat.description);
    let _ = writeln!(
        out,
        "  {} games{}, from {}",
        dat.games,
        if dat.serials {
            " with serials"
        } else {
            ", no serials"
        },
        dat.source
    );
    let _ = writeln!(out, "  stored at {}", dat.path);
    if let Some(next) = dat.next_check {
        let _ = writeln!(out, "  Redump is asked again after {}", date(next));
    }
    out
}

/// `2026-10-30` (UTC) from milliseconds since 1970.
pub fn date(ms: u64) -> String {
    // Days to a civil date: Howard Hinnant's `civil_from_days`.
    let days = (ms / 86_400_000) as i64 + 719_468;
    let era = days.div_euclid(146_097);
    let doe = days.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

pub fn verification(report: &VerificationReport) -> String {
    let mut out = String::new();
    match &report.dat {
        Some(dat) => {
            let _ = writeln!(out, "Compared with {}", dat.description);
        }
        None => {
            let _ = writeln!(
                out,
                "No Redump DAT: hashes only (`yaoplm redump --update` or --import FILE)"
            );
        }
    }
    for game in &report.games {
        verification_game(&mut out, game);
    }
    verification_counts(&mut out, &report.games);
    out
}

fn verification_game(out: &mut String, game: &GameVerification) {
    let _ = writeln!(
        out,
        "{:<12} {:<9} {}",
        game.startup,
        verify_status(game.status, game.reason),
        game.title
    );
    if let Some(entry) = &game.redump {
        let _ = writeln!(
            out,
            "    Redump: {}{}",
            entry.name,
            entry
                .url
                .as_deref()
                .map(|u| format!(" ({u})"))
                .unwrap_or_default()
        );
    }
    if game.status == VerifyStatus::BadDump {
        for candidate in &game.candidates {
            let _ = writeln!(
                out,
                "    differs from {} ({}{})",
                candidate.name,
                size(candidate.size),
                if candidate.same_size {
                    ", same size"
                } else {
                    ""
                }
            );
        }
    }
    if game.reason == Some(UnknownReason::CdImage) && game.track.is_none() {
        let _ = writeln!(
            out,
            "    Redump lists this CD as BIN/CUE; an ISO cannot match its hashes"
        );
    }
    if let Some(hashes) = &game.hashes {
        let _ = writeln!(out, "    CRC-32 {}  SHA-1 {}", hashes.crc32, hashes.sha1);
        if let Some(md5) = &hashes.md5 {
            let _ = writeln!(out, "    MD5 {md5}");
        }
    }
    if let Some(track) = &game.track {
        let _ = writeln!(
            out,
            "    data track of the BIN it was added from: CRC-32 {}  SHA-1 {}",
            track.crc32, track.sha1
        );
    }
    if let Some(error) = &game.error {
        let _ = writeln!(out, "    {error}");
    }
}

fn verification_counts(out: &mut String, games: &[GameVerification]) {
    let count = |status| games.iter().filter(|g| g.status == status).count();
    let _ = writeln!(
        out,
        "{} verified, {} bad, {} unknown, {} failed",
        count(VerifyStatus::Verified),
        count(VerifyStatus::BadDump),
        count(VerifyStatus::Unknown),
        count(VerifyStatus::Failed)
    );
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

    #[test]
    fn dates() {
        assert_eq!(date(0), "1970-01-01");
        assert_eq!(date(951_868_800_000), "2000-03-01");
        assert_eq!(date(1_709_164_800_000), "2024-02-29");
        assert_eq!(date(1_798_761_599_999), "2026-12-31");
    }

    fn game(id: &str, fragments: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "title": "Game", "startup": id, "media": "DVD", "format": "ISO",
            "path": format!("E:/DVD/{id}.iso"), "parts": [], "size": 1u64 << 30,
            "art": [], "warnings": [], "fragments": fragments,
        })
    }

    #[test]
    fn pieces_on_bdm_devices() {
        let lib: Library = serde_json::from_value(serde_json::json!({
            "root": "E:/", "profile": "Opl120Beta", "folders": [], "apps": [],
            "skipped": [], "warnings": [],
            "games": [
                game("SLUS_200.62", serde_json::json!({"kind": "known", "total": 70, "files": [70], "limit": 64})),
                game("SLUS_212.18", serde_json::json!({"kind": "unknown"})),
            ],
        }))
        .unwrap();
        let text = library(&lib);
        assert!(text.contains("PIECES  TITLE"), "{text}");
        assert!(text.contains("    70  Game"), "{text}");
        assert!(text.contains("     ?  Game"), "{text}");
        assert!(text.contains("Pieces ?: unknown"), "{text}");

        // Off BDM there is no column at all.
        let lib: Library = serde_json::from_value(serde_json::json!({
            "root": "C:/lib", "profile": "Opl120Beta", "folders": [], "apps": [],
            "skipped": [], "warnings": [],
            "games": [game("SLUS_200.62", serde_json::Value::Null)],
        }))
        .unwrap();
        assert!(!library(&lib).contains("PIECES"));

        assert_eq!(
            fragments(&Fragments::Known {
                total: 2,
                files: vec![1, 1],
                limit: None
            }),
            "2: each file in one piece, as OPL 1.1.0 needs"
        );
        assert!(
            fragments(&Fragments::Known {
                total: 2,
                files: vec![2],
                limit: None
            })
            .starts_with("2: not every file")
        );
    }
}
