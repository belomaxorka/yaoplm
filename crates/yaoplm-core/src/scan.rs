//! Library scanner: lists a device root the way OPL does (LIB-2, LIB-4).
//!
//! Mirrors `sbReadList()` and `scanForISO()` in `src/supportbase.c` of
//! ps2homebrew/Open-PS2-Loader@b3a21997 (research/02 §4): `CD/` then `DVD/`,
//! old-style names without mounting, `games.bin` hits without mounting,
//! everything else through `SYSTEM.CNF`; then `ul.cfg`. On top of that it
//! collects the files OPL derives from the game ID (CFG, ART, CHT, VMC) and
//! warns about what OPL would do differently than expected.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

use yaoplm_formats::art::{self, ArtFileName};
use yaoplm_formats::cfg::{self, CfgDocument, Dialect};
use yaoplm_formats::cht::ChtFile;
use yaoplm_formats::image::{self, Container, DiscImage};
use yaoplm_formats::iso_name::{self, ImageExtension, NameFormat, NameSkip};
use yaoplm_formats::iso9660::{self, Layers, PVD_LBA};
use yaoplm_formats::ul_cfg::{self, UlCfg};
use yaoplm_formats::zso::{ZsoError, ZsoProblem};
use yaoplm_formats::{LoaderProfile, Media, apps, games_bin, vmc};
use yaoplm_i18n::Phrase;

use crate::fsutil::{Entry, Listing, read_prefix};
use crate::model::{
    App, ArtFile, CfgSummary, CheatSummary, Game, GameFormat, GameLocation, Library, SkipReason,
    Skipped, UlPart, Warning,
};
use crate::multipart::MultiPartFile;
use crate::reasons;

/// Folders `sbCreateFolders()` makes, in the order OPL creates them.
pub const OPL_FOLDERS: [&str; 9] = [
    "CFG", "THM", "LNG", "ART", "VMC", "CHT", "APPS", "CD", "DVD",
];

/// 80-minute CD: 360,000 sectors.
pub const CD_MAX_BYTES: u64 = 360_000 * 2048;

/// Disc-image extensions people put in `CD/`/`DVD/` that OPL never lists.
const FOREIGN_IMAGE_EXTENSIONS: [&str; 12] = [
    "bin", "img", "cso", "chd", "cue", "nrg", "mdf", "mds", "ccd", "zip", "7z", "rar",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanOptions {
    pub profile: LoaderProfile,
    /// Also open images whose ID OPL takes from the name or `games.bin`, to
    /// catch mismatches. Costs a few sector reads per game.
    pub verify_images: bool,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            profile: LoaderProfile::default(),
            verify_images: true,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("{path}: {source}")]
    Io { path: PathBuf, source: io::Error },
    #[error("{0} is not a folder")]
    NotADirectory(PathBuf),
    #[error("cancelled")]
    Cancelled,
}

impl ScanError {
    /// For front ends that translate it (ADR-0029).
    pub fn phrase(&self) -> Phrase {
        match self {
            Self::Io { path, source } => Phrase::error("pathError")
                .with("path", path.display().to_string())
                .with("error", source.to_string()),
            Self::NotADirectory(path) => {
                Phrase::error("notAFolder").with("path", path.display().to_string())
            }
            Self::Cancelled => Phrase::error("cancelled"),
        }
    }
}

fn io_err(path: &Path) -> impl FnOnce(io::Error) -> ScanError + '_ {
    move |source| ScanError::Io {
        path: path.to_path_buf(),
        source,
    }
}

fn listing(dir: Option<&Entry>) -> Result<Listing, ScanError> {
    match dir {
        Some(dir) => Listing::read(&dir.path).map_err(io_err(&dir.path)),
        None => Ok(Listing::default()),
    }
}

/// What OPL would read from one image.
struct Probe {
    container: Option<Container>,
    startup: Result<String, Phrase>,
    layers: Option<Layers>,
    zso_problems: Vec<ZsoProblem>,
}

fn probe_image(path: &Path, media: Media) -> Probe {
    let failed = |why: Phrase| Probe {
        container: None,
        startup: Err(why),
        layers: None,
        zso_problems: Vec::new(),
    };
    let file = match File::open(path) {
        Ok(file) => file,
        Err(err) => {
            return failed(Phrase::reason("cannotOpen").with("error", err.to_string()));
        }
    };
    probe_reader(file, media).unwrap_or_else(failed)
}

