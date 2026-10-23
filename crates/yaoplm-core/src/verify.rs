//! Hashing games on the device for dump verification (GAME-5).
//!
//! The bytes hashed are the ISO OPL reads: the file of an ISO, the
//! decompressed sectors of a ZSO (the container is chosen by the `ZISO`
//! magic, like `ProbeZISO()` in OPL), the `ul` chunks joined in order.
//! Redump publishes PS2 DVDs as one `.iso`, so these hashes compare with
//! its DAT directly; CD games are BIN/CUE there and an ISO of one cannot
//! match (`yaoplm_hash::redump`).

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::PathBuf;

use yaoplm_formats::image::DiscImage;
use yaoplm_formats::zso::{self, ZsoError};
use yaoplm_hash::{Algorithms, HashError, Hashes, hash_reader};

use crate::model::{Game, GameLocation};
use crate::multipart::MultiPartFile;
use crate::source::sector_stream;

/// The files an image is read from, in order, with their sizes: the cache
/// key of its hashes.
pub fn image_files(game: &Game) -> Result<Vec<(PathBuf, u64)>, io::Error> {
    match &game.location {
        GameLocation::Image { path, .. } => {
            let len = std::fs::metadata(path)?.len();
            Ok(vec![(path.clone(), len)])
        }
        GameLocation::Ul { parts, .. } => parts
            .iter()
            .map(|part| match (&part.path, part.size) {
                (Some(path), Some(size)) => Ok((path.clone(), size)),
                _ => Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("chunk {} is missing", part.name),
                )),
            })
            .collect(),
    }
}

/// The ISO bytes of `game` and their length.
pub fn image_reader(game: &Game) -> Result<(Box<dyn Read + Send>, u64), io::Error> {
    let files = image_files(game)?;
    if let GameLocation::Ul { .. } = game.location {
        let len = files.iter().map(|(_, len)| len).sum();
        return Ok((Box::new(MultiPartFile::new(files)), len));
    }
    let (path, len) = files
        .into_iter()
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "the game has no image file"))?;
    let mut file = File::open(&path)?;
    let mut magic = [0u8; 4];
    let is_zso = file.read_exact(&mut magic).is_ok() && magic == zso::MAGIC;
    file.seek(SeekFrom::Start(0))?;
    if !is_zso {
        return Ok((Box::new(file), len));
    }
    let image = DiscImage::open(file).map_err(|err| match err {
        ZsoError::Io(err) => err,
        other => io::Error::new(io::ErrorKind::InvalidData, other.to_string()),
    })?;
    let iso_len = yaoplm_formats::iso9660::SectorSource::sector_count(&image)
        * yaoplm_formats::iso9660::SECTOR_SIZE as u64;
    Ok((sector_stream(image, 0)?, iso_len))
}

/// Hashes the ISO bytes of `game`. `progress` gets `(done, total)`.
pub fn hash_game(
    game: &Game,
    algorithms: Algorithms,
    stop: &dyn Fn() -> bool,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<Hashes, HashError> {
    let (mut reader, total) = image_reader(game)?;
    hash_reader(&mut reader, algorithms, stop, &mut |done| {
        progress(done, total)
    })
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use yaoplm_formats::Media;
    use yaoplm_formats::iso_name::NameFormat;

    use super::*;
    use crate::model::{GameFormat, UlPart};

    fn game(location: GameLocation, format: GameFormat) -> Game {
        Game {
            title: "Game".into(),
            startup: "SLUS_200.62".into(),
            media: Media::Dvd,
            format,
            location,
            size: 0,
            image_startup: None,
            layers: None,
            cfg: None,
            art: Vec::new(),
            cheats: None,
            warnings: Vec::new(),
        }
    }

    fn iso() -> Vec<u8> {
        (0..6 * 2048).map(|i| (i % 253) as u8).collect()
    }

    fn hashes_of(bytes: &[u8]) -> Hashes {
        hash_reader(
            &mut Cursor::new(bytes),
            Algorithms::default(),
            &|| false,
            &mut |_| {},
        )
        .unwrap()
    }

    #[test]
    fn iso_zso_and_ul_hash_the_same_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let iso = iso();
        let expected = hashes_of(&iso);

        let iso_path = dir.path().join("SLUS_200.62.Game.iso");
        std::fs::write(&iso_path, &iso).unwrap();
        let image = |path: PathBuf, format| {
            game(
                GameLocation::Image {
                    file_name: path.file_name().unwrap().to_string_lossy().into_owned(),
                    path,
                    name_format: NameFormat::Old,
                },
                format,
            )
        };
        let mut seen = Vec::new();
        let hashes = hash_game(
            &image(iso_path, GameFormat::Iso),
            Algorithms::default(),
            &|| false,
            &mut |done, total| seen.push((done, total)),
        )
        .unwrap();
        assert_eq!(hashes, expected);
        assert_eq!(seen.last(), Some(&(iso.len() as u64, iso.len() as u64)));

        let mut zso = Cursor::new(Vec::new());
        zso::compress(
            &mut Cursor::new(&iso),
            iso.len() as u64,
            &mut zso,
            &mut |_| {},
        )
        .unwrap();
        let zso_path = dir.path().join("SLUS_200.62.Game.zso");
        std::fs::write(&zso_path, zso.into_inner()).unwrap();
        let hashes = hash_game(
            &image(zso_path, GameFormat::Zso),
            Algorithms::default(),
            &|| false,
            &mut |_, _| {},
        )
        .unwrap();
        assert_eq!(hashes, expected);

        let mut parts = Vec::new();
        for (n, chunk) in iso.chunks(4 * 2048).enumerate() {
            let path = dir.path().join(format!("ul.ABCD.SLUS_200.62.0{n}"));
            std::fs::write(&path, chunk).unwrap();
            parts.push(UlPart {
                name: path.file_name().unwrap().to_string_lossy().into_owned(),
                path: Some(path),
                size: Some(chunk.len() as u64),
            });
        }
        let ul = game(
            GameLocation::Ul {
                index: 0,
                parts: parts.clone(),
            },
            GameFormat::Ul,
        );
        let hashes = hash_game(&ul, Algorithms::default(), &|| false, &mut |_, _| {}).unwrap();
        assert_eq!(hashes, expected);
        assert_eq!(image_files(&ul).unwrap().len(), 2);

        parts[1].path = None;
        let broken = game(GameLocation::Ul { index: 0, parts }, GameFormat::Ul);
        let err = hash_game(&broken, Algorithms::default(), &|| false, &mut |_, _| {}).unwrap_err();
        assert!(err.to_string().contains("missing"), "{err}");
    }
}
