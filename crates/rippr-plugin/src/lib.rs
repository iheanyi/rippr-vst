use std::{
    any::Any,
    num::NonZeroU32,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    },
    thread::JoinHandle,
};

use directories::ProjectDirs;
use nice_plug::{
    editor::dpi::{LogicalSize, PhysicalSize, Size},
    prelude::*,
};
use novonotes_run_loop::{RunLoop, RunLoopGuard};
use parking_lot::{Mutex, RwLock};
use rippr_core::{
    LibraryEntry, PlaybackEngine, PreparedSample, RipRequest, RipprSession, TrimRange, WorkerEvent,
    WorkerProcessAcquisition,
};
use rtrb::{Consumer, Producer, PushError, RingBuffer};
use serde::{Deserialize, Serialize};
use serde_json::json;
use wxp::{
    Channel, Rect, WebContext, WebViewDispatch, WxpCommandHandler, WxpWebView, WxpWebViewBuilder,
};

const UI_HTML: &str = include_str!("../../../ui/dist/index.html");
const EDITOR_WIDTH: u32 = 960;
const EDITOR_HEIGHT: u32 = 660;

enum UiToAudio {
    Activate(PreparedSample),
    Trigger,
}

#[derive(Params)]
struct RipprParams {
    #[id = "gain"]
    gain: FloatParam,
    #[persist = "active-sample-id"]
    active_sample_id: Arc<RwLock<Option<String>>>,
}

impl Default for RipprParams {
    fn default() -> Self {
        Self {
            gain: FloatParam::new(
                "Output Gain",
                util::db_to_gain(0.0),
                FloatRange::Skewed {
                    min: util::db_to_gain(-60.0),
                    max: util::db_to_gain(12.0),
                    factor: FloatRange::gain_skew_factor(-60.0, 12.0),
                },
            )
            .with_smoother(SmoothingStyle::Logarithmic(10.0))
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_gain_to_db(2))
            .with_string_to_value(formatters::s2v_f32_gain_to_db()),
            active_sample_id: Arc::new(RwLock::new(None)),
        }
    }
}

pub struct RipprPlugin {
    params: Arc<RipprParams>,
    playback: PlaybackEngine,
    ui_consumer: Consumer<UiToAudio>,
    ui_producer: Arc<Mutex<Producer<UiToAudio>>>,
    reclaim_producer: Producer<PreparedSample>,
    reclaimer_running: Arc<AtomicBool>,
    reclaimer_thread: Option<JoinHandle<()>>,
    paths: RipprPaths,
    active_media_path: Arc<Mutex<Option<PathBuf>>>,
    job_running: Arc<AtomicBool>,
    host_sample_rate: Arc<AtomicU32>,
    restore_running: Arc<AtomicBool>,
}