fn probe_reader<R: io::Read + io::Seek>(reader: R, media: Media) -> Result<Probe, Phrase> {
    let mut image = match DiscImage::open(reader) {
        Ok(image) => image,
        Err(ZsoError::Unsupported(problem)) => {
            return Ok(Probe {
                container: Some(Container::Zso),
                startup: Err(
                    Phrase::reason("zsoUnreadable").with("reason", reasons::zso_problem(&problem))
                ),
                layers: None,
                zso_problems: vec![problem],
            });
        }
        Err(err) => return Err(reasons::zso(&err)),
    };
    let zso_problems = image.zso_header().map(|h| h.problems()).unwrap_or_default();
    let startup = image::opl_startup(&mut image).map_err(|e| reasons::startup(&e));
    // OPL only looks for a second layer on DVD media.
    let layers = (media == Media::Dvd)
        .then(|| iso9660::read_pvd(&mut image, PVD_LBA).ok())
        .flatten()
        .map(|pvd| iso9660::opl_layers(&mut image, &pvd));
    Ok(Probe {
        container: Some(image.container()),
        startup,
        layers,
        zso_problems,
    })
}

fn container_format(container: Container) -> GameFormat {
    match container {
        Container::Iso => GameFormat::Iso,
        Container::Zso => GameFormat::Zso,
    }
}

struct Scanner<'a> {
    options: &'a ScanOptions,
    library: Library,
    /// Entries looked at so far and in total; returns false to stop.
    progress: &'a mut dyn FnMut(u64, u64) -> bool,
    done: u64,
    total: u64,
}

