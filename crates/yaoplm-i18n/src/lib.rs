//! User-facing texts as a catalogue key with parameters (docs/I18N.md,
//! ADR-0029). Front ends translate the key into their own language; the
//! crates render the English text from the web UI's `en` catalogue, so the
//! CLI, the logs and the English UI always say the same. A crate of its
//! own so that `yaoplm-device` and `yaoplm-core` can both use it.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

/// The web UI's English catalogue; the crates read `titles.*`, `errors.*`
/// and `notices.*` from it.
static EN: &str = include_str!("../../../web/src/locales/en/translation.json");

/// A text the user sees: a key of the catalogue and named parameters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Phrase {
    /// Such as `titles.importGames`.
    pub key: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, Param>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Param {
    /// A count (`count` picks the plural form) or a size in bytes.
    Number(u64),
    /// A name, path, ID or format, shown as it is.
    Text(String),
    /// A phrase inside a phrase, translated on its own.
    Phrase(Box<Phrase>),
}

impl Phrase {
    /// `titles.<code>`: plans, transactions and jobs.
    pub fn title(code: &str) -> Self {
        Self::keyed(format!("titles.{code}"))
    }

    /// `errors.<code>`: errors the front ends show.
    pub fn error(code: &str) -> Self {
        Self::keyed(format!("errors.{code}"))
    }

    /// `notices.<code>`: plan notices, game warnings, device advice and
    /// the reasons an item is skipped.
    pub fn notice(code: &str) -> Self {
        Self::keyed(format!("notices.{code}"))
    }

    /// `reasons.<code>`: a cause written to follow a colon inside another
    /// phrase, such as why OPL hides a disc.
    pub fn reason(code: &str) -> Self {
        Self::keyed(format!("reasons.{code}"))
    }

    fn keyed(key: String) -> Self {
        Self {
            key,
            params: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn with(mut self, name: &str, value: impl Into<Param>) -> Self {
        self.params.insert(name.to_owned(), value.into());
        self
    }

    /// Sets `count`, which also picks the plural form.
    #[must_use]
    pub fn count(self, count: usize) -> Self {
        self.with("count", count as u64)
    }

    /// The English text, as the catalogue has it.
    pub fn english(&self) -> String {
        let catalogue = catalogue();
        // English plurals: `_one` for 1, `_other` for everything else.
        let form = match self.params.get("count") {
            Some(Param::Number(1)) => Some("one"),
            Some(Param::Number(_)) => Some("other"),
            _ => None,
        };
        let template = form
            .and_then(|form| catalogue.get(&format!("{}_{form}", self.key)))
            .or_else(|| catalogue.get(&self.key));
        let Some(template) = template else {
            debug_assert!(false, "{} is not in the en catalogue", self.key);
            return self.key.clone();
        };
        interpolate(template, &self.params)
    }
}

impl fmt::Display for Phrase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.english())
    }
}

impl From<u64> for Param {
    fn from(value: u64) -> Self {
        Self::Number(value)
    }
}

impl From<u8> for Param {
    fn from(value: u8) -> Self {
        Self::Number(u64::from(value))
    }
}

impl From<u32> for Param {
    fn from(value: u32) -> Self {
        Self::Number(u64::from(value))
    }
}

impl From<usize> for Param {
    fn from(value: usize) -> Self {
        Self::Number(value as u64)
    }
}

impl From<String> for Param {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl<T: AsRef<str> + ?Sized> From<&T> for Param {
    fn from(value: &T) -> Self {
        Self::Text(value.as_ref().to_owned())
    }
}

impl From<Phrase> for Param {
    fn from(value: Phrase) -> Self {
        Self::Phrase(Box::new(value))
    }
}

/// Every string of the catalogue under its dotted key.
/// The English text of any key of the catalogue (`genre.action`), for
/// values the crates print as words rather than as phrases.
pub fn text(key: &str) -> Option<&'static str> {
    catalogue().get(key).map(String::as_str)
}

fn catalogue() -> &'static HashMap<String, String> {
    static CATALOGUE: OnceLock<HashMap<String, String>> = OnceLock::new();
    CATALOGUE.get_or_init(|| {
        let mut out = HashMap::new();
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(EN) {
            flatten(&value, "", &mut out);
        }
        out
    })
}

fn flatten(value: &serde_json::Value, prefix: &str, out: &mut HashMap<String, String>) {
    match value {
        serde_json::Value::String(text) => {
            out.insert(prefix.to_owned(), text.clone());
        }
        serde_json::Value::Object(map) => {
            for (key, value) in map {
                let key = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                flatten(value, &key, out);
            }
        }
        _ => {}
    }
}

/// i18next's `{{name}}` and `{{name, bytes}}`; a name without a value is
/// left as it is (and is a bug in a debug build).
fn interpolate(template: &str, params: &BTreeMap<String, Param>) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        let Some(len) = rest[start..].find("}}") else {
            break;
        };
        out.push_str(&rest[..start]);
        let inner = &rest[start + 2..start + len];
        let (name, format) = match inner.split_once(',') {
            Some((name, format)) => (name.trim(), Some(format.trim())),
            None => (inner.trim(), None),
        };
        match (params.get(name), format) {
            (Some(Param::Number(n)), Some("bytes")) => out.push_str(&bytes(*n)),
            (Some(Param::Number(n)), _) => out.push_str(&n.to_string()),
            (Some(Param::Text(text)), _) => out.push_str(text),
            (Some(Param::Phrase(phrase)), _) => out.push_str(&phrase.english()),
            (None, _) => {
                debug_assert!(false, "no value for {name} in {template}");
                out.push_str(&rest[start..start + len + 2]);
            }
        }
        rest = &rest[start + len + 2..];
    }
    out.push_str(rest);
    out
}

/// Sizes as the web UI writes them in English: `891 B`, `46.0 KiB`.
fn bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{n} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_plurals_params_and_nested_phrases() {
        assert_eq!(
            Phrase::title("importGames").count(1).english(),
            "Import 1 game"
        );
        assert_eq!(
            Phrase::title("importGames").count(3).english(),
            "Import 3 games"
        );
        let inner = Phrase::title("deleteGame").with("title", "Retail Game");
        assert_eq!(
            Phrase::title("recoveryUndo").with("title", inner).english(),
            "Undo: Delete Retail Game"
        );
    }

    #[test]
    fn formats_bytes_like_the_web_ui() {
        assert_eq!(bytes(891), "891 B");
        assert_eq!(bytes(47_104), "46.0 KiB");
        assert_eq!(bytes(5 << 30), "5.0 GiB");
        let phrase = Phrase::error("noSpace")
            .with("needed", 5u64 << 30)
            .with("free", 1u64 << 20);
        assert_eq!(
            phrase.english(),
            "Not enough free space: 5.0 GiB needed, 1.0 MiB available"
        );
    }

    #[test]
    fn round_trips() {
        let phrase = Phrase::title("convertGames")
            .count(2)
            .with("format", "ZSO")
            .with("nested", Phrase::notice("idKept"));
        let json = serde_json::to_string(&phrase).unwrap();
        assert_eq!(serde_json::from_str::<Phrase>(&json).unwrap(), phrase);
    }
}