impl Default for RipprPlugin {
    fn default() -> Self {
        let (ui_producer, ui_consumer) = RingBuffer::new(8);
        let (reclaim_producer, mut reclaim_consumer) = RingBuffer::new(16);
        let reclaimer_running = Arc::new(AtomicBool::new(true));
        let reclaimer_running_for_thread = Arc::clone(&reclaimer_running);
        let reclaimer_thread = std::thread::Builder::new()
            .name("rippr-sample-reclaimer".into())
            .spawn(move || {
                while reclaimer_running_for_thread.load(Ordering::Acquire)
                    || !reclaim_consumer.is_empty()
                {
                    if reclaim_consumer.pop().is_err() {
                        std::thread::park_timeout(std::time::Duration::from_millis(10));
                    }
                }
            })
            .expect("could not start sample reclaimer");

        Self {
            params: Arc::new(RipprParams::default()),
            playback: PlaybackEngine::new(),
            ui_consumer,
            ui_producer: Arc::new(Mutex::new(ui_producer)),
            reclaim_producer,
            reclaimer_running,
            reclaimer_thread: Some(reclaimer_thread),
            paths: RipprPaths::discover(),
            active_media_path: Arc::new(Mutex::new(None)),
            job_running: Arc::new(AtomicBool::new(false)),
            host_sample_rate: Arc::new(AtomicU32::new(48_000)),
            restore_running: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl Drop for RipprPlugin {
    fn drop(&mut self) {
        self.reclaimer_running.store(false, Ordering::Release);
        if let Some(thread) = self.reclaimer_thread.take() {
            thread.thread().unpark();
            let _ = thread.join();
        }
    }
}

impl Plugin for RipprPlugin {
    const NAME: &'static str = "rippr-vst";
    const VENDOR: &'static str = "Iheanyi Ekechukwu";
    const URL: &'static str = "https://github.com/iheanyi/rippr-vst";
    const EMAIL: &'static str = "iheanyi@users.noreply.github.com";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: None,
        main_output_channels: NonZeroU32::new(2),
        ..AudioIOLayout::const_default()
    }];
    const MIDI_INPUT: MidiConfig = MidiConfig::Basic;
    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        let sample_rate = buffer_config.sample_rate.round().max(1.0) as u32;
        self.host_sample_rate.store(sample_rate, Ordering::Release);
        self.restore_active_sample(sample_rate);
        true
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        Some(Box::new(RipprEditor {
            ui_producer: Arc::clone(&self.ui_producer),
            paths: self.paths.clone(),
            active_media_path: Arc::clone(&self.active_media_path),
            job_running: Arc::clone(&self.job_running),
            host_sample_rate: Arc::clone(&self.host_sample_rate),
            active_sample_id: Arc::clone(&self.params.active_sample_id),
            size: Arc::new(AtomicU64::new(pack_size(EDITOR_WIDTH, EDITOR_HEIGHT))),
            webview_dispatch: Arc::new(Mutex::new(None)),
        }))
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        while let Ok(command) = self.ui_consumer.pop() {
            match command {
                UiToAudio::Activate(sample) => {
                    if let Some(previous) = self.playback.replace(sample) {
                        if let Err(PushError::Full(previous)) = self.reclaim_producer.push(previous)
                        {
                            // Avoid freeing a large allocation in the real-time callback.
                            std::mem::forget(previous);
                        }
                    }
                }
                UiToAudio::Trigger => self.playback.trigger_now(),
            }
        }

        let mut next_event = context.next_event();
        for (sample_index, mut channels) in buffer.iter_samples().enumerate() {
            while let Some(event) = next_event {
                if event.timing() > sample_index as u32 {
                    break;
                }
                if matches!(event, NoteEvent::NoteOn { velocity, .. } if velocity > 0.0) {
                    self.playback.trigger_now();
                }
                next_event = context.next_event();
            }

            let frame = self.playback.render_frame(self.params.gain.smoothed.next());
            if let Some(left) = channels.get_mut(0) {
                *left = frame[0];
            }
            if let Some(right) = channels.get_mut(1) {
                *right = frame[1];
            }
        }

        ProcessStatus::KeepAlive
    }
}

impl RipprPlugin {
    fn restore_active_sample(&self, sample_rate: u32) {
        let Some(id) = self.params.active_sample_id.read().clone() else {
            return;
        };
        if self
            .restore_running
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }

        let producer = Arc::clone(&self.ui_producer);
        let active_path = Arc::clone(&self.active_media_path);
        let running = Arc::clone(&self.restore_running);
        let paths = self.paths.clone();
        let _ = std::thread::Builder::new()
            .name("rippr-cache-restore".into())
            .spawn(move || {
                let outcome = RipprSession::open_cache(
                    paths.data.join("library.sqlite3"),
                    paths.data.join("media"),
                    sample_rate,
                )
                .and_then(|session| session.load_entry(&id));
                if let Ok(Some(outcome)) = outcome {
                    *active_path.lock() = Some(outcome.entry.media_path.clone());
                    let _ = producer.lock().push(UiToAudio::Activate(outcome.sample));
                }
                running.store(false, Ordering::Release);
            });
    }
}

