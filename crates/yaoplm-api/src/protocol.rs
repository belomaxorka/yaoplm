use serde::{Deserialize, Serialize};

#[cfg(feature = "ts-export")]
use ts_rs::TS;

use crate::ApiError;
use crate::library::{ImageDetails, Library, LoaderProfile};

/// An operation requested by a front end.
///
/// Serialized as a JSON object tagged by `type`, e.g. `{"type":"Ping"}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(tag = "type")]
pub enum Request {
    /// Liveness check; answered with [`Response::Pong`].
    Ping,
    /// Application name and version.
    Version,
    /// Lists a device root (or its `usb_prefix` folder) as OPL would.
    /// Answered with [`Response::Library`].
    ScanLibrary {
        root: String,
        /// Defaults to OPL 1.2.0 beta.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        profile: Option<LoaderProfile>,
        /// Also read images whose ID OPL takes from the name or
        /// `games.bin`, to catch mismatches. Defaults to true.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        verify: Option<bool>,
    },
    /// Details of one ISO or ZSO file. Answered with [`Response::ImageInfo`].
    ImageInfo {
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        profile: Option<LoaderProfile>,
    },
}

/// The successful answer to a [`Request`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(tag = "type")]
pub enum Response {
    Pong,
    Version { name: String, version: String },
    Library(Box<Library>),
    ImageInfo(Box<ImageDetails>),
}

/// Identifies a long-running job for its whole lifetime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
pub struct JobId(pub u64);

/// Progress of a long-running job, streamed to subscribers.
///
/// Every job emits `Started`, any number of `Progress` events, then exactly
/// one of `Finished`, `Failed` or `Cancelled`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(tag = "type")]
pub enum JobEvent {
    Started {
        job: JobId,
    },
    /// Work done so far, in bytes for copies and hashes. `total` is `None`
    /// while the size is still unknown.
    Progress {
        job: JobId,
        done: u64,
        total: Option<u64>,
    },
    Finished {
        job: JobId,
    },
    Failed {
        job: JobId,
        error: ApiError,
    },
    Cancelled {
        job: JobId,
    },
}
