# rippr-vst

`rippr-vst` is a greenfield VST3 sample-acquisition instrument. Paste a public
HTTPS media URL in the editor and the isolated Worker uses the bundled `yt-dlp`
and FFmpeg executables to prepare the complete source as a stereo WAV. A real
waveform envelope is analyzed from that rendered file. The plug-in can then drag
the friendly-named WAV directly onto a macOS DAW audio track, trigger ordinary
sized samples from any MIDI note, or reveal the WAV as a fallback.

The plug-in does not depend on another Rippr repository and does not embed a
Tauri application runtime. Its React/TypeScript UI is compiled to one local HTML
document and attached to the DAW-owned editor window with `wxp`. The typed
frontend bridge deliberately remains shell-agnostic, so a standalone Tauri shell
can be added later without making Tauri a VST runtime dependency.

## Architecture

- `rippr-core`: validated domain types, Worker client, SQLite/WAL Sample Library,
  content-addressed publication, WAV loading/resampling, and the real-time-safe
  Playback Engine.
- `rippr-worker`: versioned JSON-lines Worker protocol plus direct, shell-free
  `yt-dlp` and FFmpeg execution in per-job temporary directories.
- `rippr-plugin`: `nice-plug` VST3 instrument adapter, one automatable output-gain
  parameter, MIDI/UI trigger handling, bounded sample handoff/reclamation queues,
  persisted Active Sample ID, and the embedded `wxp` editor.
- `ui`: React, TypeScript, Vite, and Tailwind editor with Acquire and searchable
  local Library views. Native and deterministic mock bridges implement the same
  TypeScript contract.

Acquisition begins only after an explicit editor action. Host scanning and
headless validation do not start the Worker or access the network. Project
restoration reads the persisted sample ID from the local cache and stays silent
when that media is missing.

## Build and test

Prerequisites are a current stable Rust toolchain, Node.js, and the pinned
bundler (`cargo install cargo-nice-plug --version 0.1.1 --locked`). On Apple
Silicon, prepare the pinned media tools and build the signed development bundle
with:

```sh
./scripts/prepare-tools-macos-arm64.sh
./scripts/bundle-macos.sh
```

The resulting development artifact is `target/bundled/rippr-vst.vst3`. The
bundle script ad-hoc signs the nested Worker/tools before signing the enclosing
VST3 bundle. Production distribution still requires your Developer ID signing
identity and Apple notarization.

Run the deterministic suites with:

```sh
cargo test --workspace
(cd ui && npm ci && npm test && npm run build)
```

The tests use fixture acquisition executables and local WAVs; they do not call
live providers. Release candidates should additionally pass pluginval at
strictness 10 and Steinberg's VST3 validator, then be smoke-tested in real VST3
hosts on macOS and Windows.

## Host formats

The first artifact is VST3 only. GarageBand hosts Audio Units, so it cannot load
this bundle. Supporting GarageBand requires a separately packaged and validated
AU target; that is intentionally outside this MVP.

VST3 does not offer a portable API for creating a DAW track or timeline clip.
On macOS, drag the friendly-named WAV from the editor directly onto a DAW audio
track; the plug-in starts a native AppKit drag session while preserving the
content-addressed cache file. The editor's **Choose folder** action persists a
user-selected destination for these friendly WAVs; Reveal in Finder remains
available as a fallback. Standard AppKit edit shortcuts, including Command-V,
are routed to the focused WebView field even when the host intercepts key events.

## Current release boundaries

- Public, unauthenticated HTTPS sources only.
- One active acquisition or cache-load job per plug-in instance.
- Full-source acquisition has no configured duration, download-size, rendered-size,
  or cache-quota ceiling. Available disk space and the media tools remain practical
  constraints.
- Acquisition selects the provider's best available audio stream and writes lossless
  stereo 32-bit float PCM at the source's native sample rate, avoiding an unnecessary
  sample-rate conversion before the WAV reaches the DAW.
- Stereo one-shot playback at the host's current sample rate. Very large WAVs are
  still saved, analyzed, revealed, and draggable, but in-plug-in preview is disabled
  above the bounded in-memory preview threshold to protect the DAW process.
- The editor's Preview control becomes Stop during playback and halts the active
  one-shot immediately through the lock-free UI-to-audio command queue.
- Local shared cache; no credentials, cookies, DRM handling, cloud upload, or
  in-place updater for bundled tools.
- macOS Apple Silicon development bundling is automated here. Windows resources,
  Authenticode signing, macOS notarization, Windows native file drag, interactive job
  cancellation, and cache removal controls remain release engineering work.

The complete product specification and compatibility gates live in
`.scratch/rippr-vst-vst3-plugin/spec.md`.
