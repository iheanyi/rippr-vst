import {
  ArrowTopRightOnSquareIcon,
  Bars3Icon,
  FolderOpenIcon,
  MagnifyingGlassIcon,
  MusicalNoteIcon,
  PlayIcon,
  StopIcon,
} from "@heroicons/react/16/solid";
import { FormEvent, PointerEvent, useEffect, useMemo, useRef, useState } from "react";

import type {
  LibraryEntry,
  RipEvent,
  RipprBridge,
} from "./bridge";

type AppProps = {
  bridge: RipprBridge;
};

function formatTime(seconds: number): string {
  const minutes = Math.floor(seconds / 60);
  const remaining = seconds - minutes * 60;
  return `${minutes}:${remaining.toFixed(2).padStart(5, "0")}`;
}

function Waveform({ sample }: { sample?: LibraryEntry }) {
  const peaks = sample?.waveformPeaks ?? [];
  if (!sample || peaks.length === 0) {
    return (
      <div
        className="flex h-32 items-center justify-center px-4 text-center text-sm text-zinc-600"
        aria-label="Sample waveform overview"
      >
        The waveform appears here after the full WAV is prepared.
      </div>
    );
  }

  const width = 1_000;
  const height = 128;
  const center = height / 2;
  const scale = Math.max(
    0.001,
    ...peaks.flatMap(([min, max]) => [Math.abs(min), Math.abs(max)]),
  );
  const x = (index: number) =>
    peaks.length === 1 ? width / 2 : (index / (peaks.length - 1)) * width;
  const top = peaks.map(([, max], index) =>
    `${x(index).toFixed(2)},${(center - (max / scale) * (center - 8)).toFixed(2)}`,
  );
  const bottom = [...peaks].reverse().map(([min], reverseIndex) => {
    const index = peaks.length - reverseIndex - 1;
    return `${x(index).toFixed(2)},${(center - (min / scale) * (center - 8)).toFixed(2)}`;
  });

  return (
    <div className="relative h-32 overflow-hidden">
      <svg
        className="h-full w-full"
        viewBox={`0 0 ${width} ${height}`}
        preserveAspectRatio="none"
        role="img"
        aria-label={`Waveform for ${sample.title}`}
      >
        <line x1="0" x2={width} y1={center} y2={center} stroke="rgb(161 161 170 / 0.16)" />
        <polygon points={[...top, ...bottom].join(" ")} fill="rgb(190 242 100 / 0.72)" />
      </svg>
      <span className="absolute right-2 bottom-1 font-mono text-[10px] tabular-nums text-zinc-500">
        {formatTime(sample.durationSeconds)}
      </span>
    </div>
  );
}

