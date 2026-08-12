export type RipRequestInput = {
  sourceUrl: string;
};

export type LibraryEntry = {
  id: string;
  title: string;
  creator?: string;
  sourceUrl: string;
  durationSeconds: number;
  waveformPeaks: [number, number][];
  previewAvailable: boolean;
  mediaPath: string;
};

export type RipEvent =
  | { type: "accepted" }
  | { type: "progress"; stage: string; fraction?: number }
  | {
      type: "metadata";
      title: string;
      creator?: string;
      durationSeconds?: number;
    }
  | { type: "ready"; entry: LibraryEntry }
  | { type: "failed"; message: string };

export type BootstrapState = {
  sampleRate: number;
  sampleDirectory: string;
  entries: LibraryEntry[];
  activeEntry?: LibraryEntry;
};

export interface RipprBridge {
  bootstrap(): Promise<BootstrapState>;
  startRip(
    request: RipRequestInput,
    onEvent: (event: RipEvent) => void,
  ): Promise<void>;
  activateLibraryEntry(
    id: string,
    onEvent: (event: RipEvent) => void,
  ): Promise<void>;
  preview(): Promise<void>;
  stopPreview(): Promise<void>;
  startWavDrag(): Promise<void>;
  revealActiveSample(): Promise<void>;
  chooseSampleDirectory(): Promise<string | undefined>;
}

type WxpChannel<T> = {
  onmessage?: (message: T) => void;
  toIPC?: () => string;
};

type WxpWindow = Window & {
  invoke?: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
  Channel?: new <T>(onmessage?: (message: T) => void) => WxpChannel<T>;
};

class NativeChannel<T> {
  private readonly channel: WxpChannel<T>;

  constructor(onmessage: (message: T) => void) {
    const Channel = (window as WxpWindow).Channel;
    if (!Channel) throw new Error("The plug-in event bridge is unavailable.");
    this.channel = new Channel(onmessage);
  }

  toJSON(): string {
    const id = this.channel.toIPC?.();
    if (!id) throw new Error("The plug-in event channel is unavailable.");
    return id;
  }
}

async function invoke<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  const nativeInvoke = (window as WxpWindow).invoke;
  if (!nativeInvoke) throw new Error("The plug-in command bridge is unavailable.");
  return nativeInvoke<T>(command, args);
}

export class NativeRipprBridge implements RipprBridge {
  async bootstrap(): Promise<BootstrapState> {
    return invoke("bootstrap");
  }

  async startRip(
    request: RipRequestInput,
    onEvent: (event: RipEvent) => void,
  ): Promise<void> {
    const channel = new NativeChannel(onEvent);
    await invoke("start_rip", { request, channel });
  }

  async activateLibraryEntry(
    id: string,
    onEvent: (event: RipEvent) => void,
  ): Promise<void> {
    const channel = new NativeChannel(onEvent);
    await invoke("activate_library_entry", { id, channel });
  }

  async preview(): Promise<void> {
    await invoke("preview");
  }

  async stopPreview(): Promise<void> {
    await invoke("stop_preview");
  }

  async startWavDrag(): Promise<void> {
    await invoke("start_wav_drag");
  }

  async revealActiveSample(): Promise<void> {
    await invoke("reveal_active_sample");
  }

  async chooseSampleDirectory(): Promise<string | undefined> {
    const result = await invoke<{ path?: string }>("choose_sample_directory");
    return result.path;
  }
}

export class MockRipprBridge implements RipprBridge {
  readonly requests: RipRequestInput[] = [];
  dragStarts = 0;
  previewStarts = 0;
  previewStops = 0;
  private readonly cachedEntry: LibraryEntry = {
    id: "cached-fixture",
    title: "Cached break",
    creator: "Fixture artist",
    sourceUrl: "https://example.test/cached",
    durationSeconds: 8,
    waveformPeaks: [[-0.2, 0.3], [-0.8, 0.7], [-0.35, 0.5]],
    previewAvailable: true,
    mediaPath: "/tmp/cached-fixture.wav",
  };

  async bootstrap(): Promise<BootstrapState> {
    return {
      sampleRate: 48_000,
      sampleDirectory: "/Users/fixture/Music/Rippr Samples",
      entries: [this.cachedEntry],
    };
  }

  async startRip(
    request: RipRequestInput,
    onEvent: (event: RipEvent) => void,
  ): Promise<void> {
    this.requests.push(request);
    onEvent({ type: "accepted" });
    onEvent({ type: "progress", stage: "download", fraction: 0.62 });
    onEvent({
      type: "metadata",
      title: "Fixture break",
      creator: "Fixture artist",
      durationSeconds: 28.4,
    });
    onEvent({
      type: "ready",
      entry: {
        id: "fixture",
        title: "Fixture break",
        creator: "Fixture artist",
        sourceUrl: request.sourceUrl,
        durationSeconds: 28.4,
        waveformPeaks: [[-0.1, 0.2], [-0.9, 0.85], [-0.4, 0.55]],
        previewAvailable: true,
        mediaPath: "/tmp/fixture.wav",
      },
    });
  }

  async activateLibraryEntry(
    id: string,
    onEvent: (event: RipEvent) => void,
  ): Promise<void> {
    if (id !== this.cachedEntry.id) throw new Error("Cached sample is missing.");
    onEvent({ type: "ready", entry: this.cachedEntry });
  }

  async preview(): Promise<void> {
    this.previewStarts += 1;
  }

  async stopPreview(): Promise<void> {
    this.previewStops += 1;
  }

  async startWavDrag(): Promise<void> {
    this.dragStarts += 1;
  }

  async revealActiveSample(): Promise<void> {}

  async chooseSampleDirectory(): Promise<string | undefined> {
    return "/Users/fixture/Music/New Samples";
  }
}
