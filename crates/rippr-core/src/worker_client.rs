use std::{
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use crate::{
    Acquisition, AcquisitionArtifact, RipError, RipRequest, WORKER_PROTOCOL_VERSION, WorkerCommand,
    WorkerEvent, WorkerMessage,
};

pub struct WorkerProcessAcquisition {
    worker_path: PathBuf,
    yt_dlp_path: PathBuf,
    ffmpeg_path: PathBuf,
    workspace: PathBuf,
    max_download_bytes: u64,
    max_duration_seconds: f64,
}

impl WorkerProcessAcquisition {
    pub fn new(
        worker_path: PathBuf,
        yt_dlp_path: PathBuf,
        ffmpeg_path: PathBuf,
        workspace: PathBuf,
    ) -> Self {
        Self {
            worker_path,
            yt_dlp_path,
            ffmpeg_path,
            workspace,
            max_download_bytes: 256 * 1024 * 1024,
            max_duration_seconds: 60.0 * 60.0,
        }
    }

    pub fn with_limits(mut self, max_download_bytes: u64, max_duration_seconds: f64) -> Self {
        self.max_download_bytes = max_download_bytes;
        self.max_duration_seconds = max_duration_seconds;
        self
    }
}

impl Acquisition for WorkerProcessAcquisition {
    fn acquire(
        &self,
        request: &RipRequest,
        working_directory: &Path,
        emit: &mut dyn FnMut(WorkerEvent),
    ) -> Result<AcquisitionArtifact, RipError> {
        std::fs::create_dir_all(&self.workspace)?;
        let mut child = Command::new(&self.worker_path)
            .args([
                "--yt-dlp".as_ref(),
                self.yt_dlp_path.as_os_str(),
                "--ffmpeg".as_ref(),
                self.ffmpeg_path.as_os_str(),
                "--workspace".as_ref(),
                self.workspace.as_os_str(),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let command = WorkerCommand::Prepare {
            protocol_version: WORKER_PROTOCOL_VERSION,
            request: request.clone(),
            max_download_bytes: self.max_download_bytes,
            max_duration_seconds: self.max_duration_seconds,
        };
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| RipError::Protocol("worker standard input was not available".into()))?;
        serde_json::to_writer(&mut stdin, &command)
            .map_err(|error| RipError::Protocol(error.to_string()))?;
        writeln!(stdin)?;
        drop(stdin);

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| RipError::Protocol("worker standard output was not available".into()))?;
        let mut title = None;
        let mut creator = None;
        let mut duration_seconds = None;
        let mut sample_path = None;
        for line in BufReader::new(stdout).lines() {
            let message: WorkerMessage = serde_json::from_str(&line?)
                .map_err(|error| RipError::Protocol(error.to_string()))?;
            if message.protocol_version != WORKER_PROTOCOL_VERSION {
                return Err(RipError::Protocol(format!(
                    "expected version {WORKER_PROTOCOL_VERSION}, received {}",
                    message.protocol_version
                )));
            }
            match &message.event {
                WorkerEvent::Metadata {
                    title: event_title,
                    creator: event_creator,
                    duration_seconds: event_duration,
                    ..
                } => {
                    title = Some(event_title.clone());
                    creator = event_creator.clone();
                    duration_seconds = *event_duration;
                }
                WorkerEvent::Prepared { path, .. } => sample_path = Some(path.clone()),
                WorkerEvent::Failed { message, .. } => {
                    return Err(RipError::Protocol(message.clone()));
                }
                WorkerEvent::Cancelled { .. } => {
                    return Err(RipError::Protocol("worker cancelled the job".into()));
                }
                _ => {}
            }
            emit(message.event);
        }
        let output = child.wait_with_output()?;
        if !output.status.success() {
            return Err(RipError::ExternalTool {
                tool: "rippr-worker",
                message: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            });
        }
        let worker_sample_path = sample_path
            .ok_or_else(|| RipError::Protocol("worker exited before preparing media".into()))?;
        let worker_sample_path = worker_sample_path.canonicalize()?;
        let worker_workspace = self.workspace.canonicalize()?;
        if !worker_sample_path.starts_with(&worker_workspace) {
            return Err(RipError::UnsafeArtifactPath);
        }
        let sample_path = working_directory.join("prepared.wav");
        std::fs::copy(&worker_sample_path, &sample_path)?;
        if let Some(job_directory) = worker_sample_path.parent()
            && job_directory != worker_workspace
        {
            let _ = std::fs::remove_dir_all(job_directory);
        }
        Ok(AcquisitionArtifact {
            source_url: request.source_url.clone(),
            title: title.unwrap_or_else(|| "Untitled sample".into()),
            creator,
            duration_seconds: duration_seconds
                .unwrap_or(request.trim.end_seconds - request.trim.start_seconds),
            sample_path,
        })
    }
}
