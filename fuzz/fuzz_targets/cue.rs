//! CUE sheets and raw BIN sectors: the parser never panics, positions stay
//! on a CD, a syntax error names a line the sheet has.
#![no_main]

use libfuzzer_sys::fuzz_target;
use yaoplm_formats::cue::{self, CueError, CueSheet, MAX_FRAMES, TrackMode};

/// Lines as the parser counts them: CR LF, a lone CR and a lone LF each
/// end one.
fn lines(data: &[u8]) -> usize {
    let lone_cr = data
        .iter()
        .enumerate()
        .filter(|&(i, &b)| b == b'\r' && data.get(i + 1) != Some(&b'\n'))
        .count();
    1 + lone_cr + data.iter().filter(|&&b| b == b'\n').count()
}

fuzz_target!(|data: &[u8]| {
    match CueSheet::parse(data) {
        Ok(sheet) => {
            let indexes = sheet
                .files
                .iter()
                .flat_map(|f| &f.tracks)
                .flat_map(|t| &t.indexes);
            for &(_, frames) in indexes {
                assert!(frames < MAX_FRAMES);
            }
            if let Ok(track) = sheet.data_track() {
                assert!(track.start_sector < u64::from(MAX_FRAMES));
                if let Some(sectors) = track.sectors {
                    assert!(sectors > 0 && track.start_sector + sectors < u64::from(MAX_FRAMES));
                }
            }
        }
        Err(CueError::Syntax { line, .. }) => assert!((1..=lines(data)).contains(&line)),
        Err(_) => {}
    }
    let _ = cue::probe_raw(data);
    let mut sector = [0u8; 2048];
    for mode in [
        TrackMode::Mode1_2048,
        TrackMode::Mode1_2352,
        TrackMode::Mode2_2336,
        TrackMode::Mode2_2352,
    ] {
        let _ = cue::user_data(mode, data, 0, &mut sector);
    }
});
