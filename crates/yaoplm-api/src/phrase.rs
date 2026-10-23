//! Texts front ends translate (docs/I18N.md, ADR-0029): the wire form of
//! [`yaoplm_core::Phrase`].

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
#[cfg(feature = "ts-export")]
use ts_rs::TS;

/// A key of the front end's catalogue with named parameters. Numbers are
/// counts (`count` picks the plural form) or byte sizes; a nested phrase
/// is translated first. The English text always travels next to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
pub struct Phrase {
    pub key: String,
    #[serde(default)]
    pub params: BTreeMap<String, PhraseParam>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(untagged)]
pub enum PhraseParam {
    Number(u64),
    Text(String),
    Phrase(Box<Phrase>),
}

impl From<&yaoplm_core::Phrase> for Phrase {
    fn from(phrase: &yaoplm_core::Phrase) -> Self {
        Self {
            key: phrase.key.clone(),
            params: phrase
                .params
                .iter()
                .map(|(name, value)| {
                    let value = match value {
                        yaoplm_core::Param::Number(n) => PhraseParam::Number(*n),
                        yaoplm_core::Param::Text(text) => PhraseParam::Text(text.clone()),
                        yaoplm_core::Param::Phrase(inner) => {
                            PhraseParam::Phrase(Box::new(Self::from(inner.as_ref())))
                        }
                    };
                    (name.clone(), value)
                })
                .collect(),
        }
    }
}