impl Vst3Plugin for RipprPlugin {
    const VST3_CLASS_ID: [u8; 16] = *b"RipprVstSample01";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[
        Vst3SubCategory::Instrument,
        Vst3SubCategory::Sampler,
        Vst3SubCategory::Stereo,
    ];
}

#[derive(Clone)]
struct RipprPaths {
    worker: PathBuf,
    yt_dlp: PathBuf,
    ffmpeg: PathBuf,
    data: PathBuf,
}

impl RipprPaths {
    fn discover() -> Self {
        let resources = process_path::get_dylib_path()
            .and_then(|path| path.parent()?.parent().map(Path::to_path_buf))
            .map(|contents| contents.join("Resources"));
        let resource = |environment: &str, name: &str| {
            std::env::var_os(environment)
                .map(PathBuf::from)
                .or_else(|| resources.as_ref().map(|path| path.join(name)))
                .unwrap_or_else(|| PathBuf::from(name))
        };
        let data = ProjectDirs::from("com", "Iheanyi Ekechukwu", "rippr-vst")
            .map(|project| project.data_local_dir().to_path_buf())
            .unwrap_or_else(|| std::env::temp_dir().join("rippr-vst"));
        Self {
            worker: resource("RIPPR_WORKER", executable_name("rippr-worker")),
            yt_dlp: resource("RIPPR_YT_DLP", executable_name("yt-dlp")),
            ffmpeg: resource("RIPPR_FFMPEG", executable_name("ffmpeg")),
            data,
        }
    }
}

