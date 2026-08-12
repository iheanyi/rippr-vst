//! Domain, persistence, and sample playback primitives for rippr-vst.

mod domain;
mod error;
mod library;
mod playback;
mod sample;
mod session;
mod worker_client;

pub use domain::{
    AcquisitionArtifact, LibraryEntry, RipOutcome, RipRequest, WORKER_PROTOCOL_VERSION,
    WorkerCommand, WorkerEvent, WorkerMessage, is_private_or_special_address,
};
pub use error::RipError;
pub use playback::PlaybackEngine;
pub use sample::{PreparedSample, WavAnalysis, analyze_wav, waveform_peaks_from_wav};
pub use session::{Acquisition, RipprSession};
pub use worker_client::WorkerProcessAcquisition;
