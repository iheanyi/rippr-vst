use std::{
    fs,
    io::{self, BufRead, Write},
    net::ToSocketAddrs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use clap::Parser;
use rippr_core::{
    Acquisition, AcquisitionArtifact, RipError, RipRequest, WORKER_PROTOCOL_VERSION, WorkerCommand,
    WorkerEvent, WorkerMessage, is_private_or_special_address,
};
use serde::Deserialize;

#[derive(Parser)]
struct Arguments {
    #[arg(long)]
    yt_dlp: PathBuf,
    #[arg(long)]
    ffmpeg: PathBuf,
    #[arg(long)]
    workspace: PathBuf,
}

struct ExternalToolsAcquisition {
    yt_dlp: PathBuf,
    ffmpeg: PathBuf,
}

#[derive(Deserialize)]
struct Metadata {
    title: Option<String>,
    uploader: Option<String>,
    duration: Option<f64>,
}

impl Acquisition for ExternalToolsAcquisition {
    fn acquire(
        &self,
        request: &RipRequest,
        working_directory: &Path,
        emit: &mut dyn FnMut(WorkerEvent),
    ) -> Result<AcquisitionArtifact, RipError> {
        validate_public_resolution(&request.source_url)?;
        emit(WorkerEvent::Accepted {
            job_id: request.job_id,
        });
        emit(WorkerEvent::Progress {
            job_id: request.job_id,
            stage: "metadata".into(),
            fraction: None,
        });
        let metadata_output = run(
            &self.yt_dlp,
            "yt-dlp",
            [
                "--ignore-config".into(),
                "--no-plugin-dirs".into(),
                "--dump-single-json".into(),
                "--skip-download".into(),
                "--no-playlist".into(),
                "--".into(),
                request.source_url.clone().into(),
            ],
        )?;
        let metadata: Metadata =
            serde_json::from_slice(&metadata_output.stdout).map_err(|error| {
                RipError::ExternalTool {
                    tool: "yt-dlp",
                    message: format!("invalid metadata JSON: {error}"),
                }
            })?;
        let title = metadata.title.unwrap_or_else(|| "Untitled sample".into());
        emit(WorkerEvent::Metadata {
            job_id: request.job_id,
            title: title.clone(),
            creator: metadata.uploader.clone(),
            duration_seconds: metadata.duration,
        });

        emit(WorkerEvent::Progress {
            job_id: request.job_id,
            stage: "download".into(),
            fraction: None,
        });
        let output_template = working_directory.join("source.%(ext)s");
        let download_output = run(
            &self.yt_dlp,
            "yt-dlp",
            [
                "--ignore-config".into(),
                "--no-plugin-dirs".into(),
                "--no-playlist".into(),
                "--no-warnings".into(),
                "--quiet".into(),
                "--format".into(),
                "bestaudio/best".into(),
                "--output".into(),
                output_template.into_os_string(),
                "--print".into(),
                "after_move:filepath".into(),
                "--".into(),
                request.source_url.clone().into(),
            ],
        )?;
        let downloaded_path = String::from_utf8_lossy(&download_output.stdout)
            .lines()
            .rfind(|line| !line.trim().is_empty())
            .map(PathBuf::from)
            .ok_or_else(|| RipError::ExternalTool {
                tool: "yt-dlp",
                message: "no downloaded media path was reported".into(),
            })?;
        let downloaded_path = downloaded_path.canonicalize()?;
        let job_directory = working_directory.canonicalize()?;
        if !downloaded_path.starts_with(&job_directory) {
            return Err(RipError::UnsafeArtifactPath);
        }
        emit(WorkerEvent::Progress {
            job_id: request.job_id,
            stage: "transcode".into(),
            fraction: None,
        });
        let prepared_path = working_directory.join("prepared.wav");
        run(
            &self.ffmpeg,
            "ffmpeg",
            [
                "-nostdin".into(),
                "-y".into(),
                "-i".into(),
                downloaded_path.into_os_string(),
                "-vn".into(),
                "-ac".into(),
                "2".into(),
                "-c:a".into(),
                "pcm_f32le".into(),
                "-rf64".into(),
                "auto".into(),
                prepared_path.clone().into_os_string(),
            ],
        )?;
        if !prepared_path.is_file() {
            return Err(RipError::MissingArtifact(prepared_path));
        }
        emit(WorkerEvent::Prepared {
            job_id: request.job_id,
            path: prepared_path.clone(),
        });
        Ok(AcquisitionArtifact {
            source_url: request.source_url.clone(),
            title,
            creator: metadata.uploader,
            duration_seconds: metadata.duration,
            sample_path: prepared_path,
        })
    }
}

fn validate_public_resolution(source_url: &str) -> Result<(), RipError> {
    let url = url::Url::parse(source_url).map_err(|_| RipError::UnsupportedUrl)?;
    let host = url.host_str().ok_or(RipError::UnsupportedUrl)?;
    let addresses = (host, url.port_or_known_default().unwrap_or(443))
        .to_socket_addrs()?
        .collect::<Vec<_>>();
    if addresses.is_empty()
        || addresses
            .iter()
            .any(|address| is_private_or_special_address(address.ip()))
    {
        return Err(RipError::UnsupportedUrl);
    }
    Ok(())
}

fn run(
    program: &Path,
    name: &'static str,
    arguments: impl IntoIterator<Item = std::ffi::OsString>,
) -> Result<Output, RipError> {
    let output = Command::new(program).args(arguments).output()?;
    if output.status.success() {
        Ok(output)
    } else {
        Err(RipError::ExternalTool {
            tool: name,
            message: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        })
    }
}

fn write_event(event: WorkerEvent) -> io::Result<()> {
    let message = WorkerMessage {
        protocol_version: WORKER_PROTOCOL_VERSION,
        event,
    };
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    serde_json::to_writer(&mut stdout, &message)?;
    writeln!(stdout)?;
    stdout.flush()
}

fn run_worker(arguments: Arguments) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(&arguments.workspace)?;
    for line in io::stdin().lock().lines() {
        let command: WorkerCommand = serde_json::from_str(&line?)?;
        match command {
            WorkerCommand::Prepare {
                protocol_version,
                mut request,
            } => {
                if protocol_version != WORKER_PROTOCOL_VERSION {
                    write_event(WorkerEvent::Failed {
                        job_id: request.job_id,
                        code: "unsupported_protocol".into(),
                        message: format!("expected protocol {WORKER_PROTOCOL_VERSION}"),
                    })?;
                    continue;
                }
                let validated_request = RipRequest::new(&request.source_url);
                let Ok(validated_request) = validated_request else {
                    write_event(WorkerEvent::Failed {
                        job_id: request.job_id,
                        code: "invalid_request".into(),
                        message: "only a valid public HTTPS URL is supported".into(),
                    })?;
                    continue;
                };
                request.source_url = validated_request.source_url;
                let working_directory = tempfile::Builder::new()
                    .prefix(&format!("job-{}-", request.job_id))
                    .tempdir_in(&arguments.workspace)?;
                let acquisition = ExternalToolsAcquisition {
                    yt_dlp: arguments.yt_dlp.clone(),
                    ffmpeg: arguments.ffmpeg.clone(),
                };
                let result =
                    acquisition.acquire(&request, working_directory.path(), &mut |event| {
                        let _ = write_event(event);
                    });
                match result {
                    Ok(_) => {
                        let _ = working_directory.keep();
                    }
                    Err(error) => write_event(WorkerEvent::Failed {
                        job_id: request.job_id,
                        code: "acquisition_failed".into(),
                        message: error.to_string(),
                    })?,
                }
            }
            WorkerCommand::Cancel {
                protocol_version: _,
                job_id,
            } => write_event(WorkerEvent::Cancelled { job_id })?,
        }
    }
    Ok(())
}

fn main() {
    if let Err(error) = run_worker(Arguments::parse()) {
        eprintln!("rippr-worker: {error}");
        std::process::exit(1);
    }
}