const fn executable_name(name: &'static str) -> &'static str {
    #[cfg(target_os = "windows")]
    {
        match name {
            "rippr-worker" => "rippr-worker.exe",
            "yt-dlp" => "yt-dlp.exe",
            "ffmpeg" => "ffmpeg.exe",
            _ => name,
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        name
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UiRipRequest {
    source_url: String,
    start_seconds: f64,
    end_seconds: f64,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum UiEvent {
    Accepted,
    Progress {
        stage: String,
        fraction: Option<f32>,
    },
    Metadata {
        title: String,
        creator: Option<String>,
        #[serde(rename = "durationSeconds")]
        duration_seconds: Option<f64>,
    },
    Ready {
        entry: UiLibraryEntry,
    },
    Failed {
        message: String,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UiLibraryEntry {
    id: String,
    title: String,
    creator: Option<String>,
    source_url: String,
    duration_seconds: f64,
    media_path: PathBuf,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UiBootstrap {
    sample_rate: u32,
    entries: Vec<UiLibraryEntry>,
    active_entry: Option<UiLibraryEntry>,
}

impl From<&LibraryEntry> for UiLibraryEntry {
    fn from(entry: &LibraryEntry) -> Self {
        Self {
            id: entry.id.clone(),
            title: entry.title.clone(),
            creator: entry.creator.clone(),
            source_url: entry.source_url.clone(),
            duration_seconds: entry.frame_count as f64 / f64::from(entry.rendered_sample_rate),
            media_path: entry.media_path.clone(),
        }
    }
}

struct RipprEditor {
    ui_producer: Arc<Mutex<Producer<UiToAudio>>>,
    paths: RipprPaths,
    active_media_path: Arc<Mutex<Option<PathBuf>>>,
    job_running: Arc<AtomicBool>,
    host_sample_rate: Arc<AtomicU32>,
    active_sample_id: Arc<RwLock<Option<String>>>,
    size: Arc<AtomicU64>,
    webview_dispatch: Arc<Mutex<Option<WebViewDispatch>>>,
}

struct EditorResources {
    _webview: WxpWebView,
    _web_context: WebContext,
    _run_loop_guard: RunLoopGuard,
    webview_dispatch: Arc<Mutex<Option<WebViewDispatch>>>,
}

impl Drop for EditorResources {
    fn drop(&mut self) {
        *self.webview_dispatch.lock() = None;
    }
}

impl Editor for RipprEditor {
    fn spawn(&self, parent: ParentWindowHandle, _context: Arc<dyn GuiContext>) -> Box<dyn Any> {
        let run_loop_guard = RunLoop::init().expect("could not initialize editor run loop");
        let handler = Rc::new(WxpCommandHandler::new());
        register_commands(
            &handler,
            Arc::clone(&self.ui_producer),
            self.paths.clone(),
            Arc::clone(&self.active_media_path),
            Arc::clone(&self.job_running),
            Arc::clone(&self.host_sample_rate),
            Arc::clone(&self.active_sample_id),
        );
        let mut web_context = WebContext::new(self.paths.data.join("webview"));
        let (width, height) = unpack_size(self.size.load(Ordering::Acquire));
        let webview = WxpWebViewBuilder::new(&mut web_context)
            .with_command_handler(handler)
            .with_html(UI_HTML)
            .with_devtools(cfg!(debug_assertions))
            .with_bounds(Rect {
                position: wxp::dpi::LogicalPosition::new(0.0, 0.0).into(),
                size: wxp::dpi::LogicalSize::new(f64::from(width), f64::from(height)).into(),
            })
            .build_as_child(&parent)
            .expect("could not create rippr-vst WebView");
        *self.webview_dispatch.lock() = Some(webview.dispatch());
        Box::new(EditorResources {
            _webview: webview,
            _web_context: web_context,
            _run_loop_guard: run_loop_guard,
            webview_dispatch: Arc::clone(&self.webview_dispatch),
        })
    }

    fn size(&self) -> Size {
        let (width, height) = unpack_size(self.size.load(Ordering::Acquire));
        Size::Logical(LogicalSize::new(f64::from(width), f64::from(height)))
    }

    fn param_value_changed(&self, _id: &str, _normalized_value: f32) {}
    fn param_modulation_changed(&self, _id: &str, _modulation_offset: f32) {}
    fn param_values_changed(&self) {}

    fn set_scale_factor(&self, _factor: f64) -> bool {
        true
    }

    fn set_size(&self, size: PhysicalSize<u32>) -> bool {
        if size.width < 640 || size.height < 480 {
            return false;
        }
        self.size
            .store(pack_size(size.width, size.height), Ordering::Release);
        if let Some(dispatch) = self.webview_dispatch.lock().as_ref() {
            let _ = dispatch.post_set_bounds(Rect {
                position: wxp::dpi::LogicalPosition::new(0.0, 0.0).into(),
                size: wxp::dpi::Size::Physical(size),
            });
        }
        true
    }

    fn resize_hint(&self) -> ResizeHint {
        ResizeHint::resizable()
    }
}

const fn pack_size(width: u32, height: u32) -> u64 {
    ((width as u64) << 32) | height as u64
}

const fn unpack_size(size: u64) -> (u32, u32) {
    ((size >> 32) as u32, size as u32)
}

fn register_commands(
    handler: &WxpCommandHandler,
    ui_producer: Arc<Mutex<Producer<UiToAudio>>>,
    paths: RipprPaths,
    active_media_path: Arc<Mutex<Option<PathBuf>>>,
    job_running: Arc<AtomicBool>,
    host_sample_rate: Arc<AtomicU32>,
    active_sample_id: Arc<RwLock<Option<String>>>,
) {
    let paths_for_bootstrap = paths.clone();
    let sample_rate_for_bootstrap = Arc::clone(&host_sample_rate);
    let active_id_for_bootstrap = Arc::clone(&active_sample_id);
    handler.register_sync("bootstrap", move |_context| {
        let sample_rate = sample_rate_for_bootstrap.load(Ordering::Acquire);
        let session = RipprSession::open_cache(
            paths_for_bootstrap.data.join("library.sqlite3"),
            paths_for_bootstrap.data.join("media"),
            sample_rate,
        )
        .map_err(|error| error.to_string())?;
        let active_id = active_id_for_bootstrap.read().clone();
        let mut active_entry = None;
        let entries = session
            .library_entries()
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|entry| {
                let ui_entry = UiLibraryEntry::from(&entry);
                if active_id.as_deref() == Some(entry.id.as_str()) && entry.media_path.is_file() {
                    active_entry = Some(UiLibraryEntry::from(&entry));
                }
                ui_entry
            })
            .collect();
        Ok::<_, String>(UiBootstrap {
            sample_rate,
            entries,
            active_entry,
        })
    });

    let producer_for_activation = Arc::clone(&ui_producer);
    let active_path_for_activation = Arc::clone(&active_media_path);
    let job_running_for_activation = Arc::clone(&job_running);
    let sample_rate_for_activation = Arc::clone(&host_sample_rate);
    let active_id_for_activation = Arc::clone(&active_sample_id);
    let paths_for_activation = paths.clone();
    handler.register_sync("activate_library_entry", move |context| {
        let id = context
            .arg::<String>("id")
            .map_err(|error| error.to_string())?;
        let channel = context
            .arg::<Channel>("channel")
            .map_err(|error| error.to_string())?;
        if job_running_for_activation
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err("A rip or cache load is already running.".to_string());
        }

        let producer = Arc::clone(&producer_for_activation);
        let active_path = Arc::clone(&active_path_for_activation);
        let running = Arc::clone(&job_running_for_activation);
        let active_id = Arc::clone(&active_id_for_activation);
        let paths = paths_for_activation.clone();
        let sample_rate = sample_rate_for_activation.load(Ordering::Acquire);
        std::thread::Builder::new()
            .name("rippr-cache-load".into())
            .spawn(move || {
                let result = RipprSession::open_cache(
                    paths.data.join("library.sqlite3"),
                    paths.data.join("media"),
                    sample_rate,
                )
                .and_then(|session| session.load_entry(&id));
                match result {
                    Ok(Some(outcome)) => {
                        *active_path.lock() = Some(outcome.entry.media_path.clone());
                        if producer
                            .lock()
                            .push(UiToAudio::Activate(outcome.sample))
                            .is_ok()
                        {
                            *active_id.write() = Some(outcome.entry.id.clone());
                            let _ = channel.send(UiEvent::Ready {
                                entry: UiLibraryEntry::from(&outcome.entry),
                            });
                        } else {
                            let _ = channel.send(UiEvent::Failed {
                                message: "The audio handoff queue is busy. Try again.".into(),
                            });
                        }
                    }
                    Ok(None) => {
                        let _ = channel.send(UiEvent::Failed {
                            message: "The cached sample is missing. Reacquire it explicitly."
                                .into(),
                        });
                    }
                    Err(error) => {
                        let _ = channel.send(UiEvent::Failed {
                            message: error.to_string(),
                        });
                    }
                }
                running.store(false, Ordering::Release);
            })
            .map_err(|error| {
                job_running_for_activation.store(false, Ordering::Release);
                error.to_string()
            })?;
        Ok(json!({ "accepted": true }))
    });

    let producer_for_start = Arc::clone(&ui_producer);
    let active_path_for_start = Arc::clone(&active_media_path);
    let job_running_for_start = Arc::clone(&job_running);
    let sample_rate_for_start = Arc::clone(&host_sample_rate);
    let active_id_for_start = Arc::clone(&active_sample_id);
    handler.register_sync("start_rip", move |context| {
        let request = context
            .arg::<UiRipRequest>("request")
            .map_err(|error| error.to_string())?;
        let channel = context
            .arg::<Channel>("channel")
            .map_err(|error| error.to_string())?;
        let trim = TrimRange::new(request.start_seconds, request.end_seconds)
            .map_err(|error| error.to_string())?;
        let request =
            RipRequest::new(request.source_url, trim).map_err(|error| error.to_string())?;
        if job_running_for_start
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err("A rip job is already running.".to_string());
        }

        let producer = Arc::clone(&producer_for_start);
        let active_path = Arc::clone(&active_path_for_start);
        let running = Arc::clone(&job_running_for_start);
        let active_id = Arc::clone(&active_id_for_start);
        let sample_rate = sample_rate_for_start.load(Ordering::Acquire);
        let paths = paths.clone();
        std::thread::Builder::new()
            .name("rippr-acquisition".into())
            .spawn(move || {
                let acquisition = Arc::new(WorkerProcessAcquisition::new(
                    paths.worker,
                    paths.yt_dlp,
                    paths.ffmpeg,
                    paths.data.join("worker"),
                ));
                let result = RipprSession::open(
                    acquisition,
                    paths.data.join("library.sqlite3"),
                    paths.data.join("media"),
                    sample_rate,
                )
                .and_then(|session| {
                    session.rip(request, |event| {
                        if let Some(event) = ui_event(event) {
                            let _ = channel.send(event);
                        }
                    })
                });
                match result {
                    Ok(outcome) => {
                        *active_path.lock() = Some(outcome.entry.media_path.clone());
                        if producer
                            .lock()
                            .push(UiToAudio::Activate(outcome.sample))
                            .is_ok()
                        {
                            *active_id.write() = Some(outcome.entry.id.clone());
                            let _ = channel.send(UiEvent::Ready {
                                entry: UiLibraryEntry::from(&outcome.entry),
                            });
                        } else {
                            let _ = channel.send(UiEvent::Failed {
                                message: "The audio handoff queue is busy. Try again.".into(),
                            });
                        }
                    }
                    Err(error) => {
                        let _ = channel.send(UiEvent::Failed {
                            message: error.to_string(),
                        });
                    }
                }
                running.store(false, Ordering::Release);
            })
            .map_err(|error| {
                job_running_for_start.store(false, Ordering::Release);
                error.to_string()
            })?;

        Ok(json!({ "accepted": true }))
    });

    let producer_for_preview = Arc::clone(&ui_producer);
    handler.register_sync("preview", move |_context| {
        producer_for_preview
            .lock()
            .push(UiToAudio::Trigger)
            .map_err(|_| "The preview queue is busy.".to_string())?;
        Ok::<_, String>(json!({ "triggered": true }))
    });

    handler.register_sync("reveal_active_sample", move |_context| {
        let path = active_media_path
            .lock()
            .clone()
            .ok_or_else(|| "No active sample is available.".to_string())?;
        reveal_file(&path).map_err(|error| error.to_string())?;
        Ok::<_, String>(json!({ "revealed": true }))
    });
}

fn ui_event(event: WorkerEvent) -> Option<UiEvent> {
    match event {
        WorkerEvent::Accepted { .. } => Some(UiEvent::Accepted),
        WorkerEvent::Metadata {
            title,
            creator,
            duration_seconds,
            ..
        } => Some(UiEvent::Metadata {
            title,
            creator,
            duration_seconds,
        }),
        WorkerEvent::Progress {
            stage, fraction, ..
        } => Some(UiEvent::Progress { stage, fraction }),
        WorkerEvent::Failed { message, .. } => Some(UiEvent::Failed { message }),
        WorkerEvent::Cancelled { .. } => Some(UiEvent::Failed {
            message: "The rip was cancelled.".into(),
        }),
        WorkerEvent::Prepared { .. } => None,
    }
}

fn reveal_file(path: &Path) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = std::process::Command::new("open");
        command.arg("-R").arg(path);
        command
    };
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = std::process::Command::new("explorer.exe");
        command.arg(format!("/select,{}", path.display()));
        command
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut command = {
        let mut command = std::process::Command::new("xdg-open");
        command.arg(path.parent().unwrap_or(path));
        command
    };
    command.spawn().map(|_| ())
}

nice_export_vst3!(RipprPlugin);
