import {
  ArrowPathIcon,
  Bars3Icon,
  CheckCircleIcon,
  ExclamationTriangleIcon,
  FolderOpenIcon,
  MagnifyingGlassIcon,
  MusicalNoteIcon,
  PlayIcon,
  StopIcon,
} from "@heroicons/react/16/solid";
import {
  FormEvent,
  PointerEvent,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import type { CSSProperties } from "react";

import type { LibraryEntry, RipEvent, RipprBridge } from "./bridge";

type AppProps = {
  bridge: RipprBridge;
};

type View = "acquire" | "library";
type Notice = { tone: "error" | "success"; message: string };

function formatTime(seconds: number): string {
  const rounded = Math.max(0, Math.round(seconds));
  const minutes = Math.floor(rounded / 60);
  const remaining = rounded - minutes * 60;
  return `${minutes}:${remaining.toString().padStart(2, "0")}`;
}

function formatSampleRate(sampleRate: number): string {
  return `${(sampleRate / 1_000).toLocaleString(undefined, {
    maximumFractionDigits: 1,
  })} kHz`;
}

function fileName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).at(-1) ?? path;
}

function Waveform({ sample }: { sample?: LibraryEntry }) {
  const peaks = sample?.waveformPeaks ?? [];
  if (!sample || peaks.length === 0) {
    return (
      <div
        className="flex h-40 items-center justify-center px-6 text-center"
        aria-label="Sample waveform overview"
      >
        <div className="flex max-w-[34ch] flex-col items-center gap-2">
          <MusicalNoteIcon className="size-4 shrink-0 fill-zinc-700" />
          <p className="text-pretty text-base text-zinc-500 sm:text-sm">
            Rip a source to generate its waveform.
          </p>
        </div>
      </div>
    );
  }

  const width = 1_000;
  const height = 160;
  const center = height / 2;
  const scale = Math.max(
    0.001,
    ...peaks.flatMap(([min, max]) => [Math.abs(min), Math.abs(max)]),
  );
  const x = (index: number) =>
    peaks.length === 1 ? width / 2 : (index / (peaks.length - 1)) * width;
  const top = peaks.map(([, max], index) =>
    `${x(index).toFixed(2)},${(center - (max / scale) * (center - 12)).toFixed(2)}`,
  );
  const bottom = [...peaks].reverse().map(([min], reverseIndex) => {
    const index = peaks.length - reverseIndex - 1;
    return `${x(index).toFixed(2)},${(center - (min / scale) * (center - 12)).toFixed(2)}`;
  });

  return (
    <div className="relative h-40 overflow-hidden bg-[linear-gradient(to_right,rgb(255_255_255/0.035)_1px,transparent_1px)] bg-[size:12.5%_100%]">
      <svg
        className="size-full"
        viewBox={`0 0 ${width} ${height}`}
        preserveAspectRatio="none"
        role="img"
        aria-label={`Waveform for ${sample.title}`}
      >
        <line
          x1="0"
          x2={width}
          y1={center}
          y2={center}
          className="stroke-white/10"
        />
        <polygon
          points={[...top, ...bottom].join(" ")}
          className="fill-lime-300/75"
        />
      </svg>
      <div className="pointer-events-none absolute inset-x-0 bottom-0 h-12 bg-linear-to-t from-[#171714] to-transparent" />
      <p className="absolute right-3 bottom-2 font-mono text-sm tabular-nums text-zinc-400">
        {formatTime(sample.durationSeconds)}
      </p>
    </div>
  );
}

