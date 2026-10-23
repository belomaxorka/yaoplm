//! Why something cannot be used, as phrases (ADR-0029, docs/I18N.md): the
//! errors of the format readers that users meet are translated, while
//! purely technical ones (a parser's position, an OS message) are passed
//! through as text inside a translated sentence.

use std::io;

use yaoplm_device::PathError;
use yaoplm_formats::apps::AppError;
use yaoplm_formats::cht::ChtWarning;
use yaoplm_formats::cue::{BinError, CueError};
use yaoplm_formats::image::StartupError;
use yaoplm_formats::iso_name::{self, NameError};
use yaoplm_formats::system_cnf::CnfError;
use yaoplm_formats::ul_cfg::{self, RecordAnomaly, UlError};
use yaoplm_formats::vmc::VmcProblem;
use yaoplm_formats::zso::{ZsoError, ZsoProblem};
use yaoplm_i18n::Phrase;

/// A technical message as it is, to follow a colon.
pub(crate) fn text(text: impl ToString) -> Phrase {
    Phrase::reason("text").with("text", text.to_string())
}

/// An error of the OS or a library, as a whole message.
pub(crate) fn system(err: impl ToString) -> Phrase {
    Phrase::error("system").with("error", err.to_string())
}

/// A name that cannot be used on the device, as a whole message: a file
/// name OPL would not take, or a path the device rejects.
pub(crate) fn bad_name(err: &(dyn std::error::Error + 'static)) -> Phrase {
    if let Some(err) = err.downcast_ref::<NameError>() {
        return name(err);
    }
    if let Some(err) = err.downcast_ref::<PathError>() {
        return path(err);
    }
    let inner = err
        .downcast_ref::<io::Error>()
        .and_then(io::Error::get_ref)
        .and_then(|e| e.downcast_ref::<PathError>());
    match inner {
        Some(err) => path(err),
        None => system(err),
    }
}

/// A path the device rejects, as a whole message.
pub(crate) fn path(err: &PathError) -> Phrase {
    let (PathError::BadComponent(path) | PathError::BadCharacter(path)) = err;
    Phrase::error("badPath").with("path", path)
}

/// A file name OPL would not take, as a whole message.
pub(crate) fn name(err: &NameError) -> Phrase {
    match err {
        NameError::EmptyTitle => Phrase::error("titleEmptyAfterCleanup"),
        NameError::TitleTooLong(len) => Phrase::error("titleTooLong")
            .with("length", *len)
            .with("max", iso_name::TITLE_MAX),
        NameError::UnsuitableStartup(id) => Phrase::error("startupNotForOldName").with("id", id),
        NameError::LooksLikeOldFormat(name) => Phrase::error("looksLikeOldName").with("name", name),
    }
}

/// A `ul.cfg` title or ID OPL would not take, as a whole message.
pub(crate) fn ul(err: &UlError) -> Phrase {
    match err {
        UlError::EmptyName => Phrase::error("emptyTitle"),
        UlError::NameTooLong(len) => Phrase::error("ulTitleTooLong")
            .with("length", *len)
            .with("max", ul_cfg::NAME_MAX),
        UlError::NameNotAscii => Phrase::error("ulTitleNotAscii"),
        UlError::BadStartup(id) => Phrase::error("startupNotForChunks").with("id", id),
        other => system(other),
    }
}

/// Why OPL hides a disc.
pub(crate) fn startup(err: &StartupError) -> Phrase {
    match err {
        StartupError::NoSystemCnf => Phrase::reason("noSystemCnf"),
        StartupError::Cnf(err) => cnf(err),
        StartupError::Iso(err) => Phrase::reason("isoUnreadable").with("error", err.to_string()),
    }
}

fn cnf(err: &CnfError) -> Phrase {
    Phrase::reason(match err {
        CnfError::NoBoot2 => "noBoot2",
        CnfError::NulByte => "cnfNulByte",
        CnfError::UnexpectedEof => "cnfEof",
        CnfError::MissingEquals => "cnfNoEquals",
        CnfError::MissingColon => "cnfNoColon",
        CnfError::EmptyStartup => "cnfNoFile",
        CnfError::Unpredictable => "cnfTooLong",
    })
}

pub(crate) fn vmc(problem: &VmcProblem) -> Phrase {
    match problem {
        VmcProblem::BadMagic => Phrase::reason("vmcBadMagic"),
        VmcProblem::CardType(kind) => Phrase::reason("vmcCardType").with("type", u64::from(*kind)),
        VmcProblem::SizeMismatch { file, card } => Phrase::reason("vmcSizeMismatch")
            .with("file", *file)
            .with("card", *card),
        VmcProblem::NotWholeMib(size) => Phrase::reason("vmcNotWholeMib").with("size", *size),
    }
}

pub(crate) fn record(anomaly: &RecordAnomaly) -> Phrase {
    match anomaly {
        RecordAnomaly::BadMagic => Phrase::reason("recordBadMagic"),
        RecordAnomaly::Byte08Missing => Phrase::reason("recordByte08Missing"),
        RecordAnomaly::ReservedNotZero => Phrase::reason("recordReservedNotZero"),
        RecordAnomaly::UnknownMedia(byte) => {
            Phrase::reason("recordUnknownMedia").with("value", format!("{byte:#04x}"))
        }
        RecordAnomaly::NoParts => Phrase::reason("recordNoParts"),
        RecordAnomaly::EmptyName => Phrase::reason("recordEmptyName"),
        RecordAnomaly::NonAsciiName => Phrase::reason("recordNonAsciiName"),
        RecordAnomaly::EmptyStartup => Phrase::reason("recordEmptyStartup"),
    }
}

pub(crate) fn cheat(warning: &ChtWarning) -> Phrase {
    match warning {
        ChtWarning::CodesWithoutName(codes) => {
            Phrase::reason("chtCodesWithoutName").with("codes", *codes)
        }
        ChtWarning::TooManyCheats => Phrase::reason("chtTooManyCheats"),
        ChtWarning::BlankName { line } => Phrase::reason("chtBlankName").with("line", *line),
        ChtWarning::LineTooLong { line } => Phrase::reason("chtLineTooLong").with("line", *line),
    }
}

/// Why OPL skips an app.
pub(crate) fn app(err: &AppError) -> Phrase {
    match err {
        AppError::MissingTitle => Phrase::reason("appNoTitle"),
        AppError::MissingBoot => Phrase::reason("appNoBoot"),
        AppError::BadBoot(boot) => Phrase::reason("appBadBoot").with("boot", boot),
        AppError::TooLong { field, len, max } => Phrase::reason("appTooLong")
            .with("field", *field)
            .with("length", *len)
            .with("max", *max),
        AppError::Cfg(err) => Phrase::reason("appCfg").with("error", err.to_string()),
    }
}

pub(crate) fn zso_problem(problem: &ZsoProblem) -> Phrase {
    match problem {
        ZsoProblem::HeaderSize(size) => {
            Phrase::reason("zsoHeaderSize").with("size", u64::from(*size))
        }
        ZsoProblem::BlockSize(size) => {
            Phrase::reason("zsoBlockSize").with("size", u64::from(*size))
        }
        ZsoProblem::Version(version) => {
            Phrase::reason("zsoVersion").with("version", u64::from(*version))
        }
        ZsoProblem::Align(shift) => Phrase::reason("zsoAlign")
            .with("shift", u64::from(*shift))
            .with("max", u64::from(yaoplm_formats::zso::MAX_ALIGN)),
        ZsoProblem::PartialBlock(size) => Phrase::reason("zsoPartialBlock").with("size", *size),
        ZsoProblem::Empty => Phrase::reason("zsoEmpty"),
    }
}

/// Why OPL cannot read a ZSO.
pub(crate) fn zso(err: &ZsoError) -> Phrase {
    match err {
        ZsoError::NotZso => Phrase::reason("zsoNotZso"),
        ZsoError::Unsupported(problem) => zso_problem(problem),
        other => text(other),
    }
}

/// Why a BIN/CUE source cannot be imported.
pub(crate) fn cue(err: &CueError) -> Phrase {
    match err {
        CueError::Syntax { line, problem } => Phrase::reason("cueLine")
            .with("line", *line)
            .with("problem", problem),
        CueError::NoFile => Phrase::reason("cueNoFile"),
        CueError::NoTrack => Phrase::reason("cueNoTrack"),
        CueError::FileType(kind) => Phrase::reason("cueFileType").with("type", kind),
        CueError::SecondDataTrack(track) => {
            Phrase::reason("secondDataTrack").with("track", u64::from(*track))
        }
        CueError::NotData(mode) => Phrase::reason("trackNotData").with("mode", mode),
        CueError::MultiSession => Phrase::reason("cueSessions"),
        CueError::NoIndex => Phrase::reason("cueNoIndex"),
        CueError::NextTrackNoIndex(track) => {
            Phrase::reason("cueTrackNoEnd").with("track", u64::from(*track))
        }
        CueError::TrackOrder(track) => {
            Phrase::reason("cueTrackOrder").with("track", u64::from(*track))
        }
    }
}

pub(crate) fn bin(err: &BinError) -> Phrase {
    match err {
        BinError::PartialSector {
            len,
            start,
            sector_size,
        } => Phrase::reason("binPartialSector")
            .with("length", *len)
            .with("start", *start)
            .with("size", u64::from(*sector_size)),
        BinError::StartPastEnd => Phrase::reason("binStartPastEnd"),
    }
}

#[cfg(test)]
mod tests {
    use yaoplm_formats::cfg::CfgError;
    use yaoplm_formats::iso9660::IsoError;

    use super::*;
    use crate::model::{GameFormat, SkipReason, Warning};

    /// Every text renders from the catalogue with every placeholder
    /// filled: a missing key or parameter panics in `english()`.
    fn renders(phrase: &Phrase) {
        let text = phrase.english();
        assert_ne!(text, phrase.key, "{} is not in the catalogue", phrase.key);
        assert!(!text.contains("{{"), "{text}");
    }

    #[test]
    fn every_reason_renders() {
        let reasons = [
            text("x"),
            system("x"),
            path(&PathError::BadComponent("a/../b".into())),
            name(&NameError::EmptyTitle),
            name(&NameError::TitleTooLong(170)),
            name(&NameError::UnsuitableStartup("ABCD".into())),
            name(&NameError::LooksLikeOldFormat("SLUS_200.62.x".into())),
            ul(&UlError::EmptyName),
            ul(&UlError::NameTooLong(40)),
            ul(&UlError::NameNotAscii),
            ul(&UlError::BadStartup("X".into())),
            ul(&UlError::BadPartCount(0)),
            startup(&StartupError::NoSystemCnf),
            startup(&StartupError::Iso(IsoError::NoPvd)),
            vmc(&VmcProblem::BadMagic),
            vmc(&VmcProblem::CardType(1)),
            vmc(&VmcProblem::SizeMismatch { file: 1, card: 2 }),
            vmc(&VmcProblem::NotWholeMib(3)),
            cheat(&ChtWarning::CodesWithoutName(2)),
            cheat(&ChtWarning::TooManyCheats),
            cheat(&ChtWarning::BlankName { line: 3 }),
            cheat(&ChtWarning::LineTooLong { line: 4 }),
            app(&AppError::MissingTitle),
            app(&AppError::MissingBoot),
            app(&AppError::BadBoot("a/b.elf".into())),
            app(&AppError::TooLong {
                field: "title",
                len: 70,
                max: 64,
            }),
            app(&AppError::Cfg(CfgError::EmptyKey)),
            zso(&ZsoError::NotZso),
            zso(&ZsoError::Stopped),
            cue(&CueError::Syntax {
                line: 2,
                problem: "x".into(),
            }),
            cue(&CueError::NoFile),
            cue(&CueError::NoTrack),
            cue(&CueError::FileType("WAVE".into())),
            cue(&CueError::SecondDataTrack(2)),
            cue(&CueError::NotData("AUDIO".into())),
            cue(&CueError::MultiSession),
            cue(&CueError::NoIndex),
            cue(&CueError::NextTrackNoIndex(2)),
            cue(&CueError::TrackOrder(2)),
            bin(&BinError::PartialSector {
                len: 5,
                start: 0,
                sector_size: 2352,
            }),
            bin(&BinError::StartPastEnd),
        ];
        for reason in &reasons {
            renders(reason);
        }
        for err in [
            CnfError::NoBoot2,
            CnfError::NulByte,
            CnfError::UnexpectedEof,
            CnfError::MissingEquals,
            CnfError::MissingColon,
            CnfError::EmptyStartup,
            CnfError::Unpredictable,
        ] {
            renders(&startup(&StartupError::Cnf(err)));
        }
        for problem in [
            ZsoProblem::HeaderSize(20),
            ZsoProblem::BlockSize(4096),
            ZsoProblem::Version(2),
            ZsoProblem::Align(12),
            ZsoProblem::PartialBlock(5),
            ZsoProblem::Empty,
        ] {
            renders(&zso_problem(&problem));
            renders(&zso(&ZsoError::Unsupported(problem)));
        }
        for anomaly in [
            RecordAnomaly::BadMagic,
            RecordAnomaly::Byte08Missing,
            RecordAnomaly::ReservedNotZero,
            RecordAnomaly::UnknownMedia(0x13),
            RecordAnomaly::NoParts,
            RecordAnomaly::EmptyName,
            RecordAnomaly::NonAsciiName,
            RecordAnomaly::EmptyStartup,
        ] {
            renders(&record(&anomaly));
        }
    }

    #[test]
    fn every_warning_and_skip_reason_renders() {
        let warnings = [
            Warning::StaleCache {
                cached: "SLUS_200.62".into(),
                image: "SLUS_200.63".into(),
            },
            Warning::NameIdMismatch {
                name: "SLUS_200.62".into(),
                image: "SLUS_200.63".into(),
            },
            Warning::UppercaseExtension,
            Warning::NonAsciiName,
            Warning::SmbNameTooLong(170),
            Warning::ContainerMismatch {
                extension: GameFormat::Iso,
                content: GameFormat::Zso,
            },
            Warning::ZsoUnsupportedByProfile,
            Warning::ZsoHeader(ZsoProblem::Version(2)),
            Warning::CdTooLarge(800 << 20),
            Warning::UlRecord(RecordAnomaly::NoParts),
            Warning::UlPartMissing {
                part: 1,
                name: "ul.0.SLUS_200.62.01".into(),
            },
            Warning::UlPartSize { part: 0, size: 5 },
            Warning::UlTooManyPartsForSmb(11),
            Warning::CfgLibconfig,
            Warning::CfgUnreadable("x".into()),
            Warning::CompatibilityBits {
                value: 64,
                allowed: 0x3f,
            },
            Warning::VmcMissing {
                slot: 0,
                name: "a".into(),
            },
            Warning::VmcInvalid {
                slot: 1,
                name: "a".into(),
                problem: VmcProblem::BadMagic,
            },
            Warning::ArtNotLoaded("ART/x.jpg".into()),
            Warning::ArtTooLarge {
                file: "ART/x.png".into(),
                width: 2000,
                height: 2000,
            },
            Warning::Cheats(ChtWarning::TooManyCheats),
            Warning::DuplicateStartup,
            Warning::UlCfgTrailingBytes(12),
            Warning::AppBootMissing("boot.elf".into()),
            Warning::UnfinishedOperation("20261023-1".into()),
        ];
        for warning in &warnings {
            renders(&warning.phrase());
            assert_eq!(warning.phrase().key, format!("notices.{}", warning.code()));
        }
        let skips = [
            SkipReason::UnsupportedExtension(".cso".into()),
            SkipReason::TitleTooLong,
            SkipReason::NoStartup(Phrase::reason("noGameId")),
            SkipReason::Unreadable("x".into()),
            SkipReason::BadApp(app(&AppError::MissingBoot)),
        ];
        for skip in &skips {
            renders(&skip.phrase());
            assert_eq!(skip.phrase().key, format!("notices.{}", skip.code()));
        }
    }
}
