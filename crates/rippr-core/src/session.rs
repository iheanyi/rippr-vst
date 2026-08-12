use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use chrono::Utc;
use sha2::{Digest, Sha256};

use crate::{
    AcquisitionArtifact, LibraryEntry, PreparedSample, RipError, RipOutcome, RipRequest,
    WorkerEvent, analyze_wav, library::LibraryStore,
};

pub trait Acquisition: Send + Sync {
    /// Changes whenever the canonical prepared artifact produced by this
    /// acquisition implementation changes.
    fn cache_version(&self) -> &'static str {
        "prepared-wav-v1"
    }

    fn acquire(
        &self,
        request: &RipRequest,
        working_directory: &Path,
        emit: &mut dyn FnMut(WorkerEvent),
    ) -> Result<AcquisitionArtifact, RipError>;
}

pub struct RipprSession {
    acquisition: Arc<dyn Acquisition>,
    library: LibraryStore,
    media_directory: PathBuf,
    target_sample_rate: u32,
}

impl RipprSession {
    pub fn open(
        acquisition: Arc<dyn Acquisition>,
        database_path: PathBuf,
        media_directory: PathBuf,
        target_sample_rate: u32,
    ) -> Result<Self, RipError> {
        std::fs::create_dir_all(&media_directory)?;
        Ok(Self {
            acquisition,
            library: LibraryStore::open(&database_path)?,
            media_directory,
            target_sample_rate,
        })
    }

    /// Opens only the local library/cache side of a session. Any accidental
    /// acquisition attempt fails, which keeps project restoration offline.
    pub fn open_cache(
        database_path: PathBuf,
        media_directory: PathBuf,
        target_sample_rate: u32,
    ) -> Result<Self, RipError> {
        Self::open(
            Arc::new(CacheOnlyAcquisition),
            database_path,
            media_directory,
            target_sample_rate,
        )
    }

    pub fn rip(
        &self,
        request: RipRequest,
        mut emit: impl FnMut(WorkerEvent),
    ) -> Result<RipOutcome, RipError> {
        let identity = acquisition_identity(&request, self.acquisition.cache_version());
        if let Some(mut entry) = self.library.get(&identity)?
            && entry.media_path.is_file()
        {
            let sample = preview_sample(&entry.media_path, self.target_sample_rate)?;
            if entry.waveform_peaks.is_empty() {
                entry.waveform_peaks = analyze_wav(&entry.media_path, 160)?.waveform_peaks;
                self.library.insert(&entry)?;
            }
            return Ok(RipOutcome { entry, sample });
        }

        let staging_root = self.media_directory.join("staging");
        std::fs::create_dir_all(&staging_root)?;
        let working_directory = tempfile::Builder::new()
            .prefix("rip-job-")
            .tempdir_in(staging_root)?;
        let artifact = self
            .acquisition
            .acquire(&request, working_directory.path(), &mut emit)?;
        if !artifact.sample_path.is_file() {
            return Err(RipError::MissingArtifact(artifact.sample_path));
        }

        let media_path = self.media_directory.join(format!("{identity}.wav"));
        publish_atomically(&artifact.sample_path, &media_path)?;
        let analysis = analyze_wav(&media_path, 160)?;
        let sample = preview_sample(&media_path, self.target_sample_rate)?;
        let entry = LibraryEntry {
            id: identity,
            source_url: artifact.source_url,
            title: artifact.title,
            creator: artifact.creator,
            source_duration_seconds: artifact
                .duration_seconds
                .unwrap_or(analysis.frame_count as f64 / f64::from(analysis.sample_rate)),
            rendered_sample_rate: analysis.sample_rate,
            frame_count: analysis.frame_count,
            waveform_peaks: analysis.waveform_peaks,
            media_path,
            created_at: Utc::now(),
        };
        self.library.insert(&entry)?;
        Ok(RipOutcome { entry, sample })
    }

    /// Lists the shared cache without performing acquisition or touching the network.
    pub fn library_entries(&self) -> Result<Vec<LibraryEntry>, RipError> {
        self.library.list()
    }

    /// Restores a cached entry for this session's host sample rate without reacquiring it.
    pub fn load_entry(&self, id: &str) -> Result<Option<RipOutcome>, RipError> {
        let Some(mut entry) = self.library.get(id)? else {
            return Ok(None);
        };
        if !entry.media_path.is_file() {
            return Ok(None);
        }
        let sample = preview_sample(&entry.media_path, self.target_sample_rate)?;
        if entry.waveform_peaks.is_empty() {
            entry.waveform_peaks = analyze_wav(&entry.media_path, 160)?.waveform_peaks;
            self.library.insert(&entry)?;
        }
        Ok(Some(RipOutcome { entry, sample }))
    }
}

fn preview_sample(
    path: &Path,
    target_sample_rate: u32,
) -> Result<Option<PreparedSample>, RipError> {
    match PreparedSample::from_wav(path, target_sample_rate) {
        Ok(sample) => Ok(Some(sample)),
        Err(RipError::PreviewTooLarge) => Ok(None),
        Err(error) => Err(error),
    }
}

struct CacheOnlyAcquisition;

impl Acquisition for CacheOnlyAcquisition {
    fn acquire(
        &self,
        _request: &RipRequest,
        _working_directory: &Path,
        _emit: &mut dyn FnMut(WorkerEvent),
    ) -> Result<AcquisitionArtifact, RipError> {
        Err(RipError::Protocol(
            "acquisition is disabled for an offline cache session".into(),
        ))
    }
}

fn acquisition_identity(request: &RipRequest, cache_version: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(b"rippr-vst-full-source-output-v2\0");
    digest.update(request.source_url.as_bytes());
    digest.update(cache_version.as_bytes());
    hex::encode(digest.finalize())
}

fn publish_atomically(source: &Path, destination: &Path) -> Result<(), RipError> {
    if destination.is_file() {
        return Ok(());
    }
    let temporary = destination.with_extension(format!("{}.partial", uuid::Uuid::new_v4()));
    std::fs::copy(source, &temporary)?;
    match std::fs::rename(&temporary, destination) {
        Ok(()) => Ok(()),
        Err(_error) if destination.is_file() => {
            let _ = std::fs::remove_file(temporary);
            Ok(())
        }
        Err(error) => {
            let _ = std::fs::remove_file(temporary);
            Err(error.into())
        }
    }
}
