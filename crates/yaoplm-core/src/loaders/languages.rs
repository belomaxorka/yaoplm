//! OPL language packs (SPEC LDR-3, ADR-0042): the release's archive holds
//! `lang_<Language>.lng` and `font_<Language>.ttf|otf` per language. The
//! betas wrap a zip of `<Language>-<version>/` folders in a 7z, 1.1.0 keeps
//! `<Language>/` folders in the 7z itself (research/17 §7.3), so files are
//! found by name at any depth and one nested zip deep. OPL reads only files
//! directly in `LNG/` (`src/lang.c`), so they are installed flat.

use std::collections::BTreeMap;
use std::io::{self, Cursor, Read};

use yaoplm_formats::archive::{self, Kind};
use yaoplm_i18n::Phrase;

use super::LoaderError;
use super::download::{MAX_FILE, archive_kind};

/// A language file is tens of kilobytes, a font a few hundred.
const MAX_MEMBER: u64 = 8 << 20;

/// A file to put in `LNG/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageFile {
    /// `lang_Russian.lng`, `font_Russian.ttf`.
    pub name: String,
    pub bytes: Vec<u8>,
}

/// The language a file name belongs to, and whether it is the font.
fn language_of(file: &str) -> Option<(String, bool)> {
    let lower = file.to_ascii_lowercase();
    if let Some(rest) = file
        .strip_prefix("lang_")
        .filter(|_| lower.ends_with(".lng"))
    {
        let name = &rest[..rest.len() - 4];
        return (!name.is_empty()).then(|| (name.to_owned(), false));
    }
    if let Some(rest) = file.strip_prefix("font_")
        && (lower.ends_with(".ttf") || lower.ends_with(".otf"))
    {
        let name = &rest[..rest.len() - 4];
        return (!name.is_empty()).then(|| (name.to_owned(), true));
    }
    None
}

/// Every language file in the archive, by language, one level of nested
/// zip included.
fn collect(name: &str, bytes: &[u8]) -> Result<BTreeMap<String, Vec<LanguageFile>>, LoaderError> {
    let unreadable = |error: String| {
        LoaderError::new(
            Phrase::error("loaderArchiveUnreadable")
                .with("archive", name)
                .with("error", error),
        )
    };
    let kind = archive_kind(name, bytes).ok_or_else(|| unreadable("not a zip or 7z".into()))?;
    let mut found: BTreeMap<String, Vec<LanguageFile>> = BTreeMap::new();
    let mut nested: Vec<(String, Vec<u8>)> = Vec::new();
    let mut walk = |kind: Kind, bytes: &[u8], nested: &mut Vec<(String, Vec<u8>)>, deep: bool| {
        archive::for_each_file(kind, &mut Cursor::new(bytes), &mut |entry, reader| {
            let file = entry
                .name
                .rsplit('/')
                .next()
                .unwrap_or(&entry.name)
                .to_owned();
            let zip = file.to_ascii_lowercase().ends_with(".zip");
            let wanted = language_of(&file);
            if wanted.is_none() && !(zip && !deep) {
                return Ok(());
            }
            let cap = if zip { MAX_FILE } else { MAX_MEMBER };
            if entry.size > cap {
                return Err(io::Error::other(format!("{} is too large", entry.name)));
            }
            let mut data = Vec::new();
            reader.take(cap + 1).read_to_end(&mut data)?;
            match wanted {
                Some((language, _)) => {
                    let files = found.entry(language).or_default();
                    if !files.iter().any(|f| f.name == file) {
                        files.push(LanguageFile {
                            name: file,
                            bytes: data,
                        });
                    }
                }
                None => nested.push((file, data)),
            }
            Ok(())
        })
    };
    walk(kind, bytes, &mut nested, false).map_err(|e| unreadable(e.to_string()))?;
    for (file, data) in std::mem::take(&mut nested) {
        walk(Kind::Zip, &data, &mut Vec::new(), true)
            .map_err(|e| unreadable(format!("{file}: {e}")))?;
    }
    // A font alone is no language.
    found.retain(|_, files| files.iter().any(|f| f.name.ends_with(".lng")));
    Ok(found)
}

