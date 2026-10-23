//! Containers images arrive in (IMP-1b), made from generated ISOs: CHD as
//! chdman lays it out (DVD units, or CD frames with track metadata), zip
//! and 7z archives.

use std::ffi::CString;
use std::io::{self, Cursor, Write};

use chd::metadata::KnownMetadata;
use chd::write::{ChdWriteOptions, ChdWriter};

use crate::bincue::{RawMode, raw_sector};

const FRAME: usize = 2448;

fn other(err: impl std::fmt::Display) -> io::Error {
    io::Error::other(err.to_string())
}

/// A DVD CHD of `iso`.
pub fn chd_dvd(iso: &[u8]) -> io::Result<Vec<u8>> {
    let mut writer =
        ChdWriter::new(Cursor::new(Vec::new()), ChdWriteOptions::dvd()).map_err(other)?;
    writer.write_all(iso)?;
    Ok(writer.finish().map_err(other)?.into_inner())
}

fn chd_type(mode: RawMode) -> &'static str {
    match mode {
        RawMode::Mode1_2048 => "MODE1",
        RawMode::Mode1_2352 => "MODE1_RAW",
        RawMode::Mode2_2336 => "MODE2",
        RawMode::Mode2_2352 => "MODE2_RAW",
    }
}

/// Zero frames up to the next multiple of four, as chdman pads tracks.
fn pad_track(frames: &mut Vec<u8>, count: usize) {
    for _ in count..count.div_ceil(4) * 4 {
        frames.extend_from_slice(&[0u8; FRAME]);
    }
}

/// A CD CHD: `iso` as track 1 in `mode`, then a silent audio track of
/// `audio_frames` frames when that is not zero.
pub fn chd_cd(iso: &[u8], mode: RawMode, audio_frames: usize) -> io::Result<Vec<u8>> {
    let sectors = iso.len() / 2048;
    let mut frames = Vec::with_capacity((sectors + audio_frames + 8) * FRAME);
    for (lba, sector) in iso.as_chunks::<2048>().0.iter().enumerate() {
        let raw = raw_sector(mode, lba as u64, sector);
        let mut frame = [0u8; FRAME];
        frame[..raw.len()].copy_from_slice(&raw);
        frames.extend_from_slice(&frame);
    }
    pad_track(&mut frames, sectors);
    let mut tracks = vec![format!(
        "TRACK:1 TYPE:{} SUBTYPE:NONE FRAMES:{sectors} PREGAP:0 PGTYPE:MODE1 PGSUB:RW POSTGAP:0",
        chd_type(mode)
    )];
    if audio_frames > 0 {
        frames.resize(frames.len() + audio_frames * FRAME, 0);
        pad_track(&mut frames, audio_frames);
        tracks.push(format!(
            "TRACK:2 TYPE:AUDIO SUBTYPE:NONE FRAMES:{audio_frames} PREGAP:150 PGTYPE:AUDIO PGSUB:RW POSTGAP:0"
        ));
    }
    let mut writer =
        ChdWriter::new(Cursor::new(Vec::new()), ChdWriteOptions::cd()).map_err(other)?;
    for track in tracks {
        let text = CString::new(track).map_err(other)?;
        writer
            .add_metadata(KnownMetadata::CdRomTrack2, text.as_c_str(), true)
            .map_err(other)?;
    }
    writer.write_all(&frames)?;
    Ok(writer.finish().map_err(other)?.into_inner())
}

/// A zip with `files`; `stored` keeps them uncompressed.
pub fn zip(files: &[(&str, &[u8])], stored: bool) -> io::Result<Vec<u8>> {
    let method = if stored {
        zip::CompressionMethod::Stored
    } else {
        zip::CompressionMethod::Deflated
    };
    let mut out = Cursor::new(Vec::new());
    let mut writer = zip::ZipWriter::new(&mut out);
    for (name, bytes) in files {
        writer
            .start_file(
                *name,
                zip::write::SimpleFileOptions::default()
                    .compression_method(method)
                    .large_file(bytes.len() as u64 >= u64::from(u32::MAX)),
            )
            .map_err(other)?;
        writer.write_all(bytes)?;
    }
    writer.finish().map_err(other)?;
    Ok(out.into_inner())
}

/// A solid 7z with `files` (LZMA2).
pub fn seven_zip(files: &[(&str, &[u8])]) -> io::Result<Vec<u8>> {
    let mut out = Cursor::new(Vec::new());
    {
        let mut writer = sevenz_rust2::ArchiveWriter::new(&mut out).map_err(other)?;
        let entries: Vec<sevenz_rust2::ArchiveEntry> = files
            .iter()
            .map(|(name, _)| sevenz_rust2::ArchiveEntry::new_file(name))
            .collect();
        let readers: Vec<sevenz_rust2::SourceReader<Cursor<Vec<u8>>>> = files
            .iter()
            .map(|(_, bytes)| Cursor::new(bytes.to_vec()).into())
            .collect();
        writer
            .push_archive_entries(entries, readers)
            .map_err(other)?;
        writer.finish().map_err(other)?;
    }
    Ok(out.into_inner())
}
