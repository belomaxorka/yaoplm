//! BIN/CUE fixtures made from an ISO: the raw sectors a CD dump would
//! hold, with sync, header (MSF address, mode) and Form 1 subheader. EDC
//! and ECC bytes are left zero; OPL never sees them and YAOPLM drops them.

/// Raw sector layouts, as named in CUE sheets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawMode {
    Mode1_2048,
    Mode1_2352,
    Mode2_2336,
    Mode2_2352,
}

impl RawMode {
    pub const ALL: [Self; 4] = [
        Self::Mode1_2048,
        Self::Mode1_2352,
        Self::Mode2_2336,
        Self::Mode2_2352,
    ];

    pub fn cue_name(self) -> &'static str {
        match self {
            Self::Mode1_2048 => "MODE1/2048",
            Self::Mode1_2352 => "MODE1/2352",
            Self::Mode2_2336 => "MODE2/2336",
            Self::Mode2_2352 => "MODE2/2352",
        }
    }

    pub fn sector_size(self) -> usize {
        match self {
            Self::Mode1_2048 => 2048,
            Self::Mode2_2336 => 2336,
            Self::Mode1_2352 | Self::Mode2_2352 => 2352,
        }
    }
}

fn bcd(value: u64) -> u8 {
    (((value / 10) << 4) | (value % 10)) as u8
}

/// One raw sector at disc address `lba` (2 seconds of lead-in added).
pub fn raw_sector(mode: RawMode, lba: u64, data: &[u8]) -> Vec<u8> {
    let mut raw = vec![0u8; mode.sector_size()];
    let frames = lba + 150;
    let header = [
        bcd(frames / 75 / 60),
        bcd(frames / 75 % 60),
        bcd(frames % 75),
    ];
    let at = match mode {
        RawMode::Mode1_2048 => 0,
        RawMode::Mode2_2336 => {
            // Subheader twice: file 0, channel 0, submode "data", coding 0.
            raw[2] = 0x08;
            raw[6] = 0x08;
            8
        }
        RawMode::Mode1_2352 | RawMode::Mode2_2352 => {
            raw[0] = 0x00;
            raw[1..11].fill(0xFF);
            raw[11] = 0x00;
            raw[12..15].copy_from_slice(&header);
            if mode == RawMode::Mode1_2352 {
                raw[15] = 1;
                16
            } else {
                raw[15] = 2;
                raw[18] = 0x08;
                raw[22] = 0x08;
                24
            }
        }
    };
    raw[at..at + 2048].copy_from_slice(&data[..2048]);
    raw
}

/// The BIN for `iso` (whole 2048-byte sectors), with `pregap` empty
/// sectors in front.
pub fn to_bin(iso: &[u8], mode: RawMode, pregap: u64) -> Vec<u8> {
    let zero = [0u8; 2048];
    let mut out = Vec::with_capacity((iso.len() / 2048 + pregap as usize) * mode.sector_size());
    for lba in 0..pregap {
        out.extend(raw_sector(mode, lba, &zero));
    }
    for (i, sector) in iso.as_chunks::<2048>().0.iter().enumerate() {
        out.extend(raw_sector(mode, pregap + i as u64, sector));
    }
    out
}

/// A single-track sheet for `bin_name`, starting at `INDEX 01 = pregap`.
pub fn cue_sheet(bin_name: &str, mode: RawMode, pregap: u64) -> String {
    let index1 = format!(
        "{:02}:{:02}:{:02}",
        pregap / 75 / 60,
        pregap / 75 % 60,
        pregap % 75
    );
    let index0 = if pregap > 0 {
        "    INDEX 00 00:00:00\r\n"
    } else {
        ""
    };
    format!(
        "FILE \"{bin_name}\" BINARY\r\n  TRACK 01 {}\r\n{index0}    INDEX 01 {index1}\r\n",
        mode.cue_name()
    )
}
