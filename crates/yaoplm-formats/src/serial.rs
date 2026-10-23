//! Game ID spellings: OPL's `SLUS_200.62` and the serial `SLUS-20062` used
//! by Redump, PCSX2's GameIndex and cover repositories (research/03 §1,
//! META-1).
//!
//! OPL takes the ID from `SYSTEM.CNF` or from an old-style file name
//! (`src/supportbase.c`, ps2homebrew/Open-PS2-Loader@b3a21997) and keeps it
//! as is: `XXXX_YYY.ZZ`. Databases spell the same disc `XXXX-YYYZZ`. Both
//! directions here are total functions: an ID that does not fit the
//! pattern (`DATA\MAIN.E`, `SLUS21568`, `SLES-54793/ANZ`) is only
//! normalised in the obvious ways (case, `_` for `-`, dots dropped) so that
//! a lookup still has a chance.

/// `SLUS_200.62` → `SLUS-20062`.
///
/// Anything that is not a four-letter prefix, three digits, a dot and two
/// digits is upper-cased, its underscore turned into a hyphen and the dot
/// removed.
pub fn opl_to_serial(id: &str) -> String {
    let id = id.trim();
    let bytes = id.as_bytes();
    if bytes.len() == 11
        && bytes[..4].iter().all(u8::is_ascii_alphabetic)
        && bytes[4] == b'_'
        && bytes[5..8].iter().all(u8::is_ascii_digit)
        && bytes[8] == b'.'
        && bytes[9..].iter().all(u8::is_ascii_digit)
    {
        return format!("{}-{}{}", id[..4].to_ascii_uppercase(), &id[5..8], &id[9..]);
    }
    let mut out = String::with_capacity(id.len());
    for c in id.chars() {
        match c {
            '_' => out.push('-'),
            '.' => {}
            c => out.push(c.to_ascii_uppercase()),
        }
    }
    out
}

/// `SLUS-20062` → `SLUS_200.62`.
///
/// Serials with suffixes (`SLES-54793/ANZ`, `SLPM-65002-0`) or without a
/// separator (`SLUS21568`) are returned upper-cased and unchanged
/// otherwise: OPL would never read such an ID from a disc.
pub fn serial_to_opl(serial: &str) -> String {
    let serial = serial.trim();
    let bytes = serial.as_bytes();
    if bytes.len() == 10
        && bytes[..4].iter().all(u8::is_ascii_alphabetic)
        && bytes[4] == b'-'
        && bytes[5..].iter().all(u8::is_ascii_digit)
    {
        return format!(
            "{}_{}.{}",
            serial[..4].to_ascii_uppercase(),
            &serial[5..8],
            &serial[8..]
        );
    }
    serial.to_ascii_uppercase()
}

/// The canonical lookup key: a serial, upper case, with any region or
/// revision suffix after the five digits removed (`SLES-54793/ANZ` →
/// `SLES-54793`, `SLPM-65002-0` → `SLPM-65002`). Accepts either spelling.
pub fn lookup_key(id: &str) -> String {
    let serial = opl_to_serial(id);
    let bytes = serial.as_bytes();
    if bytes.len() > 10
        && bytes[..4].iter().all(u8::is_ascii_alphabetic)
        && bytes[4] == b'-'
        && bytes[5..10].iter().all(u8::is_ascii_digit)
    {
        return serial[..10].to_owned();
    }
    serial
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_directions() {
        assert_eq!(opl_to_serial("SLUS_200.62"), "SLUS-20062");
        assert_eq!(opl_to_serial("slus_200.62"), "SLUS-20062");
        assert_eq!(opl_to_serial(" SCES_500.03 "), "SCES-50003");
        assert_eq!(serial_to_opl("SLUS-20062"), "SLUS_200.62");
        assert_eq!(serial_to_opl("slus-20062"), "SLUS_200.62");
        for id in ["SLUS_200.62", "PBPX_955.02", "SCPS_150.00"] {
            assert_eq!(serial_to_opl(&opl_to_serial(id)), id);
        }
    }

    #[test]
    fn odd_ids() {
        assert_eq!(opl_to_serial("SLUS21568"), "SLUS21568");
        assert_eq!(opl_to_serial("DATA\\MAIN.E"), "DATA\\MAINE");
        assert_eq!(opl_to_serial("ALCH_000.4"), "ALCH-0004");
        assert_eq!(serial_to_opl("SLES-54793/ANZ"), "SLES-54793/ANZ");
        assert_eq!(serial_to_opl("SLUS21568"), "SLUS21568");
        assert_eq!(serial_to_opl("KRT-005"), "KRT-005");
    }

    #[test]
    fn keys_drop_suffixes() {
        assert_eq!(lookup_key("SLES-54793/ANZ"), "SLES-54793");
        assert_eq!(lookup_key("SLPM-65002-0"), "SLPM-65002");
        assert_eq!(lookup_key("SLUS_200.62"), "SLUS-20062");
        assert_eq!(lookup_key("SLUS-21782B"), "SLUS-21782");
        assert_eq!(lookup_key("KRT-005"), "KRT-005");
        assert_eq!(lookup_key("SLUS21568"), "SLUS21568");
    }
}