export function App({ bridge }: AppProps) {
  const [view, setView] = useState<"acquire" | "library">("acquire");
  const [sourceUrl, setSourceUrl] = useState("");
  const [stage, setStage] = useState("Waiting for a URL");
  const [progress, setProgress] = useState(0);
  const [metadata, setMetadata] = useState<{
    title: string;
    creator?: string;
    durationSeconds?: number;
  }>();
  const [activeSample, setActiveSample] = useState<LibraryEntry>();
  const [error, setError] = useState<string>();
  const [busy, setBusy] = useState(false);
  const [library, setLibrary] = useState<LibraryEntry[]>([]);
  const [libraryQuery, setLibraryQuery] = useState("");
  const [sampleRate, setSampleRate] = useState(48_000);
  const [sampleDirectory, setSampleDirectory] = useState("");
  const [previewing, setPreviewing] = useState(false);
  const previewTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

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

  function startWavDrag(event: PointerEvent<HTMLButtonElement>) {
    if (event.button !== 0 || !activeSample) return;
    event.preventDefault();
    setError(undefined);
    void bridge.startWavDrag().catch((reason) => {
      setError(
        reason instanceof Error
          ? reason.message
          : "The WAV drag could not be started.",
      );
    });
  }

  useEffect(() => {
    let mounted = true;
    void bridge.bootstrap().then((state) => {
      if (!mounted) return;
      setSampleRate(state.sampleRate);
      setSampleDirectory(state.sampleDirectory);
      setLibrary(state.entries);
      if (state.activeEntry) {
        setActiveSample(state.activeEntry);
        setStage(state.activeEntry.previewAvailable ? "Ready to play" : "Ready to drag");
        setProgress(1);
      }
    }).catch((reason) => {
      if (mounted) {
        setError(reason instanceof Error ? reason.message : "The sample library is unavailable.");
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
        setStage(
          event.stage.charAt(0).toUpperCase() + event.stage.slice(1),
        );
        setProgress(event.fraction ?? 0.18);
        break;
      case "metadata":
        setMetadata(event);
        setStage("Preparing sample");
        setProgress(0.8);
        break;
      case "ready":
        if (previewTimer.current) clearTimeout(previewTimer.current);
        setPreviewing(false);
        setActiveSample(event.entry);
        setLibrary((entries) => [
          event.entry,
          ...entries.filter((entry) => entry.id !== event.entry.id),
        ]);
        setStage(event.entry.previewAvailable ? "Ready to play" : "Ready to drag");
        setProgress(1);
        setBusy(false);
        break;
      case "failed":
        setError(event.message);
        setStage("Rip failed");
        setBusy(false);
        break;
    }
  }

  async function activateLibraryEntry(entry: LibraryEntry) {
    setError(undefined);
    setBusy(true);
    setStage("Loading cached sample");
    setProgress(0.4);
    try {
      await bridge.activateLibraryEntry(entry.id, receiveEvent);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "The cached sample could not be loaded.");
      setStage("Missing sample");
      setBusy(false);
    }
  }

  async function submitRip(event: FormEvent) {
    event.preventDefault();
    setError(undefined);
    setBusy(true);
    setProgress(0.04);
    setStage("Validating URL");
    try {
      await bridge.startRip({ sourceUrl }, receiveEvent);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "The rip could not start.");
      setStage("Rip failed");
      setBusy(false);
    }
  }

  async function chooseSampleFolder() {
    setError(undefined);
    try {
      const directory = await bridge.chooseSampleDirectory();
      if (directory) setSampleDirectory(directory);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "The sample folder could not be changed.");
    }
  }

  async function togglePreview() {
    if (!activeSample?.previewAvailable) return;
    setError(undefined);
    try {
      if (previewing) {
        await bridge.stopPreview();
        if (previewTimer.current) clearTimeout(previewTimer.current);
        setPreviewing(false);
        return;
      }

      await bridge.preview();
      setPreviewing(true);
      if (previewTimer.current) clearTimeout(previewTimer.current);
      previewTimer.current = setTimeout(() => {
        previewTimer.current = undefined;
        setPreviewing(false);
      }, Math.max(0, activeSample.durationSeconds * 1_000));
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "Preview playback failed.");
      setPreviewing(false);
    }
  }

  return (
    <main className="isolate min-h-dvh bg-[#11110f] text-zinc-100 antialiased">
      <header className="border-b border-white/8 bg-[#151512]">
        <div className="mx-auto flex min-h-14 max-w-[1200px] items-center gap-4 px-4 sm:px-6">
          <div className="flex shrink-0 items-center gap-2" aria-label="rippr vst">
            <span className="font-mono text-base tracking-[-0.06em] text-lime-300">
              rippr/
            </span>
            <span className="font-mono text-base tracking-[-0.06em] text-zinc-500">
              vst
            </span>
          </div>
          <nav className="min-w-0 flex-1 overflow-x-auto" aria-label="Primary">
            <ul className="flex min-w-max items-center gap-1" role="list">
              <li>
                <button
                  type="button"
                  aria-current={view === "acquire" ? "page" : undefined}
                  onClick={() => setView("acquire")}
                  className={`rounded-md px-3 py-1.5 text-base sm:text-sm ${view === "acquire" ? "bg-white/7 text-zinc-100" : "text-zinc-500 hover:text-zinc-200"}`}
                >
                  Acquire
                </button>
              </li>
              <li>
                <button
                  type="button"
                  aria-current={view === "library" ? "page" : undefined}
                  onClick={() => setView("library")}
                  className={`rounded-md px-3 py-1.5 text-base sm:text-sm ${view === "library" ? "bg-white/7 text-zinc-100" : "text-zinc-500 hover:text-zinc-200"}`}
                >
                  Library
                </button>
              </li>
            </ul>
          </nav>
          <div className="hidden shrink-0 items-center gap-2 text-sm text-zinc-500 sm:flex">
            <span className="size-1.5 rounded-full bg-lime-300" />
            {(sampleRate / 1000).toLocaleString(undefined, { maximumFractionDigits: 1 })} kHz stereo
          </div>
        </div>
      </header>

      <div className="mx-auto grid max-w-[1200px] grid-cols-1 md:grid-cols-[minmax(0,3fr)_minmax(17rem,1fr)]">
        <section className={`${view === "acquire" ? "" : "hidden"} min-w-0 px-4 py-6 sm:px-6 sm:py-8 md:border-r md:border-white/8`}>
          <div className="flex flex-col gap-6">
            <div>
              <p className="font-mono text-base uppercase tracking-wide text-lime-300 sm:text-sm">
                New rip
              </p>
              <h1 className="max-w-[22ch] text-balance text-3xl font-semibold tracking-tight text-white">
                Pull a sound into the session.
              </h1>
              <p className="max-w-[62ch] text-pretty text-base text-zinc-500 sm:text-sm">
                Paste a public media URL and Rippr prepares the complete source
                as a WAV you can trigger or drag into your arrangement.
              </p>
            </div>

            <form className="flex flex-col gap-4" onSubmit={submitRip}>
              <div className="flex flex-col gap-2">
                <label
                  htmlFor="source-url"
                  className="text-base font-medium text-zinc-300 sm:text-sm"
                >
                  Public media URL
                </label>
                <div className="flex min-w-0 flex-col gap-2 sm:flex-row">
                  <input
                    id="source-url"
                    name="sourceUrl"
                    type="url"
                    required
                    value={sourceUrl}
                    onChange={(event) => setSourceUrl(event.target.value)}
                    placeholder="https://…"
                    className="min-w-0 flex-1 rounded-lg bg-white/5 px-3 py-2.5 text-base text-white ring-1 ring-white/10 placeholder:text-zinc-600 focus-visible:-outline-offset-1 focus-visible:outline-2 focus-visible:outline-lime-300 sm:py-2 sm:text-sm"
                  />
                  <button
                    type="submit"
                    disabled={busy}
                    className="rounded-lg bg-lime-300 px-4 py-2.5 text-base font-medium text-zinc-950 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-lime-300 disabled:cursor-not-allowed disabled:bg-lime-300/40 sm:py-2 sm:text-sm"
                  >
                    {busy ? "Ripping…" : "Rip sample"}
                  </button>
                </div>
              </div>

              <div className="flex min-w-0 items-center gap-3 rounded-lg bg-white/3 px-3 py-2.5 ring-1 ring-white/8">
                <FolderOpenIcon className="size-4 shrink-0 fill-zinc-500" />
                <div className="min-w-0 flex-1">
                  <p className="text-xs font-medium text-zinc-400">Sample folder</p>
                  <p className="truncate text-xs text-zinc-600" title={sampleDirectory}>
                    {sampleDirectory || "Loading…"}
                  </p>
                </div>
                <button
                  type="button"
                  onClick={() => void chooseSampleFolder()}
                  className="shrink-0 rounded-md bg-white/7 px-2.5 py-1.5 text-sm text-zinc-200 ring-1 ring-white/8 hover:bg-white/10"
                >
                  Choose folder
                </button>
              </div>

              <div className="[--padding:--spacing(3)] [--radius:var(--radius-xl)] rounded-(--radius) bg-black/25 p-(--padding) ring-1 ring-white/8">
                <div className="rounded-[calc(var(--radius)-var(--padding))] bg-[#191916] p-3">
                  <Waveform sample={activeSample} />
                </div>
              </div>

              <div className="flex flex-wrap items-center gap-3 border-t border-white/8 pt-4">
                {metadata?.durationSeconds ? (
                  <p className="text-base tabular-nums text-zinc-400 sm:text-sm">
                    Full source · {formatTime(metadata.durationSeconds)}
                  </p>
                ) : (
                  <p className="text-base text-zinc-600 sm:text-sm">The complete source will be prepared.</p>
                )}
              </div>
            </form>
          </div>
        </section>

        {view === "library" ? (
          <section className="min-w-0 px-4 py-6 sm:px-6 sm:py-8 md:border-r md:border-white/8" aria-labelledby="library-heading">
            <div className="flex flex-col gap-6">
              <div>
                <p className="font-mono text-base uppercase tracking-wide text-lime-300 sm:text-sm">Local cache</p>
                <h1 id="library-heading" className="text-3xl font-semibold tracking-tight text-white">Sample Library</h1>
                <p className="max-w-[60ch] text-base text-zinc-500 sm:text-sm">Reuse prepared WAVs instantly. Loading a cached sample never contacts the source.</p>
              </div>
              <label className="relative block">
                <span className="sr-only">Search sample library</span>
                <MagnifyingGlassIcon className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 fill-zinc-600" />
                <input
                  type="search"
                  value={libraryQuery}
                  onChange={(event) => setLibraryQuery(event.target.value)}
                  placeholder="Search title, creator, or source"
                  className="w-full rounded-lg bg-white/5 py-2.5 pr-3 pl-9 text-base text-white ring-1 ring-white/10 placeholder:text-zinc-600 focus-visible:-outline-offset-1 focus-visible:outline-2 focus-visible:outline-lime-300 sm:py-2 sm:text-sm"
                />
              </label>
              {filteredLibrary.length ? (
                <ul className="divide-y divide-white/8 border-y border-white/8" role="list">
                  {filteredLibrary.map((entry) => (
                    <li key={entry.id} className="flex min-w-0 items-center gap-4 py-4">
                      <div className="min-w-0 flex-1">
                        <p className="truncate text-base font-medium text-zinc-100 sm:text-sm">{entry.title}</p>
                        <p className="truncate text-base text-zinc-600 sm:text-sm">{entry.creator ?? entry.sourceUrl}</p>
                      </div>
                      <p className="hidden shrink-0 font-mono text-xs tabular-nums text-zinc-600 sm:block">{formatTime(entry.durationSeconds)}</p>
                      <button
                        type="button"
                        disabled={busy}
                        onClick={() => void activateLibraryEntry(entry)}
                        aria-label={`Load ${entry.title}`}
                        className="shrink-0 rounded-lg bg-white/7 px-3 py-2 text-base text-zinc-200 ring-1 ring-white/8 hover:bg-white/10 disabled:cursor-not-allowed disabled:text-zinc-700 sm:text-sm"
                      >
                        Load
                      </button>
                    </li>
                  ))}
                </ul>
              ) : (
                <p className="border-y border-white/8 py-8 text-center text-base text-zinc-600 sm:text-sm">
                  {library.length ? "No cached samples match that search." : "Your prepared samples will appear here."}
                </p>
              )}
            </div>
          </section>
        ) : null}

        <aside className="min-w-0 border-t border-white/8 px-4 py-6 sm:px-6 md:border-t-0">
          <div className="flex flex-col gap-8">
            <section aria-labelledby="active-sample-heading">
              <div className="flex items-center justify-between gap-3">
                <h2
                  id="active-sample-heading"
                  className="text-lg font-semibold text-white"
                >
                  Active sample
                </h2>
                <span className="rounded-full bg-lime-300/10 px-2 py-1 text-sm text-lime-300">
                  {stage}
                </span>
              </div>
              <div className="flex flex-col gap-3 pt-4">
                <div className="h-1 overflow-hidden rounded-full bg-white/6">
                  <div
                    className="h-full rounded-full bg-lime-300"
                    style={{ width: progressWidth }}
                  />
                </div>
                {error ? (
                  <p role="alert" className="text-base text-red-300 sm:text-sm">
                    {error}
                  </p>
                ) : null}
                <div className="min-w-0">
                  <p className="truncate text-base font-medium text-zinc-100 sm:text-sm">
                    {activeSample?.title ?? metadata?.title ?? "No sample loaded"}
                  </p>
                  <p className="truncate text-base text-zinc-600 sm:text-sm">
                    {activeSample?.creator ?? metadata?.creator ?? "Paste a URL to begin."}
                  </p>
                </div>
                <button
                  type="button"
                  disabled={!activeSample}
                  onPointerDown={startWavDrag}
                  className="flex w-full cursor-grab items-center justify-center gap-2 rounded-lg bg-lime-300 px-3 py-2.5 text-base font-medium text-zinc-950 hover:bg-lime-200 active:cursor-grabbing disabled:cursor-not-allowed disabled:bg-lime-300/30 disabled:text-zinc-600 sm:text-sm"
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
                    className="flex items-center justify-center gap-1.5 rounded-lg bg-white/7 py-2 pr-3 pl-2 text-base text-zinc-200 ring-1 ring-white/8 hover:bg-white/10 disabled:cursor-not-allowed disabled:text-zinc-700 sm:text-sm"
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
                    onClick={() => void bridge.revealActiveSample()}
                    className="flex items-center justify-center gap-1.5 rounded-lg bg-white/3 py-2 pr-3 pl-2 text-base text-zinc-400 ring-1 ring-white/8 hover:bg-white/7 disabled:cursor-not-allowed disabled:text-zinc-700 sm:text-sm"
                  >
                    <FolderOpenIcon className="size-4 h-lh shrink-0 fill-current" />
                    Reveal WAV
                  </button>
                </div>
              </div>
            </section>

            <section className="border-t border-white/8 pt-6" aria-labelledby="handoff-heading">
              <h2 id="handoff-heading" className="text-lg font-semibold text-white">
                Into the DAW
              </h2>
              <ul className="flex flex-col gap-3 pt-3" role="list">
                <li className="flex items-start gap-2">
                  <MusicalNoteIcon className="size-4 h-lh shrink-0 fill-zinc-500" />
                  <p className="text-base text-zinc-500 sm:text-sm">
                    Play any MIDI note to trigger previewable samples.
                  </p>
                </li>
                <li className="flex items-start gap-2">
                  <ArrowTopRightOnSquareIcon className="size-4 h-lh shrink-0 fill-zinc-500" />
                  <p className="text-base text-zinc-500 sm:text-sm">
                    Drag the WAV directly onto an audio track in your DAW.
                  </p>
                </li>
              </ul>
            </section>

            <section className="border-t border-white/8 pt-6" aria-labelledby="acquisition-heading">
              <h2 id="acquisition-heading" className="text-lg font-semibold text-white">
                Acquisition
              </h2>
              <dl className="grid grid-cols-2 gap-x-4 gap-y-2 pt-3 text-base sm:text-sm">
                <dt className="text-zinc-600">Range</dt>
                <dd className="text-right text-zinc-300">Full source</dd>
                <dt className="text-zinc-600">Size limit</dt>
                <dd className="text-right text-zinc-300">None</dd>
                <dt className="text-zinc-600">Output</dt>
                <dd className="text-right text-zinc-300">32-bit float WAV</dd>
              </dl>
            </section>
          </div>
        </aside>
      </div>
    </main>
  );
}
