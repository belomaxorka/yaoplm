//! Details of one disc image: what OPL would make of it, and what it is.

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

use yaoplm_formats::image::{self, Container, DiscImage};
use yaoplm_formats::iso_name::{self, IsoFileName};
use yaoplm_formats::iso9660::{self, Layers, PVD_LBA, SectorSource};
use yaoplm_formats::system_cnf::{self, CnfInfo};
use yaoplm_formats::zso::{ZsoError, ZsoHeader, ZsoProblem};
use yaoplm_formats::{LoaderProfile, Media};

use crate::model::{GameFormat, Warning};
use crate::scan::CD_MAX_BYTES;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Ps2,
    /// `SYSTEM.CNF` with `BOOT` only. OPL hides these; they go to POPStarter.
    Ps1,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageInfo {
    pub path: PathBuf,
    pub file_size: u64,
    pub format: GameFormat,
    pub zso: Option<ZsoHeader>,
    /// Uncompressed sectors.
    pub sectors: u64,
    pub volume_id: Option<String>,
    pub volume_sectors: Option<u32>,
    pub layers: Option<Layers>,
    /// The first 1023 bytes of `SYSTEM.CNF;1`, as text.
    pub system_cnf: Option<String>,
    pub cnf: CnfInfo,
    /// The ID OPL derives, or why it would hide the image.
    pub startup: Result<String, String>,
    pub platform: Platform,
    /// IMP-3 heuristic: CD when the volume fits on an 80-minute CD.
    pub suggested_media: Media,
    /// How OPL would read the file name, if it is an image name at all.
    pub file_name: Option<IsoFileName>,
    pub warnings: Vec<Warning>,
}

#[derive(Debug, thiserror::Error)]
pub enum InfoError {
    #[error("{path}: {source}")]
    Io { path: PathBuf, source: io::Error },
    #[error("{path}: {source}")]
    Zso { path: PathBuf, source: ZsoError },
}

pub fn image_info(path: &Path, profile: LoaderProfile) -> Result<ImageInfo, InfoError> {
    let io_err = |source| InfoError::Io {
        path: path.to_path_buf(),
        source,
    };
    let file = File::open(path).map_err(io_err)?;
    let file_size = file.metadata().map_err(io_err)?.len();
    let mut image = DiscImage::open(file).map_err(|source| match source {
        ZsoError::Io(source) => io_err(source),
        source => InfoError::Zso {
            path: path.to_path_buf(),
            source,
        },
    })?;
    let mut warnings = Vec::new();
    let format = match image.container() {
        Container::Iso => GameFormat::Iso,
        Container::Zso => GameFormat::Zso,
    };
    let zso = image.zso_header().copied();
    let problems: Vec<ZsoProblem> = zso.map(|h| h.problems()).unwrap_or_default();
    warnings.extend(problems.iter().map(|p| Warning::ZsoHeader(p.to_string())));
    if format == GameFormat::Zso && !profile.reads_zso() {
        warnings.push(Warning::ZsoUnsupportedByProfile);
    }

    let pvd = iso9660::read_pvd(&mut image, PVD_LBA).ok();
    let layers = pvd.as_ref().map(|pvd| iso9660::opl_layers(&mut image, pvd));
    let cnf_file = match &pvd {
        Some(pvd) => image::read_system_cnf(&mut image, pvd).ok().flatten(),
        None => None,
    };
    let cnf_bytes = cnf_file.as_ref().map(|f| f.bytes.clone());
    let cnf = cnf_bytes
        .as_deref()
        .map(system_cnf::parse_info)
        .unwrap_or_default();
    let startup = image::opl_startup(&mut image).map_err(|e| e.to_string());
    let platform = match (&cnf.boot2, &cnf.boot) {
        (Some(_), _) => Platform::Ps2,
        (None, Some(_)) => Platform::Ps1,
        (None, None) => Platform::Unknown,
    };

    let volume_sectors = pvd.as_ref().map(|p| p.volume_space_size);
    let volume_bytes = volume_sectors.map_or(image.sector_count(), u64::from) * 2048;
    let suggested_media =
        if matches!(layers, Some(Layers::Dual { .. })) || volume_bytes > CD_MAX_BYTES {
            Media::Dvd
        } else {
            Media::Cd
        };

    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| iso_name::classify(n).ok());
    if let (Some(name), Ok(image_id)) = (&file_name, &startup)
        && let Some(name_id) = &name.startup
        && name_id != image_id
    {
        warnings.push(Warning::NameIdMismatch {
            name: name_id.clone(),
            image: image_id.clone(),
        });
    }

    Ok(ImageInfo {
        path: path.to_path_buf(),
        file_size,
        format,
        zso,
        sectors: image.sector_count(),
        volume_id: pvd.as_ref().map(|p| p.volume_id.clone()),
        volume_sectors,
        layers,
        system_cnf: cnf_bytes.map(|b| String::from_utf8_lossy(&b).into_owned()),
        cnf,
        startup,
        platform,
        suggested_media,
        file_name,
        warnings,
    })
}
