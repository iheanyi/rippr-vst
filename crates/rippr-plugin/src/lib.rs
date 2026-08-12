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
    LibraryEntry, PlaybackEngine, PreparedSample, RipRequest, RipprSession, WorkerEvent,
    WorkerProcessAcquisition,
};
use rtrb::{Consumer, Producer, RingBuffer};
use serde::{Deserialize, Serialize};
use serde_json::json;
use wxp::{
    Channel, Rect, WebContext, WebViewDispatch, WxpCommandHandler, WxpWebView, WxpWebViewBuilder,
};

mod native_directory;
mod native_drag;
mod native_edit_shortcuts;

use native_directory::choose_sample_directory;
use native_drag::NativeDragContext;
use native_edit_shortcuts::NativeEditShortcuts;

const UI_HTML: &str = include_str!("../../../ui/dist/index.html");
const EDITOR_WIDTH: u32 = 960;
const EDITOR_HEIGHT: u32 = 660;

enum UiToAudio {
    Activate(PreparedSample),
    Trigger,
    Stop,
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
    sample_directory: Arc<RwLock<PathBuf>>,
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

        let paths = RipprPaths::discover();
        let sample_directory =
            load_sample_directory(&paths.data).unwrap_or_else(|| paths.data.join("handoff"));
        Self {
            params: Arc::new(RipprParams::default()),
            playback: PlaybackEngine::new(),
            ui_consumer,
            ui_producer: Arc::new(Mutex::new(ui_producer)),
            reclaim_producer,
            reclaimer_running,
            reclaimer_thread: Some(reclaimer_thread),
            paths,
            sample_directory: Arc::new(RwLock::new(sample_directory)),
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
            sample_directory: Arc::clone(&self.sample_directory),
            active_media_path: Arc::clone(&self.active_media_path),
            job_running: Arc::clone(&self.job_running),
            host_sample_rate: Arc::clone(&self.host_sample_rate),
            active_sample_id: Arc::clone(&self.params.active_sample_id),
            size: Arc::new(AtomicU64::new(pack_size(EDITOR_WIDTH, EDITOR_HEIGHT))),
            scale_factor: Arc::new(AtomicU64::new(1.0_f64.to_bits())),
            webview_dispatch: Arc::new(Mutex::new(None)),
        }))
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        // Do not consume an activation unless the previous allocation can be
        // handed off for background reclamation. This may defer UI commands by
        // a block, but it guarantees the callback never frees or leaks sample
        // memory even if the reclaimer thread is temporarily descheduled.
        while !self.reclaim_producer.is_full() {
            let Ok(command) = self.ui_consumer.pop() else {
                break;
            };
            match command {
                UiToAudio::Activate(sample) => {
                    if let Some(previous) = self.playback.replace(sample) {
                        self.reclaim_producer
                            .push(previous)
                            .expect("reclamation capacity was checked before activation");
                    }
                }
                UiToAudio::Trigger => self.playback.trigger_now(),
                UiToAudio::Stop => self.playback.stop(),
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
        let sample_directory = Arc::clone(&self.sample_directory);
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
                    if let Ok(handoff_path) = prepare_handoff_file(
                        &sample_directory.read(),
                        &outcome.entry.media_path,
                        &outcome.entry.id,
                        &outcome.entry.title,
                    ) {
                        let preview_ready = outcome.sample.is_none_or(|sample| {
                            producer.lock().push(UiToAudio::Activate(sample)).is_ok()
                        });
                        if preview_ready {
                            *active_path.lock() = Some(handoff_path);
                        }
                    }
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

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct UserPreferences {
    sample_directory: Option<PathBuf>,
}

fn load_sample_directory(data_directory: &Path) -> Option<PathBuf> {
    let bytes = std::fs::read(data_directory.join("preferences.json")).ok()?;
    serde_json::from_slice::<UserPreferences>(&bytes)
        .ok()?
        .sample_directory
}

fn save_sample_directory(data_directory: &Path, sample_directory: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(data_directory)?;
    let destination = data_directory.join("preferences.json");
    let temporary = data_directory.join("preferences.json.partial");
    let bytes = serde_json::to_vec_pretty(&UserPreferences {
        sample_directory: Some(sample_directory.to_path_buf()),
    })?;
    std::fs::write(&temporary, bytes)?;
    std::fs::rename(temporary, destination)
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
    waveform_peaks: Vec<[f32; 2]>,
    preview_available: bool,
    media_path: PathBuf,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UiBootstrap {
    sample_rate: u32,
    sample_directory: PathBuf,
    entries: Vec<UiLibraryEntry>,
    active_entry: Option<UiLibraryEntry>,
}

impl UiLibraryEntry {
    fn from_entry(entry: &LibraryEntry, preview_available: bool) -> Self {
        Self {
            id: entry.id.clone(),
            title: entry.title.clone(),
            creator: entry.creator.clone(),
            source_url: entry.source_url.clone(),
            duration_seconds: entry.frame_count as f64 / f64::from(entry.rendered_sample_rate),
            waveform_peaks: entry.waveform_peaks.clone(),
            preview_available,
            media_path: entry.media_path.clone(),
        }
    }
}

struct RipprEditor {
    ui_producer: Arc<Mutex<Producer<UiToAudio>>>,
    paths: RipprPaths,
    sample_directory: Arc<RwLock<PathBuf>>,
    active_media_path: Arc<Mutex<Option<PathBuf>>>,
    job_running: Arc<AtomicBool>,
    host_sample_rate: Arc<AtomicU32>,
    active_sample_id: Arc<RwLock<Option<String>>>,
    size: Arc<AtomicU64>,
    scale_factor: Arc<AtomicU64>,
    webview_dispatch: Arc<Mutex<Option<WebViewDispatch>>>,
}

struct EditorResources {
    _webview: WxpWebView,
    _web_context: WebContext,
    _run_loop_guard: RunLoopGuard,
    _edit_shortcuts: NativeEditShortcuts,
    webview_dispatch: Arc<Mutex<Option<WebViewDispatch>>>,
}

struct EditorCommandState {
    ui_producer: Arc<Mutex<Producer<UiToAudio>>>,
    paths: RipprPaths,
    sample_directory: Arc<RwLock<PathBuf>>,
    active_media_path: Arc<Mutex<Option<PathBuf>>>,
    job_running: Arc<AtomicBool>,
    host_sample_rate: Arc<AtomicU32>,
    active_sample_id: Arc<RwLock<Option<String>>>,
    native_drag: NativeDragContext,
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
        let native_drag = NativeDragContext::new(parent);
        let edit_shortcuts = NativeEditShortcuts::new(parent);
        register_commands(
            &handler,
            EditorCommandState {
                ui_producer: Arc::clone(&self.ui_producer),
                paths: self.paths.clone(),
                sample_directory: Arc::clone(&self.sample_directory),
                active_media_path: Arc::clone(&self.active_media_path),
                job_running: Arc::clone(&self.job_running),
                host_sample_rate: Arc::clone(&self.host_sample_rate),
                active_sample_id: Arc::clone(&self.active_sample_id),
                native_drag,
            },
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
            _edit_shortcuts: edit_shortcuts,
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

    fn set_scale_factor(&self, factor: f64) -> bool {
        if !factor.is_finite() || factor <= 0.0 {
            return false;
        }
        self.scale_factor.store(factor.to_bits(), Ordering::Release);
        true
    }

    fn set_size(&self, size: PhysicalSize<u32>) -> bool {
        // VST3 reports physical dimensions. AppKit's view coordinates are
        // points (nice-plug leaves the macOS scale at 1), while Windows may
        // provide an explicit display scale. Keep our canonical size logical,
        // and let Wry convert those logical bounds exactly once.
        let scale_factor = f64::from_bits(self.scale_factor.load(Ordering::Acquire));
        let Some((width, height)) = logical_editor_size(size.width, size.height, scale_factor)
        else {
            return false;
        };
        if width < 640 || height < 480 {
            return false;
        }
        self.size.store(pack_size(width, height), Ordering::Release);
        if let Some(dispatch) = self.webview_dispatch.lock().as_ref() {
            let _ = dispatch.post_set_bounds(Rect {
                position: wxp::dpi::LogicalPosition::new(0.0, 0.0).into(),
                size: wxp::dpi::LogicalSize::new(f64::from(width), f64::from(height)).into(),
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

fn logical_editor_size(width: u32, height: u32, scale_factor: f64) -> Option<(u32, u32)> {
    if !scale_factor.is_finite() || scale_factor <= 0.0 {
        return None;
    }
    Some((
        (f64::from(width) / scale_factor).round() as u32,
        (f64::from(height) / scale_factor).round() as u32,
    ))
}

fn register_commands(handler: &WxpCommandHandler, state: EditorCommandState) {
    let EditorCommandState {
        ui_producer,
        paths,
        sample_directory,
        active_media_path,
        job_running,
        host_sample_rate,
        active_sample_id,
        native_drag,
    } = state;
    let paths_for_bootstrap = paths.clone();
    let sample_rate_for_bootstrap = Arc::clone(&host_sample_rate);
    let active_id_for_bootstrap = Arc::clone(&active_sample_id);
    let sample_directory_for_bootstrap = Arc::clone(&sample_directory);
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
                let preview_available = PreparedSample::is_previewable(&entry.media_path);
                let ui_entry = UiLibraryEntry::from_entry(&entry, preview_available);
                if active_id.as_deref() == Some(entry.id.as_str()) && entry.media_path.is_file() {
                    active_entry = Some(UiLibraryEntry::from_entry(&entry, preview_available));
                }
                ui_entry
            })
            .collect();
        Ok::<_, String>(UiBootstrap {
            sample_rate,
            sample_directory: sample_directory_for_bootstrap.read().clone(),
            entries,
            active_entry,
        })
    });

    let producer_for_activation = Arc::clone(&ui_producer);
    let active_path_for_activation = Arc::clone(&active_media_path);
    let job_running_for_activation = Arc::clone(&job_running);
    let sample_rate_for_activation = Arc::clone(&host_sample_rate);
    let active_id_for_activation = Arc::clone(&active_sample_id);
    let sample_directory_for_activation = Arc::clone(&sample_directory);
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
        let sample_directory = Arc::clone(&sample_directory_for_activation);
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
                        let handoff_path = match prepare_handoff_file(
                            &sample_directory.read(),
                            &outcome.entry.media_path,
                            &outcome.entry.id,
                            &outcome.entry.title,
                        ) {
                            Ok(path) => path,
                            Err(error) => {
                                let _ = channel.send(UiEvent::Failed {
                                    message: format!(
                                        "Could not prepare the WAV for DAW handoff: {error}"
                                    ),
                                });
                                running.store(false, Ordering::Release);
                                return;
                            }
                        };
                        let preview_available = outcome.sample.is_some();
                        let preview_ready = outcome.sample.is_none_or(|sample| {
                            producer.lock().push(UiToAudio::Activate(sample)).is_ok()
                        });
                        if preview_ready {
                            *active_path.lock() = Some(handoff_path.clone());
                            *active_id.write() = Some(outcome.entry.id.clone());
                            let mut entry =
                                UiLibraryEntry::from_entry(&outcome.entry, preview_available);
                            entry.media_path = handoff_path;
                            let _ = channel.send(UiEvent::Ready { entry });
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
    let sample_directory_for_start = Arc::clone(&sample_directory);
    let paths_for_start = paths.clone();
    handler.register_sync("start_rip", move |context| {
        let request = context
            .arg::<UiRipRequest>("request")
            .map_err(|error| error.to_string())?;
        let channel = context
            .arg::<Channel>("channel")
            .map_err(|error| error.to_string())?;
        let request = RipRequest::new(request.source_url).map_err(|error| error.to_string())?;
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
        let sample_directory = Arc::clone(&sample_directory_for_start);
        let sample_rate = sample_rate_for_start.load(Ordering::Acquire);
        let paths = paths_for_start.clone();
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
                        let handoff_path = match prepare_handoff_file(
                            &sample_directory.read(),
                            &outcome.entry.media_path,
                            &outcome.entry.id,
                            &outcome.entry.title,
                        ) {
                            Ok(path) => path,
                            Err(error) => {
                                let _ = channel.send(UiEvent::Failed {
                                    message: format!(
                                        "Could not prepare the WAV for DAW handoff: {error}"
                                    ),
                                });
                                running.store(false, Ordering::Release);
                                return;
                            }
                        };
                        let preview_available = outcome.sample.is_some();
                        let preview_ready = outcome.sample.is_none_or(|sample| {
                            producer.lock().push(UiToAudio::Activate(sample)).is_ok()
                        });
                        if preview_ready {
                            *active_path.lock() = Some(handoff_path.clone());
                            *active_id.write() = Some(outcome.entry.id.clone());
                            let mut entry =
                                UiLibraryEntry::from_entry(&outcome.entry, preview_available);
                            entry.media_path = handoff_path;
                            let _ = channel.send(UiEvent::Ready { entry });
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

    let sample_directory_for_choose = Arc::clone(&sample_directory);
    let data_directory_for_choose = paths.data.clone();
    handler.register_sync("choose_sample_directory", move |_context| {
        let Some(directory) = choose_sample_directory()? else {
            return Ok::<_, String>(json!({ "cancelled": true }));
        };
        std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
        save_sample_directory(&data_directory_for_choose, &directory)
            .map_err(|error| error.to_string())?;
        *sample_directory_for_choose.write() = directory.clone();
        Ok(json!({ "path": directory }))
    });

    let producer_for_preview = Arc::clone(&ui_producer);
    handler.register_sync("preview", move |_context| {
        producer_for_preview
            .lock()
            .push(UiToAudio::Trigger)
            .map_err(|_| "The preview queue is busy.".to_string())?;
        Ok::<_, String>(json!({ "triggered": true }))
    });

    let producer_for_stop = Arc::clone(&ui_producer);
    handler.register_sync("stop_preview", move |_context| {
        producer_for_stop
            .lock()
            .push(UiToAudio::Stop)
            .map_err(|_| "The preview queue is busy.".to_string())?;
        Ok::<_, String>(json!({ "stopped": true }))
    });

    let active_path_for_reveal = Arc::clone(&active_media_path);
    handler.register_sync("reveal_active_sample", move |_context| {
        let path = active_path_for_reveal
            .lock()
            .clone()
            .ok_or_else(|| "No active sample is available.".to_string())?;
        reveal_file(&path).map_err(|error| error.to_string())?;
        Ok::<_, String>(json!({ "revealed": true }))
    });

    handler.register_sync("start_wav_drag", move |_context| {
        let path = active_media_path
            .lock()
            .clone()
            .ok_or_else(|| "No active sample is available.".to_string())?;
        native_drag.start(&path)?;
        Ok::<_, String>(json!({ "started": true }))
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

fn prepare_handoff_file(
    sample_directory: &Path,
    cache_path: &Path,
    entry_id: &str,
    title: &str,
) -> std::io::Result<PathBuf> {
    if !cache_path.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "the cached WAV is missing",
        ));
    }

    std::fs::create_dir_all(sample_directory)?;
    let stem = friendly_wav_stem(title);
    let mut destination = sample_directory.join(format!("{stem}.wav"));
    if destination.is_file()
        && std::fs::metadata(&destination)?.len() != std::fs::metadata(cache_path)?.len()
    {
        destination = sample_directory.join(format!(
            "{stem} - {}.wav",
            entry_id.chars().take(8).collect::<String>()
        ));
    }
    if !destination.is_file() {
        match std::fs::hard_link(cache_path, &destination) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => {
                std::fs::copy(cache_path, &destination)?;
            }
        }
    }
    Ok(destination)
}

fn friendly_wav_stem(title: &str) -> String {
    const MAX_CHARS: usize = 80;

    let mut stem = String::new();
    let mut last_was_space = false;
    for character in title.trim().chars().take(MAX_CHARS) {
        let character = match character {
            '/' | '\\' => '-',
            ':' | '?' | '*' | '"' | '<' | '>' | '|' => '-',
            character if character.is_control() => ' ',
            character => character,
        };
        if character.is_whitespace() {
            if !last_was_space && !stem.is_empty() {
                stem.push(' ');
            }
            last_was_space = true;
        } else {
            stem.push(character);
            last_was_space = false;
        }
    }

    let stem = stem.trim_matches([' ', '.']).to_owned();
    if stem.is_empty() {
        "Rippr Sample".into()
    } else {
        stem
    }
}

nice_export_vst3!(RipprPlugin);

#[cfg(test)]
mod tests {
    use std::fs;

    #[cfg(unix)]
    use std::os::unix::fs::MetadataExt;

    use super::{friendly_wav_stem, logical_editor_size, prepare_handoff_file};

    #[test]
    fn host_resize_is_converted_to_logical_units_once() {
        assert_eq!(logical_editor_size(1_200, 800, 1.0), Some((1_200, 800)));
        assert_eq!(logical_editor_size(1_800, 1_200, 1.5), Some((1_200, 800)));
        assert_eq!(logical_editor_size(1_200, 800, 0.0), None);
    }

    #[test]
    fn friendly_wav_names_are_safe_and_human_readable() {
        assert_eq!(
            friendly_wav_stem("  Kick / Snare: take 1  "),
            "Kick - Snare- take 1"
        );
        assert_eq!(friendly_wav_stem("..."), "Rippr Sample");
    }

    #[test]
    fn handoff_uses_a_friendly_hard_link_without_renaming_the_cache() {
        let root = tempfile::tempdir().unwrap();
        let cache = root.path().join("0123456789abcdef.wav");
        fs::write(&cache, b"RIFF fixture").unwrap();

        let handoff =
            prepare_handoff_file(root.path(), &cache, "0123456789abcdef", "Amen Break / 1969")
                .unwrap();

        assert_eq!(
            handoff.file_name().unwrap().to_string_lossy(),
            "Amen Break - 1969.wav"
        );
        assert_eq!(fs::read(&handoff).unwrap(), b"RIFF fixture");
        assert!(cache.exists());

        #[cfg(unix)]
        assert_eq!(
            fs::metadata(&cache).unwrap().ino(),
            fs::metadata(&handoff).unwrap().ino()
        );
    }
}
