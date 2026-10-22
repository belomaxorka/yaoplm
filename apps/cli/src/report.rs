//! Human-readable output for `scan` and `info`.

use std::fmt::Write;
use std::path::Path;

use yaoplm_api::library::{
    GameFormat, ImageDetails, Library, LoaderProfile, Media, NameFormat, Notice, Platform,
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
            let _ = write!(title, " ({} parts)", game.parts.len());
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
        Some(id) => line(&mut out, "Game ID:", id),
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
            "{} (guessed from size; folder decides in OPL)",
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