impl Scanner<'_> {
    fn tick(&mut self) -> Result<(), ScanError> {
        self.done += 1;
        if (self.progress)(self.done.min(self.total), self.total) {
            Ok(())
        } else {
            Err(ScanError::Cancelled)
        }
    }

    fn scan_image_folder(&mut self, dir: &Entry, media: Media) -> Result<(), ScanError> {
        let listing = Listing::read(&dir.path).map_err(io_err(&dir.path))?;
        let cache = listing
            .find_file(games_bin::FILE_NAME)
            .and_then(|e| fs::read(&e.path).ok())
            .map(|bytes| games_bin::parse(&bytes))
            .unwrap_or_default();

        for entry in &listing.entries {
            self.tick()?;
            if entry.name.eq_ignore_ascii_case(games_bin::FILE_NAME) {
                continue;
            }
            let name = match iso_name::classify(&entry.name) {
                Ok(name) => name,
                Err(NameSkip::TitleTooLong) => {
                    self.skip(entry, SkipReason::TitleTooLong);
                    continue;
                }
                Err(NameSkip::NotAnImage) => {
                    let ext = entry
                        .name
                        .rsplit_once('.')
                        .map(|(_, e)| e.to_ascii_lowercase());
                    if let Some(ext) =
                        ext.filter(|e| FOREIGN_IMAGE_EXTENSIONS.contains(&e.as_str()))
                    {
                        self.skip(entry, SkipReason::UnsupportedExtension(format!(".{ext}")));
                    }
                    continue;
                }
            };
            if entry.is_dir {
                self.skip(entry, SkipReason::NoStartup(Phrase::reason("isAFolder")));
                continue;
            }

            let cached = (name.format == NameFormat::New)
                .then(|| games_bin::lookup(&cache, &entry.name))
                .flatten();
            let must_mount = name.format == NameFormat::New && cached.is_none();
            let probe =
                (must_mount || self.options.verify_images).then(|| probe_image(&entry.path, media));

            let mut warnings = Vec::new();
            let startup = match (&name.startup, cached, &probe) {
                (Some(from_name), _, _) => from_name.clone(),
                (None, Some(cached), _) => cached.startup.clone(),
                (None, None, Some(probe)) => match &probe.startup {
                    Ok(id) => id.clone(),
                    Err(why) => {
                        self.skip(entry, SkipReason::NoStartup(why.clone()));
                        continue;
                    }
                },
                (None, None, None) => continue,
            };
            let image_startup = probe.as_ref().and_then(|p| p.startup.clone().ok());
            if let Some(image) = &image_startup
                && *image != startup
            {
                warnings.push(match name.format {
                    NameFormat::Old => Warning::NameIdMismatch {
                        name: startup.clone(),
                        image: image.clone(),
                    },
                    NameFormat::New => Warning::StaleCache {
                        cached: startup.clone(),
                        image: image.clone(),
                    },
                });
            }

            let by_extension = match name.extension {
                ImageExtension::Iso => GameFormat::Iso,
                ImageExtension::Zso => GameFormat::Zso,
            };
            let format = probe
                .as_ref()
                .and_then(|p| p.container)
                .map_or(by_extension, container_format);
            if format != by_extension {
                warnings.push(Warning::ContainerMismatch {
                    extension: by_extension,
                    content: format,
                });
            }
            if format == GameFormat::Zso && !self.options.profile.reads_zso() {
                warnings.push(Warning::ZsoUnsupportedByProfile);
            }
            for problem in probe.iter().flat_map(|p| &p.zso_problems) {
                warnings.push(Warning::ZsoHeader(*problem));
            }
            if !name.lowercase_extension() {
                warnings.push(Warning::UppercaseExtension);
            }
            if !entry.name.is_ascii() {
                warnings.push(Warning::NonAsciiName);
            }
            if !iso_name::fits_smb(&entry.name) {
                warnings.push(Warning::SmbNameTooLong(entry.name.len()));
            }
            if media == Media::Cd && entry.len > CD_MAX_BYTES {
                warnings.push(Warning::CdTooLarge(entry.len));
            }

            self.library.games.push(Game {
                title: name.title.clone(),
                startup,
                media,
                format,
                location: GameLocation::Image {
                    path: entry.path.clone(),
                    file_name: entry.name.clone(),
                    name_format: name.format,
                },
                size: entry.len,
                image_startup,
                layers: probe.and_then(|p| p.layers),
                cfg: None,
                art: Vec::new(),
                cheats: None,
                warnings,
            });
        }
        Ok(())
    }

    fn scan_ul(&mut self, top: &Listing, ul: &Entry) -> Result<(), ScanError> {
        let bytes = fs::read(&ul.path).map_err(io_err(&ul.path))?;
        let table = UlCfg::parse(&bytes);
        if !table.trailing.is_empty() {
            self.library
                .warnings
                .push(Warning::UlCfgTrailingBytes(table.trailing.len()));
        }
        for (index, record) in table.records.iter().enumerate() {
            self.tick()?;
            let mut warnings: Vec<Warning> = record
                .anomalies()
                .into_iter()
                .map(Warning::UlRecord)
                .collect();
            let count = record.parts();
            let mut parts = Vec::new();
            for part in 0..count {
                let name = record.chunk_name(part);
                let found = top.find_file(&name);
                match found {
                    None => warnings.push(Warning::UlPartMissing {
                        part,
                        name: name.clone(),
                    }),
                    Some(e) if part + 1 < count && e.len != ul_cfg::CHUNK_SIZE => {
                        warnings.push(Warning::UlPartSize {
                            part,
                            name: name.clone(),
                            size: e.len,
                        });
                    }
                    Some(_) => {}
                }
                parts.push(UlPart {
                    name,
                    path: found.map(|e| e.path.clone()),
                    size: found.map(|e| e.len),
                });
            }
            if !ul_cfg::fits_smb(usize::from(count)) {
                warnings.push(Warning::UlTooManyPartsForSmb(count));
            }

            let complete: Option<Vec<(PathBuf, u64)>> = parts
                .iter()
                .map(|p| Some((p.path.clone()?, p.size?)))
                .collect();
            let probe = complete
                .filter(|parts| self.options.verify_images && !parts.is_empty())
                .map(|parts| probe_reader(MultiPartFile::new(parts), record.media()));
            let probe = probe.and_then(Result::ok);
            let image_startup = probe.as_ref().and_then(|p| p.startup.clone().ok());
            if let Some(image) = &image_startup
                && *image != record.startup()
            {
                warnings.push(Warning::NameIdMismatch {
                    name: record.startup(),
                    image: image.clone(),
                });
            }

            self.library.games.push(Game {
                title: record.name(),
                startup: record.startup(),
                media: record.media(),
                format: GameFormat::Ul,
                size: parts.iter().filter_map(|p| p.size).sum(),
                location: GameLocation::Ul { index, parts },
                image_startup,
                layers: probe.and_then(|p| p.layers),
                cfg: None,
                art: Vec::new(),
                cheats: None,
                warnings,
            });
        }
        Ok(())
    }

    fn skip(&mut self, entry: &Entry, reason: SkipReason) {
        self.library.skipped.push(Skipped {
            path: entry.path.clone(),
            reason,
        });
    }
}

/// Art files grouped by lower-case key; FAT matches names case-insensitively.
fn index_art(listing: &Listing) -> HashMap<String, Vec<(&Entry, ArtFileName)>> {
    let mut index: HashMap<String, Vec<(&Entry, ArtFileName)>> = HashMap::new();
    for entry in listing.files() {
        if let Some(parsed) = art::parse_art_file_name(&entry.name) {
            index
                .entry(parsed.key.to_lowercase())
                .or_default()
                .push((entry, parsed));
        }
    }
    index
}

