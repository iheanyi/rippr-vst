#![cfg(unix)]

use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    process::{Command, Stdio},
};

use rippr_core::{RipRequest, TrimRange, WorkerCommand, WorkerMessage};
use tempfile::tempdir;

#[test]
fn prepares_a_wav_through_direct_tool_arguments() {
    let root = tempdir().unwrap();
    let fixture = root.path().join("fixture.wav");
    let mut writer = hound::WavWriter::create(
        &fixture,
        hound::WavSpec {
            channels: 2,
            sample_rate: 48_000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )
    .unwrap();
    writer.write_sample(0.5_f32).unwrap();
    writer.write_sample(-0.5_f32).unwrap();
    writer.finalize().unwrap();

    let yt_dlp = root.path().join("fake-yt-dlp");
    let ffmpeg = root.path().join("fake-ffmpeg");
    let arguments = root.path().join("ffmpeg-arguments.txt");
    fs::write(
        &yt_dlp,
        r#"#!/bin/sh
case " $* " in
  *" --dump-single-json "*) printf '%s\n' '{"title":"Fixture break","uploader":"Fixture artist","duration":1.0}' ;;
  *)
    previous=""
    for argument in "$@"; do
      if [ "$previous" = "--output" ]; then template="$argument"; fi
      previous="$argument"
    done
    output=$(printf '%s' "$template" | sed 's/%(ext)s/wav/')
    cp "$FAKE_SOURCE_WAV" "$output"
    printf '%s\n' "$output"
    ;;
esac
"#,
    )
    .unwrap();
    fs::write(
        &ffmpeg,
        r#"#!/bin/sh
printf '%s\n' "$@" > "$FAKE_ARGUMENT_LOG"
for argument in "$@"; do output="$argument"; done
cp "$FAKE_SOURCE_WAV" "$output"
"#,
    )
    .unwrap();
    fs::set_permissions(&yt_dlp, fs::Permissions::from_mode(0o755)).unwrap();
    fs::set_permissions(&ffmpeg, fs::Permissions::from_mode(0o755)).unwrap();

    let request = RipRequest::new(
        "https://example.com/watch?v=fixture;touch-pwned",
        TrimRange::new(0.25, 0.75).unwrap(),
    )
    .unwrap();
    let command = WorkerCommand::Prepare {
        protocol_version: 1,
        request,
        max_download_bytes: 1_000_000,
        max_duration_seconds: 60.0,
    };
    let mut child = Command::new(env!("CARGO_BIN_EXE_rippr-worker"))
        .args([
            "--yt-dlp",
            yt_dlp.to_str().unwrap(),
            "--ffmpeg",
            ffmpeg.to_str().unwrap(),
            "--workspace",
            root.path().to_str().unwrap(),
        ])
        .env("FAKE_SOURCE_WAV", &fixture)
        .env("FAKE_ARGUMENT_LOG", &arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(
        child.stdin.take().unwrap(),
        "{}",
        serde_json::to_string(&command).unwrap()
    )
    .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let messages = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<WorkerMessage>(line).unwrap())
        .collect::<Vec<_>>();
    let prepared_path = messages
        .iter()
        .find_map(|message| match &message.event {
            rippr_core::WorkerEvent::Prepared { path, .. } => Some(path),
            _ => None,
        })
        .expect("worker should publish a prepared event");
    assert!(prepared_path.is_file());

    let arguments = fs::read_to_string(arguments).unwrap();
    assert!(arguments.lines().any(|argument| argument == "-ss"));
    assert!(arguments.lines().any(|argument| argument == "0.250000"));
    assert!(arguments.lines().any(|argument| argument == "-t"));
    assert!(arguments.lines().any(|argument| argument == "0.500000"));
    assert!(!root.path().join("pwned").exists());
}
