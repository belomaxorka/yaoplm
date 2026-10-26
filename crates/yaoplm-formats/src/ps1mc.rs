//! The PS1 memory card, read only: a raw 128 KiB image, as sd2psx keeps
//! them in `MemoryCards/PS1/` (VMC-2, ADR-0048).
//!
//! Format reference: `docs/research/19-psxmemcard-gen2-audit.md` §3. The
//! layout follows ps2vmc-tool's PS1 reader (`src/ps1card.c`, GPL-3,
//! bucanero/ps2vmc-tool@32a0245, after MemcardRex), whose logic this ports:
//! - 16 blocks of 8 KiB; block 0 is the card's own: frame 0 starts with
//!   `MC`, frames 1–15 describe blocks 1–15, 128 bytes each;
//! - a frame: its state (`0xA0` free, `0x51` a save's first block, `0x52`
//!   a middle one, `0x53` the last; `0xA1`–`0xA3` the same, deleted), the
//!   save's size in bytes (first block), the next block's frame index
//!   (`0xFFFF` for none), the save's name (`BA`/`BE`/`BI` region, product
//!   code, identifier; 20 ASCII bytes) and a checksum: the XOR of the
//!   frame's other bytes;
//! - a save's first block starts with `SC` and its title: 64 bytes of
//!   Shift-JIS at offset 4.
//!
//! Exploits keep odd frames on purpose: FreePSXBoot's has a size no save
//! has (research/19 §3). The reader reports what is there and never fixes
//! it; a chain that loops or leaves the card ends where it breaks.

use crate::icon_sys::decode_title;

pub const CARD_SIZE: usize = 128 * 1024;
const FRAME: usize = 128;
const BLOCK: usize = 8 * 1024;
pub const SLOTS: usize = 15;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Ps1Error {
    #[error("a PS1 memory card image is 131072 bytes, not {0}")]
    Size(usize),
    #[error("not a PS1 memory card image (no `MC` header)")]
    NotACard,
}

/// What a directory frame says about its block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotState {
    Free,
    First,
    Middle,
    Last,
    DeletedFirst,
    DeletedMiddle,
    DeletedLast,
    /// No state a card uses: the raw byte.
    Corrupt(u8),
}

impl SlotState {
    fn of(byte: u8) -> Self {
        match byte {
            0xA0 => Self::Free,
            0x51 => Self::First,
            0x52 => Self::Middle,
            0x53 => Self::Last,
            0xA1 => Self::DeletedFirst,
            0xA2 => Self::DeletedMiddle,
            0xA3 => Self::DeletedLast,
            other => Self::Corrupt(other),
        }
    }
}

/// One of the 15 directory frames.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slot {
    /// The block it describes, 1–15.
    pub block: u8,
    pub state: SlotState,
    pub size: u32,
    /// The next block's frame index, 0–14, when it names one.
    pub next: Option<u16>,
    pub name: String,
    pub checksum_ok: bool,
}

/// A save: a first block and the blocks its chain links.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Save {
    /// Blocks in chain order, 1–15.
    pub blocks: Vec<u8>,
    /// `BASLUS-01234…`: region, product code, identifier.
    pub name: String,
    pub size: u32,
    /// From the first block, decoded; empty without the `SC` header.
    pub title: String,
}

impl Save {
    /// `BA` (America), `BE` (Europe), `BI` (Japan).
    pub fn region(&self) -> &str {
        self.name.get(..2).unwrap_or("")
    }

    /// `SLUS-01234`.
    pub fn product(&self) -> &str {
        self.name.get(2..12).unwrap_or("")
    }
}

/// A parsed card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ps1Card {
    pub slots: Vec<Slot>,
    pub saves: Vec<Save>,
}