fn collect_art(
    index: &HashMap<String, Vec<(&Entry, ArtFileName)>>,
    key: &str,
    profile: LoaderProfile,
    warnings: &mut Vec<Warning>,
) -> Vec<ArtFile> {
    let Some(files) = index.get(&key.to_lowercase()) else {
        return Vec::new();
    };
    files
        .iter()
        .map(|(entry, parsed)| {
            let dimensions = read_prefix(&entry.path, 26)
                .ok()
                .and_then(|bytes| art::png_header(&bytes))
                .map(|h| (h.width, h.height));
            if !parsed.loads_in(profile) {
                warnings.push(Warning::ArtNotLoaded(entry.name.clone()));
            }
            if let Some((width, height)) = dimensions
                && (width > art::MAX_DIMENSION || height > art::MAX_DIMENSION)
            {
                warnings.push(Warning::ArtTooLarge {
                    file: entry.name.clone(),
                    width,
                    height,
                });
            }
            ArtFile {
                path: entry.path.clone(),
                suffix: parsed.suffix.clone(),
                dimensions,
                size: entry.len,
                modified: entry.modified,
            }
        })
        .collect()
}

fn read_cfg(
    path: &Path,
    profile: LoaderProfile,
    vmcs: &Listing,
    warnings: &mut Vec<Warning>,
) -> CfgSummary {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) => {
            warnings.push(Warning::CfgUnreadable(err.to_string()));
            return CfgSummary {
                path: path.to_path_buf(),
                dialect: Dialect::Opl,
                compatibility: None,
                vmc: [None, None],
            };
        }
    };
    let dialect = cfg::detect_dialect(&bytes);
    if dialect == Dialect::Libconfig {
        warnings.push(Warning::CfgLibconfig);
        return CfgSummary {
            path: path.to_path_buf(),
            dialect,
            compatibility: None,
            vmc: [None, None],
        };
    }
    let doc = CfgDocument::parse(&bytes);
    let compatibility = doc.get_int("$Compatibility");
    if let Some(value) = compatibility
        && value as u32 & !profile.compat_mask() != 0
    {
        warnings.push(Warning::CompatibilityBits {
            value,
            allowed: profile.compat_mask(),
        });
    }
    let vmc = [doc.get("$VMC_0"), doc.get("$VMC_1")].map(|name| name.filter(|n| !n.is_empty()));
    for (slot, name) in vmc.iter().enumerate() {
        let Some(name) = name else { continue };
        let slot = slot as u8;
        match vmcs.find_file(&vmc::file_name(name)) {
            None => warnings.push(Warning::VmcMissing {
                slot,
                name: name.clone(),
            }),
            Some(file) => {
                let superblock = read_prefix(&file.path, vmc::SUPERBLOCK_SIZE).unwrap_or_default();
                if let Err(problem) = vmc::opl_check(&superblock, file.len) {
                    warnings.push(Warning::VmcInvalid {
                        slot,
                        name: name.clone(),
                        problem,
                    });
                }
            }
        }
    }
    CfgSummary {
        path: path.to_path_buf(),
        dialect,
        compatibility,
        vmc,
    }
}

fn read_cheats(path: &Path, warnings: &mut Vec<Warning>) -> CheatSummary {
    let file = fs::read(path)
        .map(|b| ChtFile::parse(&b))
        .unwrap_or_default();
    warnings.extend(file.warnings.into_iter().map(Warning::Cheats));
    CheatSummary {
        path: path.to_path_buf(),
        cheats: file.cheats.len(),
    }
}

fn scan_apps(
    dir: Option<&Entry>,
    art: &HashMap<String, Vec<(&Entry, ArtFileName)>>,
    profile: LoaderProfile,
    library: &mut Library,
) -> Result<(), ScanError> {
    let apps_dir = listing(dir)?;
    for folder in apps_dir.entries.iter().filter(|e| e.is_dir) {
        let inside = Listing::read(&folder.path).map_err(io_err(&folder.path))?;
        // Folders without title.cfg are not apps at all.
        let Some(title_cfg) = inside.find_file(apps::TITLE_CFG) else {
            continue;
        };
        let bytes = fs::read(&title_cfg.path).map_err(io_err(&title_cfg.path))?;
        let entry = match apps::parse_title_cfg(&bytes) {
            Ok(entry) => entry,
            Err(err) => {
                library.skipped.push(Skipped {
                    path: folder.path.clone(),
                    reason: SkipReason::BadApp(reasons::app(&err)),
                });
                continue;
            }
        };
        let mut warnings = Vec::new();
        let boot_found = inside.find_file(&entry.boot).is_some();
        if !boot_found {
            warnings.push(Warning::AppBootMissing(entry.boot.clone()));
        }
        let art = collect_art(art, &entry.boot, profile, &mut warnings);
        library.apps.push(App {
            folder: folder.name.clone(),
            title: entry.title,
            boot: entry.boot,
            boot_found,
            art,
            warnings,
        });
    }
    Ok(())
}

