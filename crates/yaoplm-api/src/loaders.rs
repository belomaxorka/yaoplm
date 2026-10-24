//! The loader catalogue as the front ends see it (LDR-1, LDR-6): the build
//! cards, their channels and presets. The texts come in every interface
//! language from the catalogue itself (ADR-0038); the front end picks one.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use yaoplm_core::loaders::{self, Build, DocKind, Launcher, Publisher, Target};

#[cfg(feature = "ts-export")]
use ts_rs::TS;

use crate::ApiError;
use crate::library::LoaderProfile;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct LoaderCatalog {
    pub schema: u32,
    /// In catalogue order.
    pub builds: Vec<LoaderBuild>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct LoaderBuild {
    pub id: String,
    /// `opl`, `riptopl`, `wopl`, `uopl`, `nhddl`.
    pub family: String,
    pub name: String,
    pub publisher: LoaderPublisher,
    /// `owner/name` on GitHub.
    pub repo: String,
    pub profile: LoaderProfile,
    pub recommended: bool,
    pub devices: Vec<LoaderTarget>,
    pub license: LoaderLicense,
    /// By interface language (`en`, `ru`).
    pub summary: BTreeMap<String, String>,
    pub limits: BTreeMap<String, String>,
    pub docs: Vec<LoaderDoc>,
    pub channel: LoaderChannel,
    pub presets: Vec<LoaderPreset>,
    /// The preset a wizard starts with on each device.
    pub default_presets: Vec<LoaderDefaultPreset>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum LoaderPublisher {
    Official,
    Fork,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum LoaderTarget {
    Usb,
    Mx4sio,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct LoaderLicense {
    pub spdx: String,
    pub url: String,
    pub authors: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum LoaderDocKind {
    Readme,
    Site,
    Wiki,
    Fork,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct LoaderDoc {
    pub kind: LoaderDocKind,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct LoaderChannel {
    pub tag: String,
    /// The tag is rebuilt in place (`latest`).
    pub floating: bool,
    pub prerelease: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum LoaderLauncher {
    Fmcb,
    Ps2bbl,
    Ps2bblMx4sio,
    Osdmenu,
    OplApps,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct LoaderPreset {
    /// `fmcb`, `ps2bbl`, `app`.
    pub id: String,
    /// Relative to the device root.
    pub path: String,
    /// The `title.cfg` title of an app folder.
    pub title: Option<String>,
    pub devices: Vec<LoaderTarget>,
    pub launchers: Vec<LoaderLauncher>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct LoaderDefaultPreset {
    pub device: LoaderTarget,
    pub preset: String,
}

fn target(target: Target) -> LoaderTarget {
    match target {
        Target::Usb => LoaderTarget::Usb,
        Target::Mx4sio => LoaderTarget::Mx4sio,
    }
}

fn launcher(launcher: Launcher) -> LoaderLauncher {
    match launcher {
        Launcher::Fmcb => LoaderLauncher::Fmcb,
        Launcher::Ps2bbl => LoaderLauncher::Ps2bbl,
        Launcher::Ps2bblMx4sio => LoaderLauncher::Ps2bblMx4sio,
        Launcher::Osdmenu => LoaderLauncher::Osdmenu,
        Launcher::OplApps => LoaderLauncher::OplApps,
    }
}

fn build(build: &Build) -> LoaderBuild {
    LoaderBuild {
        id: build.id.clone(),
        family: build.family.name().to_ascii_lowercase(),
        name: build.name.clone(),
        publisher: match build.publisher {
            Publisher::Official => LoaderPublisher::Official,
            Publisher::Fork => LoaderPublisher::Fork,
        },
        repo: build.repo.clone(),
        profile: build.profile.into(),
        recommended: build.recommended,
        devices: build.devices.iter().copied().map(target).collect(),
        license: LoaderLicense {
            spdx: build.license.spdx.clone(),
            url: build.license.url.clone(),
            authors: build.license.authors.clone(),
        },
        summary: build.summary.clone(),
        limits: build.limits.clone(),
        docs: build
            .docs
            .iter()
            .map(|doc| LoaderDoc {
                kind: match doc.kind {
                    DocKind::Readme => LoaderDocKind::Readme,
                    DocKind::Site => LoaderDocKind::Site,
                    DocKind::Wiki => LoaderDocKind::Wiki,
                    DocKind::Fork => LoaderDocKind::Fork,
                },
                url: doc.url.clone(),
            })
            .collect(),
        channel: LoaderChannel {
            tag: build.channel.tag.clone(),
            floating: build.channel.floating,
            prerelease: build.channel.prerelease,
        },
        presets: build
            .layout
            .presets
            .iter()
            .map(|preset| LoaderPreset {
                id: preset.id.clone(),
                path: preset.path.clone(),
                title: preset.title.clone(),
                devices: preset.devices.iter().copied().map(target).collect(),
                launchers: preset.launchers.iter().copied().map(launcher).collect(),
            })
            .collect(),
        default_presets: build
            .layout
            .defaults
            .iter()
            .map(|(device, preset)| LoaderDefaultPreset {
                device: target(*device),
                preset: preset.clone(),
            })
            .collect(),
    }
}

/// The catalogue compiled into the program.
pub fn catalog() -> Result<LoaderCatalog, ApiError> {
    let catalog = loaders::built_in().map_err(|e| ApiError::internal_error(e.to_string()))?;
    Ok(LoaderCatalog {
        schema: catalog.schema,
        builds: catalog.builds.iter().map(build).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalogue_reaches_the_front_ends() {
        let catalog = catalog().unwrap();
        let ids: Vec<&str> = catalog.builds.iter().map(|b| b.id.as_str()).collect();
        assert_eq!(ids, ["opl-stable", "opl-beta", "yaopl"]);
        let beta = &catalog.builds[1];
        assert!(beta.recommended);
        assert_eq!(beta.family, "opl");
        assert_eq!(beta.profile, LoaderProfile::Opl120Beta);
        assert_eq!(catalog.builds[0].profile, LoaderProfile::Opl110);
        assert_eq!(beta.channel.tag, "latest");
        assert!(beta.channel.floating);
        assert_eq!(beta.summary.keys().collect::<Vec<_>>(), ["en", "ru"]);
        let usb = beta
            .default_presets
            .iter()
            .find(|d| d.device == LoaderTarget::Usb)
            .unwrap();
        assert_eq!(usb.preset, "fmcb");
        let yaopl = &catalog.builds[2];
        assert_eq!(yaopl.publisher, LoaderPublisher::Fork);
        let json = serde_json::to_value(beta).unwrap();
        assert_eq!(json["defaultPresets"][0]["device"], "usb");
        assert_eq!(json["presets"][2]["launchers"][1], "ps2bblMx4sio");
    }
}