fn ascii(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

impl Ps1Card {
    pub fn parse(bytes: &[u8]) -> Result<Self, Ps1Error> {
        if bytes.len() != CARD_SIZE {
            return Err(Ps1Error::Size(bytes.len()));
        }
        if !bytes.starts_with(b"MC") {
            return Err(Ps1Error::NotACard);
        }
        let slots: Vec<Slot> = (0..SLOTS)
            .map(|i| {
                let frame = &bytes[FRAME * (i + 1)..FRAME * (i + 2)];
                let next = u16::from_le_bytes([frame[8], frame[9]]);
                let xor = frame[..FRAME - 1].iter().fold(0u8, |acc, b| acc ^ b);
                Slot {
                    block: (i + 1) as u8,
                    state: SlotState::of(frame[0]),
                    size: u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]),
                    next: (usize::from(next) < SLOTS).then_some(next),
                    name: ascii(&frame[10..30]),
                    checksum_ok: xor == frame[FRAME - 1],
                }
            })
            .collect();
        let saves = slots
            .iter()
            .enumerate()
            .filter(|(_, slot)| slot.state == SlotState::First)
            .map(|(i, slot)| {
                let mut blocks = vec![slot.block];
                let mut current = i;
                while let Some(next) = slots[current].next {
                    let next = usize::from(next);
                    let state = slots[next].state;
                    if blocks.contains(&slots[next].block)
                        || !matches!(state, SlotState::Middle | SlotState::Last)
                    {
                        break;
                    }
                    blocks.push(slots[next].block);
                    if state == SlotState::Last {
                        break;
                    }
                    current = next;
                }
                let data = &bytes[BLOCK * usize::from(slot.block)..][..BLOCK];
                let title = if data.starts_with(b"SC") {
                    let raw = &data[4..68];
                    // Two-byte characters: the title ends at a NUL on an
                    // even offset.
                    let end = (0..raw.len())
                        .step_by(2)
                        .find(|&j| raw[j] == 0)
                        .unwrap_or(raw.len());
                    decode_title(&raw[..end])
                } else {
                    String::new()
                };
                Save {
                    blocks,
                    name: slot.name.clone(),
                    size: slot.size,
                    title,
                }
            })
            .collect();
        Ok(Self { slots, saves })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card() -> Vec<u8> {
        let mut bytes = vec![0u8; CARD_SIZE];
        bytes[..2].copy_from_slice(b"MC");
        for i in 0..SLOTS {
            set_frame(&mut bytes, i, 0xA0, 0, 0xFFFF, "");
        }
        bytes
    }

    fn set_frame(bytes: &mut [u8], index: usize, state: u8, size: u32, next: u16, name: &str) {
        let frame = &mut bytes[FRAME * (index + 1)..FRAME * (index + 2)];
        frame.fill(0);
        frame[0] = state;
        frame[4..8].copy_from_slice(&size.to_le_bytes());
        frame[8..10].copy_from_slice(&next.to_le_bytes());
        frame[10..10 + name.len()].copy_from_slice(name.as_bytes());
        frame[FRAME - 1] = frame[..FRAME - 1].iter().fold(0, |a, b| a ^ b);
    }

    #[test]
    fn a_two_block_save_with_its_title() {
        let mut bytes = card();
        set_frame(&mut bytes, 0, 0x51, 16384, 1, "BASLUS-00067SAVE01");
        set_frame(&mut bytes, 1, 0x53, 0, 0xFFFF, "");
        let title = encoding_rs::SHIFT_JIS
            .encode("ＦＩＮＡＬ　ＦＡＮＴＡＳＹ")
            .0
            .into_owned();
        let block = BLOCK;
        bytes[block..block + 2].copy_from_slice(b"SC");
        bytes[block + 4..block + 4 + title.len()].copy_from_slice(&title);
        let parsed = Ps1Card::parse(&bytes).unwrap();
        assert_eq!(parsed.saves.len(), 1);
        let save = &parsed.saves[0];
        assert_eq!(save.blocks, [1, 2]);
        assert_eq!((save.region(), save.product()), ("BA", "SLUS-00067"));
        assert_eq!(save.title, "FINAL FANTASY");
        assert!(parsed.slots.iter().all(|s| s.checksum_ok));
        assert_eq!(parsed.slots[2].state, SlotState::Free);
    }

    #[test]
    fn odd_frames_are_reported_not_fixed() {
        let mut bytes = card();
        // FreePSXBoot-style: a size no save has, a loop back to itself.
        set_frame(
            &mut bytes,
            0,
            0x51,
            0xFFFF_FFF0,
            0,
            "BESCES-00000FREEPSXBOOT",
        );
        set_frame(&mut bytes, 1, 0x52, 0, 1, "");
        set_frame(&mut bytes, 2, 0x51, 8192, 1, "BASLUS-11111A");
        bytes[FRAME * 4] = 0x77;
        let parsed = Ps1Card::parse(&bytes).unwrap();
        assert_eq!(parsed.saves[0].blocks, [1]);
        assert_eq!(parsed.saves[0].size, 0xFFFF_FFF0);
        // A middle block that names itself ends the chain.
        assert_eq!(parsed.saves[1].blocks, [3, 2]);
        assert_eq!(parsed.slots[3].state, SlotState::Corrupt(0x77));
        assert!(!parsed.slots[3].checksum_ok);
    }

    #[test]
    fn not_a_card() {
        assert_eq!(Ps1Card::parse(&[0; 100]), Err(Ps1Error::Size(100)));
        assert_eq!(Ps1Card::parse(&vec![0; CARD_SIZE]), Err(Ps1Error::NotACard));
    }
}
