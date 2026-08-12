//! Domain, persistence, and sample playback primitives for rippr-vst.

mod domain;
mod error;
mod library;
mod playback;
mod sample;
mod session;
mod worker_client;

pub use domain::{
    AcquisitionArtifact, LibraryEntry, RipOutcome, RipRequest, TrimRange, WORKER_PROTOCOL_VERSION,
    WorkerCommand, WorkerEvent, WorkerMessage,
};
pub use error::RipError;
pub use playback::PlaybackEngine;
pub use sample::PreparedSample;
pub use session::{Acquisition, RipprSession};
pub use worker_client::WorkerProcessAcquisition;