/// Lists `root` (a device root, or its `usb_prefix` folder) as OPL would.
pub fn scan_library(root: &Path, options: &ScanOptions) -> Result<Library, ScanError> {
    scan_library_with(root, options, &mut |_, _| true)
}

/// [`scan_library`] with progress: `progress(done, total)` is called for
/// every image and `ul.cfg` entry and stops the scan by returning false.
pub fn scan_library_with(
    root: &Path,
    options: &ScanOptions,
    progress: &mut dyn FnMut(u64, u64) -> bool,
) -> Result<Library, ScanError> {
    if !root.is_dir() {
        return Err(ScanError::NotADirectory(root.to_path_buf()));
    }
    let top = Listing::read(root).map_err(io_err(root))?;
    let mut total = 0u64;
    for media in [Media::Cd, Media::Dvd] {
        if let Some(dir) = top.find_dir(media.folder()) {
            total += Listing::read(&dir.path).map_or(0, |l| l.entries.len() as u64);
        }
    }
    if let Some(ul) = top.find_file("ul.cfg") {
        total += ul.len / ul_cfg::RECORD_SIZE as u64;
    }
    let mut scanner = Scanner {
        progress,
        done: 0,
        total,
        options,
        library: Library {
            root: root.to_path_buf(),
            folders: OPL_FOLDERS
                .iter()
                .map(|f| ((*f).to_owned(), top.find_dir(f).is_some()))
                .collect(),
            games: Vec::new(),
            apps: Vec::new(),
            skipped: Vec::new(),
            warnings: Vec::new(),
        },
    };

    for media in [Media::Cd, Media::Dvd] {
        if let Some(dir) = top.find_dir(media.folder()) {
            scanner.scan_image_folder(dir, media)?;
        }
    }
    if let Some(ul) = top.find_file("ul.cfg") {
        scanner.scan_ul(&top, ul)?;
    }
    let mut library = scanner.library;

    let cfgs = listing(top.find_dir("CFG"))?;
    let chts = listing(top.find_dir("CHT"))?;
    let vmcs = listing(top.find_dir("VMC"))?;
    let art_dir = listing(top.find_dir(art::ART_FOLDER))?;
    let art = index_art(&art_dir);
    let profile = options.profile;

    let mut seen: HashMap<String, usize> = HashMap::new();
    for game in &library.games {
        *seen.entry(game.startup.clone()).or_default() += 1;
    }
    for game in &mut library.games {
        if seen.get(&game.startup).is_some_and(|&n| n > 1) {
            game.warnings
                .push(Warning::DuplicateStartup(game.startup.clone()));
        }
        if let Some(cfg) = cfgs.find_file(&format!("{}.cfg", game.startup)) {
            game.cfg = Some(read_cfg(&cfg.path, profile, &vmcs, &mut game.warnings));
        }
        game.art = collect_art(&art, &game.startup, profile, &mut game.warnings);
        if let Some(cht) = chts.find_file(&format!("{}.cht", game.startup)) {
            game.cheats = Some(read_cheats(&cht.path, &mut game.warnings));
        }
    }
    scan_apps(top.find_dir(apps::APPS_FOLDER), &art, profile, &mut library)?;
    let journals = Listing::read(&root.join(crate::journal::JOURNAL_DIR)).unwrap_or_default();
    for entry in journals.files().filter(|e| e.name.ends_with(".json")) {
        let id = entry.name.trim_end_matches(".json").to_owned();
        library.warnings.push(Warning::UnfinishedOperation(id));
    }

    library.games.sort_by(|a, b| {
        (a.title.to_lowercase(), &a.startup, a.media).cmp(&(
            b.title.to_lowercase(),
            &b.startup,
            b.media,
        ))
    });
    library.apps.sort_by_key(|a| a.title.to_lowercase());
    Ok(library)
}
