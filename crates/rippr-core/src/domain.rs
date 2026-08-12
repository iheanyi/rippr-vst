use std::{net::IpAddr, path::PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use url::{Host, Url};
use uuid::Uuid;

use crate::{PreparedSample, RipError};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RipRequest {
    pub job_id: Uuid,
    pub source_url: String,
}

pub const WORKER_PROTOCOL_VERSION: u32 = 2;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum WorkerCommand {
    Prepare {
        protocol_version: u32,
        request: RipRequest,
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
    pub fn new(source_url: impl AsRef<str>) -> Result<Self, RipError> {
        let url = Url::parse(source_url.as_ref()).map_err(|_| RipError::UnsupportedUrl)?;
        if url.scheme() != "https" || url.username() != "" || url.password().is_some() {
            return Err(RipError::UnsupportedUrl);
        }
        match url.host().ok_or(RipError::UnsupportedUrl)? {
            Host::Ipv4(address) if is_private_or_special_address(address.into()) => {
                return Err(RipError::UnsupportedUrl);
            }
            Host::Ipv6(address) if is_private_or_special_address(address.into()) => {
                return Err(RipError::UnsupportedUrl);
            }
            Host::Domain(domain) => {
                let domain = domain.trim_end_matches('.');
                if domain.eq_ignore_ascii_case("localhost")
                    || domain.to_ascii_lowercase().ends_with(".localhost")
                {
                    return Err(RipError::UnsupportedUrl);
                }
            }
            _ => {}
        }

        Ok(Self {
            job_id: Uuid::new_v4(),
            source_url: url.to_string(),
        })
    }
}

pub fn is_private_or_special_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => {
            address.is_private()
                || address.is_loopback()
                || address.is_link_local()
                || address.is_broadcast()
                || address.is_multicast()
                || address.is_unspecified()
                || address.octets()[0] == 0
        }
        IpAddr::V6(address) => {
            address.is_loopback()
                || address.is_unspecified()
                || address.is_multicast()
                || (address.segments()[0] & 0xfe00) == 0xfc00
                || (address.segments()[0] & 0xffc0) == 0xfe80
                || address
                    .to_ipv4_mapped()
                    .is_some_and(|address| is_private_or_special_address(address.into()))
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
    pub duration_seconds: Option<f64>,
    pub sample_path: PathBuf,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct LibraryEntry {
    pub id: String,
    pub source_url: String,
    pub title: String,
    pub creator: Option<String>,
    pub source_duration_seconds: f64,
    pub rendered_sample_rate: u32,
    pub frame_count: usize,
    pub waveform_peaks: Vec<[f32; 2]>,
    pub media_path: PathBuf,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug)]
pub struct RipOutcome {
    pub entry: LibraryEntry,
    pub sample: Option<PreparedSample>,
}
