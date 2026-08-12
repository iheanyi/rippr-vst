use std::{net::IpAddr, path::PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

use crate::{PreparedSample, RipError};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct TrimRange {
    pub start_seconds: f64,
    pub end_seconds: f64,
}

impl TrimRange {
    pub fn new(start_seconds: f64, end_seconds: f64) -> Result<Self, RipError> {
        if !start_seconds.is_finite()
            || !end_seconds.is_finite()
            || start_seconds < 0.0
            || end_seconds <= start_seconds
        {
            return Err(RipError::InvalidTrimRange);
        }
        Ok(Self {
            start_seconds,
            end_seconds,
        })
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RipRequest {
    pub job_id: Uuid,
    pub source_url: String,
    pub trim: TrimRange,
}

pub const WORKER_PROTOCOL_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum WorkerCommand {
    Prepare {
        protocol_version: u32,
        request: RipRequest,
        max_download_bytes: u64,
        max_duration_seconds: f64,
    },
    Cancel {
        protocol_version: u32,
        job_id: Uuid,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct WorkerMessage {
    pub protocol_version: u32,
    #[serde(flatten)]
    pub event: WorkerEvent,
}

impl RipRequest {
    pub fn new(source_url: impl AsRef<str>, trim: TrimRange) -> Result<Self, RipError> {
        let url = Url::parse(source_url.as_ref()).map_err(|_| RipError::UnsupportedUrl)?;
        if url.scheme() != "https" || url.username() != "" || url.password().is_some() {
            return Err(RipError::UnsupportedUrl);
        }
        let host = url.host_str().ok_or(RipError::UnsupportedUrl)?;
        if host.eq_ignore_ascii_case("localhost")
            || host.ends_with(".localhost")
            || host.parse::<IpAddr>().is_ok_and(is_private_address)
        {
            return Err(RipError::UnsupportedUrl);
        }

        Ok(Self {
            job_id: Uuid::new_v4(),
            source_url: url.to_string(),
            trim,
        })
    }
}

fn is_private_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => {
            address.is_private()
                || address.is_loopback()
                || address.is_link_local()
                || address.is_broadcast()
                || address.is_unspecified()
                || address.octets()[0] == 0
        }
        IpAddr::V6(address) => {
            address.is_loopback()
                || address.is_unspecified()
                || (address.segments()[0] & 0xfe00) == 0xfc00
                || (address.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkerEvent {
    Accepted {
        job_id: Uuid,
    },
    Metadata {
        job_id: Uuid,
        title: String,
        creator: Option<String>,
        duration_seconds: Option<f64>,
    },
    Progress {
        job_id: Uuid,
        stage: String,
        fraction: Option<f32>,
    },
    Prepared {
        job_id: Uuid,
        path: PathBuf,
    },
    Cancelled {
        job_id: Uuid,
    },
    Failed {
        job_id: Uuid,
        code: String,
        message: String,
    },
}

#[derive(Clone, Debug)]
pub struct AcquisitionArtifact {
    pub source_url: String,
    pub title: String,
    pub creator: Option<String>,
    pub duration_seconds: f64,
    pub sample_path: PathBuf,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct LibraryEntry {
    pub id: String,
    pub source_url: String,
    pub title: String,
    pub creator: Option<String>,
    pub source_duration_seconds: f64,
    pub trim: TrimRange,
    pub rendered_sample_rate: u32,
    pub frame_count: usize,
    pub media_path: PathBuf,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug)]
pub struct RipOutcome {
    pub entry: LibraryEntry,
    pub sample: PreparedSample,
}
