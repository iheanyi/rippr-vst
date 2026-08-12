use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use rippr_core::{
    Acquisition, AcquisitionArtifact, PlaybackEngine, RipRequest, RipprSession, WorkerEvent,
};
use tempfile::tempdir;

struct FixtureAcquisition {
    calls: Arc<AtomicUsize>,
}

impl Acquisition for FixtureAcquisition {
    fn acquire(
        &self,
        request: &RipRequest,
        working_directory: &Path,
        emit: &mut dyn FnMut(WorkerEvent),
    ) -> Result<AcquisitionArtifact, rippr_core::RipError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        emit(WorkerEvent::Accepted {
            job_id: request.job_id,
        });

        let path = working_directory.join("fixture.wav");
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 48_000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut writer = hound::WavWriter::create(&path, spec)?;
        for [left, right] in [
            [0.25_f32, -0.25_f32],
            [0.50, -0.50],
            [0.75, -0.75],
            [1.00, -1.00],
        ] {
            writer.write_sample(left)?;
            writer.write_sample(right)?;
        }
        writer.finalize()?;

        emit(WorkerEvent::Prepared {
            job_id: request.job_id,
            path: path.clone(),
        });

        Ok(AcquisitionArtifact {
            source_url: request.source_url.clone(),
            title: "Fixture break".into(),
            creator: Some("Fixture artist".into()),
            duration_seconds: Some(4.0 / 48_000.0),
            sample_path: path,
        })
    }
}

#[test]
fn rip_request_becomes_a_library_entry_and_sample_accurate_audio() {
    let root = tempdir().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let acquisition = Arc::new(FixtureAcquisition {
        calls: Arc::clone(&calls),
    });
    let session = RipprSession::open(
        acquisition.clone(),
        root.path().join("library.sqlite3"),
        root.path().join("media"),
        48_000,
    )
    .unwrap();
    let request = RipRequest::new("https://example.test/fixture").unwrap();
    let mut events = Vec::new();

    let outcome = session.rip(request, |event| events.push(event)).unwrap();

    assert_eq!(outcome.entry.title, "Fixture break");
    assert_eq!(outcome.entry.creator.as_deref(), Some("Fixture artist"));
    assert!(
        events
            .iter()
            .any(|event| matches!(event, WorkerEvent::Accepted { .. }))
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, WorkerEvent::Prepared { .. }))
    );

    let mut engine = PlaybackEngine::new();
    engine.activate(outcome.sample.clone().unwrap());
    engine.trigger_at(2);
    let mut output = [[0.0_f32; 2]; 6];
    engine.render(&mut output, 1.0);

    assert_eq!(
        output,
        [
            [0.0, 0.0],
            [0.0, 0.0],
            [0.25, -0.25],
            [0.50, -0.50],
            [0.75, -0.75],
            [1.00, -1.00],
        ]
    );

    engine.trigger_now();
    engine.stop();
    let mut stopped_output = [[1.0_f32; 2]; 2];
    engine.render(&mut stopped_output, 1.0);
    assert_eq!(stopped_output, [[0.0, 0.0]; 2]);

    let library = session.library_entries().unwrap();
    assert_eq!(library.len(), 1);
    assert_eq!(library[0].id, outcome.entry.id);
    assert_eq!(
        outcome.entry.waveform_peaks,
        [[-0.25, 0.25], [-0.5, 0.5], [-0.75, 0.75], [-1.0, 1.0],]
    );

    let restored = session.load_entry(&outcome.entry.id).unwrap().unwrap();
    assert_eq!(restored.entry.id, outcome.entry.id);
    assert_eq!(restored.sample.unwrap().frame_count(), 4);

    let duplicate_request = RipRequest::new("https://example.test/fixture").unwrap();
    let duplicate = session.rip(duplicate_request, |_| {}).unwrap();
    assert_eq!(duplicate.entry.id, outcome.entry.id);
    assert_eq!(calls.load(Ordering::Relaxed), 1);

    let different_host_rate = RipprSession::open(
        acquisition,
        root.path().join("library.sqlite3"),
        root.path().join("media"),
        44_100,
    )
    .unwrap();
    let at_44_1 = different_host_rate
        .rip(
            RipRequest::new("https://example.test/fixture").unwrap(),
            |_| {},
        )
        .unwrap();
    assert_eq!(at_44_1.entry.id, outcome.entry.id);
    assert_eq!(at_44_1.entry.rendered_sample_rate, 48_000);
    assert_eq!(at_44_1.entry.frame_count, 4);
    assert_eq!(at_44_1.sample.unwrap().sample_rate(), 44_100);
    assert_eq!(calls.load(Ordering::Relaxed), 1);
}