/// The languages a pack offers, sorted.
pub fn names(name: &str, bytes: &[u8]) -> Result<Vec<String>, LoaderError> {
    Ok(collect(name, bytes)?.into_keys().collect())
}

/// The files of `language` (its `.lng`, and its font when it has one).
pub fn files(name: &str, bytes: &[u8], language: &str) -> Result<Vec<LanguageFile>, LoaderError> {
    let mut all = collect(name, bytes)?;
    let key = all
        .keys()
        .find(|k| k.eq_ignore_ascii_case(language))
        .cloned()
        .ok_or_else(|| {
            LoaderError::new(
                Phrase::error("loaderLanguageMissing")
                    .with("language", language)
                    .with("archive", name),
            )
        })?;
    let mut files = all.remove(&key).unwrap_or_default();
    files.sort_by(|a, b| {
        b.name
            .ends_with(".lng")
            .cmp(&a.name.ends_with(".lng"))
            .then(a.name.cmp(&b.name))
    });
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loaders::download::tests::zip;

    #[test]
    fn languages_by_file_name() {
        assert_eq!(
            language_of("lang_Russian.lng"),
            Some(("Russian".into(), false))
        );
        assert_eq!(
            language_of("font_Japanese.otf"),
            Some(("Japanese".into(), true))
        );
        assert_eq!(language_of("lang_.lng"), None);
        assert_eq!(language_of("Readme.txt"), None);
    }

    #[test]
    fn a_zip_inside_the_archive_is_read_too() {
        // The betas' layout: a zip of versioned folders inside the archive.
        let inner = zip(&[
            ("README", b"how to".as_slice()),
            ("Russian-v1.2.0-Beta-2271-b3a2199/lang_Russian.lng", b"ru"),
            ("Russian-v1.2.0-Beta-2271-b3a2199/font_Russian.ttf", b"font"),
            ("German-v1.2.0-Beta-2271-b3a2199/lang_German.lng", b"de"),
            (
                "Arabic-v1.2.0-Beta-2271-b3a2199/font_Arabic.ttf",
                b"font only",
            ),
        ]);
        let outer = zip(&[(
            "OPNPS2LD-LANGS/OPNPS2LD-LANGS-v1.2.0-Beta-2271-b3a2199.zip",
            &inner,
        )]);
        assert_eq!(
            names("OPNPS2LD-LANGS.zip", &outer).unwrap(),
            ["German", "Russian"]
        );
        let got = files("OPNPS2LD-LANGS.zip", &outer, "russian").unwrap();
        let named: Vec<&str> = got.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(named, ["lang_Russian.lng", "font_Russian.ttf"]);
        assert_eq!(got[0].bytes, b"ru");
        assert_eq!(
            files("x.zip", &outer, "Klingon").unwrap_err().phrase.key,
            "errors.loaderLanguageMissing"
        );
    }

    #[test]
    fn folders_of_the_stable_pack() {
        let pack = zip(&[
            (
                "OPNPS2LD-LANGUAGES-AND-FONTS-v1.1.0/Readme.txt",
                b"readme".as_slice(),
            ),
            (
                "OPNPS2LD-LANGUAGES-AND-FONTS-v1.1.0/Russian/lang_Russian.lng",
                b"ru",
            ),
            (
                "OPNPS2LD-LANGUAGES-AND-FONTS-v1.1.0/Russian/font_Russian.ttf",
                b"font",
            ),
        ]);
        assert_eq!(names("pack.zip", &pack).unwrap(), ["Russian"]);
        assert_eq!(files("pack.zip", &pack, "Russian").unwrap().len(), 2);
    }
}