export function App({ bridge }: AppProps) {
  const [view, setView] = useState<View>("acquire");
  const [sourceUrl, setSourceUrl] = useState("");
  const [stage, setStage] = useState("Waiting for a URL");
  const [progress, setProgress] = useState(0);
  const [metadata, setMetadata] = useState<{
    title: string;
    creator?: string;
    durationSeconds?: number;
  }>();
  const [activeSample, setActiveSample] = useState<LibraryEntry>();
  const [notice, setNotice] = useState<Notice>();
  const [busy, setBusy] = useState(false);
  const [library, setLibrary] = useState<LibraryEntry[]>([]);
  const [libraryQuery, setLibraryQuery] = useState("");
  const [sampleRate, setSampleRate] = useState(48_000);
  const [sampleDirectory, setSampleDirectory] = useState("");
  const [previewing, setPreviewing] = useState(false);
  const previewTimer = useRef<ReturnType<typeof setTimeout> | undefined>(
    undefined,
  );
  const folderHandoffVersion = useRef(0);

  const progressWidth = useMemo(
    () => `${Math.round(Math.min(1, Math.max(0, progress)) * 100)}%`,
    [progress],
  );
  const filteredLibrary = useMemo(() => {
    const query = libraryQuery.trim().toLocaleLowerCase();
    if (!query) return library;
    return library.filter((entry) =>
      [entry.title, entry.creator, entry.sourceUrl]
        .filter(Boolean)
        .some((value) => value!.toLocaleLowerCase().includes(query)),
    );
  }, [library, libraryQuery]);
  const visibleTitle =
    activeSample?.title ?? metadata?.title ?? "No sample loaded";
  const visibleCreator =
    activeSample?.creator ?? metadata?.creator ?? "Paste a public media URL to begin.";
  const statusTone = notice?.tone === "error"
    ? "error"
    : busy
      ? "busy"
      : activeSample
        ? "ready"
        : "idle";

  function stopPreviewState() {
    if (previewTimer.current) clearTimeout(previewTimer.current);
    previewTimer.current = undefined;
    setPreviewing(false);
  }

  function startWavDrag(event: PointerEvent<HTMLButtonElement>) {
    if (event.button !== 0 || !activeSample) return;
    event.preventDefault();
    setNotice(undefined);
    void bridge.startWavDrag().catch((reason) => {
      setNotice({
        tone: "error",
        message:
          reason instanceof Error
            ? reason.message
            : "The WAV drag could not be started.",
      });
    });
  }

  useEffect(() => {
    let mounted = true;
    void bridge
      .bootstrap()
      .then((state) => {
        if (!mounted) return;
        setSampleRate(state.sampleRate);
        setSampleDirectory(state.sampleDirectory);
        setLibrary(state.entries);
        if (state.activeEntry) {
          setActiveSample(state.activeEntry);
          setStage("Ready");
          setProgress(1);
        }
      })
      .catch((reason) => {
        if (mounted) {
          setNotice({
            tone: "error",
            message:
              reason instanceof Error
                ? reason.message
                : "The sample library is unavailable.",
          });
        }
      });
    return () => {
      mounted = false;
      if (previewTimer.current) clearTimeout(previewTimer.current);
    };
  }, [bridge]);

  function receiveEvent(event: RipEvent) {
    switch (event.type) {
      case "accepted":
        setStage("Starting worker");
        break;
      case "progress":
        setStage(event.stage.charAt(0).toUpperCase() + event.stage.slice(1));
        setProgress(event.fraction ?? 0.18);
        break;
      case "metadata":
        setMetadata(event);
        setStage("Preparing WAV");
        setProgress(0.8);
        break;
      case "ready":
        stopPreviewState();
        setActiveSample(event.entry);
        setLibrary((entries) => [
          event.entry,
          ...entries.filter((entry) => entry.id !== event.entry.id),
        ]);
        setStage("Ready");
        setProgress(1);
        setBusy(false);
        setNotice({ tone: "success", message: "WAV ready for your DAW." });
        break;
      case "handoff_ready":
        folderHandoffVersion.current += 1;
        setActiveSample(event.entry);
        setLibrary((entries) =>
          entries.map((entry) =>
            entry.id === event.entry.id ? event.entry : entry,
          ),
        );
        setNotice({
          tone: "success",
          message: "The active WAV is ready in the new folder.",
        });
        break;
      case "handoff_failed":
        folderHandoffVersion.current += 1;
        setNotice({ tone: "error", message: event.message });
        break;
      case "failed":
        setNotice({ tone: "error", message: event.message });
        setStage("Rip failed");
        setBusy(false);
        break;
    }
  }

  async function activateLibraryEntry(entry: LibraryEntry) {
    setNotice(undefined);
    setBusy(true);
    setStage("Loading cached sample");
    setProgress(0.4);
    try {
      await bridge.activateLibraryEntry(entry.id, receiveEvent);
    } catch (reason) {
      setNotice({
        tone: "error",
        message:
          reason instanceof Error
            ? reason.message
            : "The cached sample could not be loaded.",
      });
      setStage("Missing sample");
      setBusy(false);
    }
  }

  async function submitRip(event: FormEvent) {
    event.preventDefault();
    setNotice(undefined);
    setMetadata(undefined);
    setBusy(true);
    setProgress(0.04);
    setStage("Validating URL");
    try {
      await bridge.startRip({ sourceUrl }, receiveEvent);
    } catch (reason) {
      setNotice({
        tone: "error",
        message:
          reason instanceof Error ? reason.message : "The rip could not start.",
      });
      setStage("Rip failed");
      setBusy(false);
    }
  }

  async function chooseSampleFolder() {
    setNotice(undefined);
    const handoffVersion = folderHandoffVersion.current;
    try {
      const selection = await bridge.chooseSampleDirectory(receiveEvent);
      if (selection) {
        setSampleDirectory(selection.path);
        if (folderHandoffVersion.current === handoffVersion) {
          setNotice({
            tone: "success",
            message: selection.activeHandoffPending
              ? "Sample folder updated. Preparing the active WAV there…"
              : "Sample folder updated. New WAVs will save there.",
          });
        }
      }
    } catch (reason) {
      setNotice({
        tone: "error",
        message:
          reason instanceof Error
            ? reason.message
            : "The sample folder could not be changed.",
      });
    }
  }

  async function revealActiveSample() {
    setNotice(undefined);
    try {
      await bridge.revealActiveSample();
    } catch (reason) {
      setNotice({
        tone: "error",
        message:
          reason instanceof Error ? reason.message : "The WAV could not be revealed.",
      });
    }
  }

  async function togglePreview() {
    if (!activeSample?.previewAvailable) return;
    setNotice(undefined);
    try {
      if (previewing) {
        await bridge.stopPreview();
        stopPreviewState();
        return;
      }

      await bridge.preview();
      setPreviewing(true);
      if (previewTimer.current) clearTimeout(previewTimer.current);
      previewTimer.current = setTimeout(
        stopPreviewState,
        Math.max(0, activeSample.durationSeconds * 1_000),
      );
    } catch (reason) {
      setNotice({
        tone: "error",
        message:
          reason instanceof Error ? reason.message : "Preview playback failed.",
      });
      stopPreviewState();
    }
  }

  return (
    <main className="isolate min-h-dvh bg-[#10100e] text-zinc-100 antialiased scheme-only-dark">
      <header className="sticky top-0 z-10 border-b border-white/8 bg-[#141411]/95 backdrop-blur">
        <div className="flex min-h-13 items-center gap-4 px-4 sm:px-5">
          <div className="flex shrink-0 items-baseline gap-2" aria-label="rippr vst">
            <p className="font-mono text-base tracking-[-0.06em] text-lime-300">
              rippr/
            </p>
            <p className="font-mono text-base tracking-[-0.06em] text-zinc-600">
              vst
            </p>
          </div>
          <nav className="min-w-0 flex-1 overflow-x-auto" aria-label="Primary">
            <ul className="flex min-w-max items-center gap-1" role="list">
              {(["acquire", "library"] as const).map((item) => (
                <li key={item}>
                  <button
                    type="button"
                    aria-label={item === "acquire" ? "Acquire" : "Library"}
                    aria-current={view === item ? "page" : undefined}
                    onClick={() => setView(item)}
                    className={`rounded-md px-3 py-1.5 text-base focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-lime-300 sm:text-sm ${
                      view === item
                        ? "bg-white/7 text-zinc-100"
                        : "text-zinc-500 hover:text-zinc-200"
                    }`}
                  >
                    {item === "acquire" ? "Acquire" : `Library · ${library.length}`}
                  </button>
                </li>
              ))}
            </ul>
          </nav>
          <div className="hidden shrink-0 items-center gap-2 sm:flex">
            <span className="size-1.5 shrink-0 rounded-full bg-lime-300" />
            <p className="font-mono text-sm tabular-nums text-zinc-500">
              {formatSampleRate(sampleRate)} host
            </p>
          </div>
        </div>
      </header>

      <div className="grid min-h-[calc(100dvh-3.25rem)] grid-cols-1 md:grid-cols-[minmax(0,1fr)_18rem] xl:grid-cols-[minmax(0,1fr)_20rem]">
        <section className="min-w-0 px-4 py-5 sm:px-6 sm:py-6 md:border-r md:border-white/8">
          {view === "acquire" ? (
            <div className="flex flex-col gap-5">
              <div>
                <p className="font-mono text-base uppercase tracking-wide text-lime-300 sm:text-sm">
                  New rip
                </p>
                <h1 className="max-w-[24ch] text-balance text-3xl font-semibold tracking-tight text-white">
                  Pull audio into your set
                </h1>
                <p className="max-w-[56ch] text-pretty text-base text-zinc-500 sm:text-sm">
                  Paste a public URL. Rippr prepares the full source as a DAW-ready WAV.
                </p>
              </div>

              <form className="flex flex-col gap-4" onSubmit={submitRip}>
                <div className="flex flex-col gap-2">
                  <label
                    htmlFor="source-url"
                    className="text-base font-medium text-zinc-300 sm:text-sm"
                  >
                    Media URL
                  </label>
                  <div className="flex min-w-0 flex-col gap-2 sm:flex-row">
                    <input
                      id="source-url"
                      name="sourceUrl"
                      type="url"
                      inputMode="url"
                      autoComplete="url"
                      required
                      disabled={busy}
                      value={sourceUrl}
                      onChange={(event) => setSourceUrl(event.target.value)}
                      placeholder="https://…"
                      className="min-w-0 flex-1 rounded-lg bg-white/5 px-3 py-2.5 text-base text-white ring-1 ring-white/10 placeholder:text-zinc-600 focus-visible:-outline-offset-1 focus-visible:outline-2 focus-visible:outline-lime-300 disabled:cursor-wait disabled:text-zinc-500 sm:py-2 sm:text-sm"
                    />
                    <button
                      type="submit"
                      disabled={busy}
                      className={`rounded-lg px-3 py-2.5 text-base font-medium focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-lime-300 disabled:cursor-wait sm:py-2 sm:text-sm ${
                        activeSample
                          ? "bg-white/7 text-zinc-100 ring-1 ring-white/8 hover:bg-white/10 disabled:text-zinc-600"
                          : "bg-lime-300 text-zinc-950 ring-1 ring-lime-300 hover:bg-lime-200 disabled:bg-lime-300/35 disabled:text-zinc-600"
                      }`}
                    >
                      {busy ? "Ripping…" : activeSample ? "Rip another" : "Rip sample"}
                    </button>
                  </div>
                </div>

                <div className="flex min-w-0 items-center gap-3 border-y border-white/8 py-3">
                  <FolderOpenIcon className="size-4 h-lh shrink-0 fill-zinc-600" />
                  <div className="min-w-0 flex-1">
                    <p className="text-base font-medium text-zinc-400 sm:text-sm">
                      Save WAVs to
                    </p>
                    <p
                      className="truncate text-base text-zinc-500 sm:text-sm"
                      title={sampleDirectory}
                    >
                      {sampleDirectory || "Loading sample folder…"}
                    </p>
                  </div>
                  <button
                    type="button"
                    disabled={busy}
                    onClick={() => void chooseSampleFolder()}
                    className="shrink-0 rounded-md bg-white/6 px-2.5 py-1.5 text-base text-zinc-300 ring-1 ring-white/8 hover:bg-white/10 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-lime-300 disabled:cursor-wait disabled:text-zinc-700 sm:text-sm"
                  >
                    Change
                  </button>
                </div>

                <div className="[--padding:--spacing(2)] [--radius:var(--radius-xl)] rounded-(--radius) bg-black/25 p-(--padding) ring-1 ring-white/8">
                  <div className="overflow-hidden rounded-[calc(var(--radius)-var(--padding))] bg-[#171714]">
                    <Waveform sample={activeSample} />
                    <div className="grid grid-cols-3 divide-x divide-white/8 border-t border-white/8">
                      <div className="min-w-0 px-3 py-2.5">
                        <p className="text-base font-medium text-zinc-400 sm:text-sm">
                          Range
                        </p>
                        <p className="truncate text-base text-zinc-500 sm:text-sm">
                          Full source
                        </p>
                      </div>
                      <div className="min-w-0 px-3 py-2.5">
                        <p className="text-base font-medium text-zinc-400 sm:text-sm">
                          Format
                        </p>
                        <p className="truncate text-base text-zinc-500 sm:text-sm">
                          32-bit float
                        </p>
                      </div>
                      <div className="min-w-0 px-3 py-2.5">
                        <p className="text-base font-medium text-zinc-400 sm:text-sm">
                          Limit
                        </p>
                        <p className="truncate text-base text-zinc-500 sm:text-sm">
                          None
                        </p>
                      </div>
                    </div>
                  </div>
                </div>
              </form>
            </div>
          ) : (
            <div className="flex flex-col gap-5">
              <div>
                <p className="font-mono text-base uppercase tracking-wide text-lime-300 sm:text-sm">
                  Local cache
                </p>
                <h1 className="max-w-[24ch] text-balance text-3xl font-semibold tracking-tight text-white">
                  Sample library
                </h1>
                <p className="max-w-[56ch] text-pretty text-base text-zinc-500 sm:text-sm">
                  Reload prepared WAVs instantly without contacting the source.
                </p>
              </div>

              <label className="relative">
                <span className="sr-only">Search sample library</span>
                <MagnifyingGlassIcon className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 fill-zinc-600" />
                <input
                  name="libraryQuery"
                  type="search"
                  value={libraryQuery}
                  onChange={(event) => setLibraryQuery(event.target.value)}
                  placeholder="Search title, artist, or source"
                  className="w-full rounded-lg bg-white/5 py-2.5 pr-3 pl-9 text-base text-white ring-1 ring-white/10 placeholder:text-zinc-600 focus-visible:-outline-offset-1 focus-visible:outline-2 focus-visible:outline-lime-300 sm:py-2 sm:text-sm"
                />
              </label>

              {filteredLibrary.length ? (
                <ul className="divide-y divide-white/8 border-y border-white/8" role="list">
                  {filteredLibrary.map((entry) => {
                    const isActive = entry.id === activeSample?.id;
                    return (
                      <li key={entry.id} className="flex min-w-0 items-center gap-3 py-3.5">
                        <span
                          className={`size-1.5 shrink-0 rounded-full ${isActive ? "bg-lime-300" : "bg-zinc-800"}`}
                          aria-hidden="true"
                        />
                        <div className="min-w-0 flex-1">
                          <p className="truncate text-base font-medium text-zinc-100 sm:text-sm">
                            {entry.title}
                          </p>
                          <p className="truncate text-base text-zinc-500 sm:text-sm">
                            {entry.creator ?? entry.sourceUrl}
                          </p>
                        </div>
                        <p className="hidden shrink-0 font-mono text-sm tabular-nums text-zinc-500 sm:block">
                          {formatTime(entry.durationSeconds)}
                        </p>
                        <button
                          type="button"
                          disabled={busy || isActive}
                          onClick={() => void activateLibraryEntry(entry)}
                          aria-label={isActive ? `${entry.title} is active` : `Load ${entry.title}`}
                          className="shrink-0 rounded-md bg-white/6 px-2.5 py-1.5 text-base text-zinc-300 ring-1 ring-white/8 hover:bg-white/10 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-lime-300 disabled:cursor-default disabled:bg-transparent disabled:text-lime-300 disabled:ring-transparent sm:text-sm"
                        >
                          {isActive ? "Active" : "Load"}
                        </button>
                      </li>
                    );
                  })}
                </ul>
              ) : (
                <div className="border-y border-white/8 py-10 text-center">
                  <p className="text-pretty text-base text-zinc-500 sm:text-sm">
                    {library.length
                      ? "No samples match this search."
                      : "Your prepared samples will appear here."}
                  </p>
                </div>
              )}
            </div>
          )}
        </section>

        <aside className="min-w-0 border-t border-white/8 bg-white/2 px-4 py-5 sm:px-5 md:border-t-0">
          <div className="flex flex-col gap-5 md:sticky md:top-18">
            <section aria-labelledby="active-sample-heading">
              <div className="flex items-center justify-between gap-3">
                <h2 id="active-sample-heading" className="text-lg font-semibold text-white">
                  Active sample
                </h2>
                <p
                  className={`max-w-[11rem] shrink-0 truncate rounded-full px-2 py-1 text-sm ${
                    statusTone === "error"
                      ? "bg-red-400/10 text-red-300"
                      : statusTone === "busy"
                        ? "bg-amber-300/10 text-amber-200"
                        : statusTone === "ready"
                          ? "bg-lime-300/10 text-lime-300"
                          : "bg-white/5 text-zinc-500"
                  }`}
                >
                  {stage}
                </p>
              </div>

              <div className="flex flex-col gap-3 pt-4">
                <div
                  className="h-1 overflow-hidden rounded-full bg-white/6"
                  role="progressbar"
                  aria-label="Rip progress"
                  aria-valuemin={0}
                  aria-valuemax={100}
                  aria-valuenow={Math.round(progress * 100)}
                >
                  <div
                    className="h-full w-(--rip-progress) rounded-full bg-lime-300"
                    style={{ "--rip-progress": progressWidth } as CSSProperties}
                  />
                </div>

                {notice ? (
                  <div
                    role={notice.tone === "error" ? "alert" : "status"}
                    className={`flex items-start gap-2 rounded-lg px-3 py-2 ring-1 ${
                      notice.tone === "error"
                        ? "bg-red-400/6 text-red-200 ring-red-300/15"
                        : "bg-lime-300/6 text-lime-200 ring-lime-300/15"
                    }`}
                  >
                    {notice.tone === "error" ? (
                      <ExclamationTriangleIcon className="size-4 h-lh shrink-0 fill-current" />
                    ) : (
                      <CheckCircleIcon className="size-4 h-lh shrink-0 fill-current" />
                    )}
                    <p className="min-w-0 text-pretty text-base sm:text-sm">
                      {notice.message}
                    </p>
                  </div>
                ) : null}

                <div className="min-w-0 border-b border-white/8 pb-3">
                  <p className="truncate text-base font-medium text-zinc-100 sm:text-sm">
                    {visibleTitle}
                  </p>
                  <p className="truncate text-base text-zinc-500 sm:text-sm">
                    {visibleCreator}
                  </p>
                </div>

                {activeSample ? (
                  <dl
                    className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-1.5"
                    aria-label="Prepared WAV format"
                  >
                    <dt className="text-base font-medium text-zinc-400 sm:text-sm">
                      File
                    </dt>
                    <dd className="truncate text-right text-base text-zinc-500 sm:text-sm" title={activeSample.mediaPath}>
                      {fileName(activeSample.mediaPath)}
                    </dd>
                    <dt className="text-base font-medium text-zinc-400 sm:text-sm">
                      Format
                    </dt>
                    <dd className="text-right text-base text-zinc-500 sm:text-sm">
                      32-bit float WAV
                    </dd>
                    <dt className="text-base font-medium text-zinc-400 sm:text-sm">
                      Audio
                    </dt>
                    <dd className="text-right text-base tabular-nums text-zinc-500 sm:text-sm">
                      {formatSampleRate(activeSample.renderedSampleRate)} · Stereo
                    </dd>
                    <dt className="text-base font-medium text-zinc-400 sm:text-sm">
                      Length
                    </dt>
                    <dd className="text-right font-mono text-sm tabular-nums text-zinc-500">
                      {formatTime(activeSample.durationSeconds)}
                    </dd>
                  </dl>
                ) : null}

                <button
                  type="button"
                  disabled={!activeSample}
                  onPointerDown={startWavDrag}
                  className="flex w-full cursor-grab items-center justify-center gap-2 rounded-lg bg-lime-300 py-2.5 pr-3 pl-2 text-base font-medium text-zinc-950 ring-1 ring-lime-300 hover:bg-lime-200 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-lime-300 active:cursor-grabbing disabled:cursor-not-allowed disabled:bg-white/5 disabled:text-zinc-700 disabled:ring-white/5 sm:py-2 sm:text-sm"
                >
                  <Bars3Icon className="size-4 h-lh shrink-0 fill-current" />
                  Drag WAV into your DAW
                </button>

                <div className="grid grid-cols-2 gap-2">
                  <button
                    type="button"
                    disabled={!activeSample?.previewAvailable}
                    onClick={() => void togglePreview()}
                    aria-label={previewing ? "Stop preview" : "Preview sample"}
                    aria-pressed={previewing}
                    className="flex items-center justify-center gap-1.5 rounded-lg bg-white/7 py-2 pr-3 pl-2 text-base text-zinc-200 ring-1 ring-white/8 hover:bg-white/10 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-lime-300 disabled:cursor-not-allowed disabled:bg-white/3 disabled:text-zinc-700 sm:text-sm"
                  >
                    {previewing ? (
                      <StopIcon className="size-4 h-lh shrink-0 fill-current" />
                    ) : (
                      <PlayIcon className="size-4 h-lh shrink-0 fill-current" />
                    )}
                    {previewing ? "Stop" : "Preview"}
                  </button>
                  <button
                    type="button"
                    disabled={!activeSample}
                    onClick={() => void revealActiveSample()}
                    className="flex items-center justify-center gap-1.5 rounded-lg bg-white/3 py-2 pr-3 pl-2 text-base text-zinc-400 ring-1 ring-white/8 hover:bg-white/7 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-lime-300 disabled:cursor-not-allowed disabled:text-zinc-700 sm:text-sm"
                  >
                    <FolderOpenIcon className="size-4 h-lh shrink-0 fill-current" />
                    Reveal
                  </button>
                </div>
              </div>
            </section>

            <section className="border-t border-white/8 pt-4" aria-labelledby="workflow-heading">
              <h2 id="workflow-heading" className="text-lg font-semibold text-white">
                Workflow
              </h2>
              <ul className="flex flex-col gap-2 pt-3" role="list">
                <li className="flex items-start gap-2">
                  <Bars3Icon className="size-4 h-lh shrink-0 fill-zinc-600" />
                  <p className="text-pretty text-base text-zinc-500 sm:text-sm">
                    Press and hold the drag control, then drop onto an audio track.
                  </p>
                </li>
                <li className="flex items-start gap-2">
                  <ArrowPathIcon className="size-4 h-lh shrink-0 fill-zinc-600" />
                  <p className="text-pretty text-base text-zinc-500 sm:text-sm">
                    Any MIDI note retriggers the active sample from the start.
                  </p>
                </li>
              </ul>
            </section>
          </div>
        </aside>
      </div>
    </main>
  );
}
