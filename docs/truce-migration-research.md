# Truce migration research

Research date: 2026-08-12
Target reviewed: Truce **v6.3.0**, the current GitHub release as of this review.

## Recommendation

Proceed with the Truce rewrite on the feature branch, with **VST3 + AUv2 + standalone** as the first supported matrix. Truce has the audio, event, state, editor-parenting, resize, export, build, install, validation, and signing primitives needed for Rippr. It does **not** provide a WebView editor or an arbitrary sidecar-resource declaration, so the current WXP editor and helper-process bundle flow remain custom integration work.

Three items are migration gates rather than cleanup:

1. **WXP ownership/thread affinity.** `truce_core::Editor` is `Send`, while the pinned WXP WebView is deliberately UI-thread-bound (`!Send`/`!Sync`). Store the live WXP resources behind a checked UI-thread wrapper such as `SendWrapper`, or keep them in a thread-local registry addressed by a `Send` token. Creation, dereference, close, and drop must all occur on the opening UI thread. Validate repeated open/close/reopen in every host before treating this as safe. See Truce's [`Editor: Send` contract](https://github.com/truce-audio/truce/blob/v6.3.0/crates/truce-core/src/editor.rs#L86-L112).
2. **Helper resources and signing.** Truce 6.3.0's configuration schema has no arbitrary bundle-resource/sidecar field. `cargo truce` stages and signs each macOS bundle during build/package, so copying `rippr-worker`, `yt-dlp`, or `ffmpeg` afterward invalidates the outer signature. The project needs a resource-aware build wrapper (or an upstream Truce resource feature), followed by inside-out re-signing. See the [configuration schema](https://github.com/truce-audio/truce/blob/v6.3.0/crates/truce-build/src/lib.rs#L250-L380), [VST3 staging](https://github.com/truce-audio/truce/blob/v6.3.0/crates/cargo-truce/src/commands/package/stage.rs#L320-L399), [AUv2 staging](https://github.com/truce-audio/truce/blob/v6.3.0/crates/cargo-truce/src/commands/package/stage.rs#L512-L595), and [Truce's recursive signer](https://github.com/truce-audio/truce/blob/v6.3.0/crates/cargo-truce/src/util/codesign.rs#L60-L140).
3. **Plugin identity.** Truce derives the VST3 CID from its canonical plugin ID; it does not expose an arbitrary CID override in the v6.3.0 schema. A framework migration from the existing explicit nice-plug CID therefore changes the VST3 class identity. `migrate_state` cannot repair a session in which the DAW cannot instantiate the old class. Because Rippr is not yet a stable public release, this is the right time to accept and freeze the new identity; otherwise explicit legacy-CID support must be added to Truce before shipping. See [canonical ID derivation](https://github.com/truce-audio/truce/blob/v6.3.0/crates/truce-build/src/lib.rs#L18-L49) and [VST3 CID derivation](https://github.com/truce-audio/truce/blob/v6.3.0/crates/truce-utils/src/state.rs#L246-L264).

## Version and crate setup

The latest release is [`v6.3.0`](https://github.com/truce-audio/truce/releases/tag/v6.3.0). Its workspace declares Rust **1.92**, edition **2024**, and the custom `LicenseRef-TruceLicense-1.0` license identifier ([source](https://github.com/truce-audio/truce/blob/v6.3.0/Cargo.toml#L73-L84)). The repository already targets Rust 1.92, so there is no MSRV increase.

Pin the first migration to the exact Truce family version (`=6.3.0`) and retain `Cargo.lock`. Truce is moving quickly enough that accepting a semver-compatible update while the adapter is being stabilized would make failures harder to attribute. Follow the official example's format-feature layout:

```toml
[lib]
crate-type = ["cdylib", "staticlib", "rlib"]

[[bin]]
name = "rippr-vst-standalone"
path = "src/main.rs"
required-features = ["standalone"]

[features]
default = ["vst3"]
vst3 = ["dep:truce-vst3", "truce/vst3"]
au = ["dep:truce-au"]
standalone = ["dep:truce-standalone"]

[dependencies]
truce = { version = "=6.3.0", default-features = false }
truce-vst3 = { version = "=6.3.0", optional = true }
truce-au = { version = "=6.3.0", optional = true }
truce-standalone = { version = "=6.3.0", features = ["gui"], optional = true }
```

This mirrors the official [example crate configuration](https://github.com/truce-audio/truce/blob/v6.3.0/examples/truce-example-gain/Cargo.toml#L12-L53). Keep each format crate at the same exact version as `truce`; the `truce::plugin!` macro emits every enabled format export ([macro contract](https://github.com/truce-audio/truce/blob/v6.3.0/crates/truce/src/plugin_macro.rs#L1-L78), [format exports](https://github.com/truce-audio/truce/blob/v6.3.0/crates/truce/src/plugin_macro.rs#L224-L257)):

```rust
truce::plugin! {
    logic: Rippr,
    params: RipprParams,
}
```

The standalone entry point is intentionally tiny, matching the [official example](https://github.com/truce-audio/truce/blob/v6.3.0/examples/truce-example-gain/src/main.rs#L1-L5):

```rust
use rippr_plugin::Plugin;

fn main() {
    truce_standalone::run::<Plugin>();
}
```

## `truce.toml` and frozen identity

A suitable phase-one configuration is:

```toml
[vendor]
name = "Iheanyi Ekechukwu"
id = "com.iheanyi"
url = "https://github.com/iheanyi/rippr-vst"
au_manufacturer = "IhEk"

[[plugin]]
name = "Rippr"
bundle_id = "rippr-vst"
crate = "rippr-plugin"
category = "instrument"
description = "Acquire, preview, and drag DAW-ready samples without leaving your session."
fourcc = "Ripr"
vst3_subcategory = "Sampler"
au_tag = "Sampler"
```

`au_manufacturer` and `fourcc` must each remain four ASCII characters. `category = "instrument"` supplies MIDI input by default; an explicit `midi_input = true` is only documentation, not a functional requirement. `bundle_id` must be lowercase ASCII alphanumerics with `-`, `_`, or `.` separators and is the stable identity input ([schema and validation](https://github.com/truce-audio/truce/blob/v6.3.0/crates/truce-build/src/lib.rs#L27-L70), [plugin fields](https://github.com/truce-audio/truce/blob/v6.3.0/crates/truce-build/src/lib.rs#L266-L305)).

Freeze these values before the first public build:

- Canonical Truce ID: `com.iheanyi.rippr-vst`.
- AU manufacturer: `IhEk`.
- AU subtype/fourcc: `Ripr`.
- VST3 CID: `DA A5 0B 63 A1 25 B8 B9 0C 04 FF B5 A4 1F A2 A2`, the 16 little-endian bytes returned by FNV-1a-128 over the canonical ID ([algorithm](https://github.com/truce-audio/truce/blob/v6.3.0/crates/truce-utils/src/state.rs#L246-L264)).

Changing the display `name` is safe; changing `vendor.id` or `bundle_id` changes the saved-state envelope identity and VST3 CID. There is no `vst3_cid` field in the official v6.3.0 `PluginDef` schema.

## API mapping

| Current responsibility | Truce v6.3.0 implementation |
| --- | --- |
| Plugin descriptor and DSP | Implement `PluginLogic` for a stateless descriptor. Set `type Params` and `type DspState`; use `init`, `reset`, and real-time-safe `process`. Truce owns and passes mutable `DspState` per block. ([trait](https://github.com/truce-audio/truce/blob/v6.3.0/crates/truce-plugin/src/lib.rs#L190-L480)) |
| Instrument bus | Return a stereo output-only `BusLayout` and keep `category = "instrument"`. Consume note events by `sample_offset` and write `buffer.output(ch)[sample]`, as the official synth examples do. ([bus API](https://github.com/truce-audio/truce/blob/v6.3.0/crates/truce-core/src/bus.rs)) |
| Parameters and editor-shared state | Put the gain in a `#[param]` field, small session values such as active sample ID/path in `#[persist]` fields, and unsaved queues/atomics/path services in `#[skip]` fields on `Params`. The editor factory receives only `Arc<Params>`, not the DSP state. ([state guide](https://truce.audio/docs/guide/state/), [editor factory](https://github.com/truce-audio/truce/blob/v6.3.0/crates/truce-plugin/src/lib.rs#L456-L480)) |
| Audio-owned playback engine | Keep `PlaybackEngine`, the UI-to-audio consumer, and any reclamation handoff in `DspState`. Move only producer/shared handles into `#[skip]` state reachable through `Params`. Never lock, allocate, perform I/O, or destroy large samples in `process`. ([real-time contract](https://github.com/truce-audio/truce/blob/v6.3.0/crates/truce-plugin/src/lib.rs#L270-L306)) |
| Plugin editor | Implement `truce_core::Editor` directly and construct WXP in `open(parent, context)`. Tear it down in `close`; use `idle` only for short UI pumping. ([Editor API](https://github.com/truce-audio/truce/blob/v6.3.0/crates/truce-core/src/editor.rs#L86-L220)) |
| Format exports | Replace the nice-plug export macro with one `truce::plugin!` invocation. Cargo features select VST3/AU; standalone calls `truce_standalone::run::<Plugin>()`. ([plugin macro](https://github.com/truce-audio/truce/blob/v6.3.0/crates/truce/src/plugin_macro.rs#L1-L78)) |
| State migration | Params and `#[persist]` fields are handled by Truce's envelope. Use `snapshot_into`/`snapshot_version` only for small custom DSP state and `migrate_state(ForeignState)` for recognizable legacy blobs after the new plugin has been instantiated. ([state methods](https://github.com/truce-audio/truce/blob/v6.3.0/crates/truce-plugin/src/lib.rs#L306-L455)) |

One Rippr-specific construction issue deserves an explicit design: the paired `rtrb::Producer`/`Consumer` is currently created together, but `DspState::init` receives `&Params`. A workable shape is to create the ring in the `Params` default, store the producer in shared `#[skip]` state, and store the consumer as a one-time `Option` consumed by `init`. That assumption must be tested against host instance/reset behavior: `reset` may run repeatedly, while `init` must be the only consumer-take site.

For saved state, persist a stable library/cache identifier or user handoff path, not decoded sample audio. Truce's inline snapshot path runs from the audio-thread state-save lane and its own docs classify it as KB-scale; large sampler audio belongs in the existing cache and should be reloaded asynchronously ([snapshot guidance](https://github.com/truce-audio/truce/blob/v6.3.0/crates/truce-plugin/src/lib.rs#L306-L407)).

## WXP custom editor integration

Truce's official roadmap lists a WebView backend as future work, so WXP should remain Rippr's renderer rather than being replaced with a Truce built-in backend ([roadmap](https://truce.audio/docs/roadmap/)). Truce does support hand-rolled editors through its raw parent-window API ([raw-window guide](https://truce.audio/docs/guide/gui/raw-window-handle/)).

The adapter should implement:

```rust
impl Editor for RipprEditor {
    fn size(&self) -> (u32, u32) { (960, 660) }
    fn open(&mut self, parent: RawWindowHandle, context: PluginContext) { /* WXP child */ }
    fn close(&mut self) { /* drop WXP on opener UI thread */ }
    fn set_size(&mut self, width: u32, height: u32) -> bool { /* post WXP bounds */ true }
    fn can_resize(&self) -> bool { true }
    fn min_size(&self) -> (u32, u32) { (640, 480) }
    fn can_maximize(&self) -> bool { true }
}
```

`can_maximize` matters only to standalone; plugin hosts own their top-level window. `max_size` can remain unbounded if the web UI genuinely renders correctly at arbitrary dimensions. Truce uses logical points and supplies host scale changes through `set_scale_factor`; AppKit already handles Retina scaling ([full resize/DPI contract](https://github.com/truce-audio/truce/blob/v6.3.0/crates/truce-core/src/editor.rs#L86-L220)).

Raw handle conversion needs a local adapter because the two APIs are not type-compatible:

- Truce exposes `RawWindowHandle::AppKit(*mut c_void)` as an **NSView pointer**, `Win32(*mut c_void)` as an HWND, and `X11(u64)` as a window ID ([definition](https://github.com/truce-audio/truce/blob/v6.3.0/crates/truce-core/src/editor.rs#L86-L93)).
- WXP's Wry stack uses `raw-window-handle` 0.6. Construct a small local `WxpParent` that implements the 0.6 `HasWindowHandle` contract. On macOS, wrap the non-null NSView pointer in `AppKitWindowHandle`; on Windows, wrap HWND in `Win32WindowHandle`.
- Do not reuse Truce's public `truce_gui::platform::ParentWindow` adapter: in v6.3.0 it targets raw-window-handle 0.5, while this WXP revision expects 0.6.
- Treat Linux as a separate follow-up. Truce provides a generic X11 ID while a Wry backend may distinguish Xlib from Xcb; do not claim Linux support until that handle contract is tested.

The current Rippr editor also installs native AppKit edit shortcuts and native `NSDraggingSession` support. Those should continue to receive the original Truce NSView pointer; neither feature should be routed through HTML drag-and-drop.

Resize capability differs by format. The official roadmap records host-driven resize support for VST3, CLAP, AUv3, and LV2, while **AUv2 opens at natural size**. Therefore:

- Ableton/VST3 must exercise live host resize and fullscreen/maximized host layouts.
- GarageBand/AUv2 is a valid functional test target, but it will not prove host-driven resizing under Truce 6.3.0.
- Standalone must exercise edge resize and maximize/fullscreen because Rippr opts into `can_maximize`.

Custom raw editors do not automatically implement Truce's headless screenshot hook; its default is `None` ([Editor screenshot API](https://github.com/truce-audio/truce/blob/v6.3.0/crates/truce-core/src/editor.rs#L210-L230)). Keep browser/Vitest UI tests and real-host screenshots unless a WXP-specific headless renderer is added.

## Worker process, resources, and signing

Do **not** move downloads, yt-dlp, ffmpeg, SQLite access, waveform decoding, or cache restores into Truce `BackgroundTask`. Truce's pool is process-global and deliberately small; handlers must stay short and non-blocking because one blocking task stalls other plugin instances. Long/blocking work belongs on a plugin-owned thread ([task contract](https://github.com/truce-audio/truce/blob/v6.3.0/crates/truce-core/src/tasks.rs#L1-L35), [worker guide](https://truce.audio/docs/guide/workers/)). Rippr's existing dedicated worker process/thread boundary is the correct architecture.

For macOS development bundles, adapt the existing bundle script to this order:

1. Run `cargo truce build` for the requested format(s).
2. Copy `rippr-worker`, `yt-dlp`, `ffmpeg`, and the third-party notices into that bundle's `Contents/Resources`.
3. Sign each nested Mach-O/helper executable.
4. Sign the completed outer `.vst3`, `.component`, or `.app` bundle last.
5. Verify with `codesign --verify --deep --strict --verbose=2 <bundle>` and inspect the resource paths from inside the running plugin.

Truce's signer deliberately discovers and signs Mach-O binaries from the inside out; its source warns against relying on `codesign --deep` as the signing operation ([implementation](https://github.com/truce-audio/truce/blob/v6.3.0/crates/cargo-truce/src/util/codesign.rs#L60-L140)). `--deep` is appropriate above only as a verification check.

This works for VST3, AUv2, and standalone because each macOS executable can resolve its adjacent `Contents/Resources`. However, `cargo truce package` currently rebuilds/stages/signs its own bundles and offers no arbitrary resource hook. Before release packaging, choose one of:

- contribute/use an upstream `resources`/sidecar manifest feature in Truce;
- maintain a small Truce fork that stages helpers before its signing pass; or
- keep a custom packaging/notarization pipeline operating on resource-complete, re-signed bundles.

AUv2 staging marks the component sandbox-safe in its Info.plist ([staging source](https://github.com/truce-audio/truce/blob/v6.3.0/crates/cargo-truce/src/commands/package/stage.rs#L512-L595)). GarageBand acceptance must therefore verify actual helper launch, network acquisition, transcoding, cache writes, preview, and native drag-out; `auval` alone cannot establish that those runtime operations work in the host.

AUv3 should stay out of phase one. Its out-of-process/sandboxed execution and packaging model make helper execution and writable-location behavior a separate product effort.

## Build and validation plan

Install the matching CLI and inspect the workstation:

```bash
cargo install cargo-truce --version 6.3.0 --locked
cargo truce doctor
```

Build and install the two plugin formats, then run standalone:

```bash
cargo truce build --vst3 --au2 -p rippr-plugin
# stage helpers and re-sign the completed bundles here
cargo truce install --vst3 --au2 -p rippr-plugin
cargo truce run -p rippr-plugin
```

The documented CLI supports per-format build/install and standalone run workflows ([CLI reference](https://truce.audio/docs/reference/cli/), [README build flow](https://github.com/truce-audio/truce/blob/v6.3.0/README.md#L20-L80)). Validate the installed formats explicitly rather than invoking validators for formats Rippr does not ship:

```bash
cargo truce validate --pluginval --auval -p rippr-plugin
auval -v aumu Ripr IhEk
```

The first command uses pluginval for VST3 and Apple's auval for AUv2 ([validation docs](https://github.com/truce-audio/truce/blob/v6.3.0/README.md#L188-L229)). On the machine inspected during this research, `auval` and `xcrun` are present; `cargo-truce` and `pluginval` still need to be installed before full validation.

Automated and validator checks are necessary but not sufficient. Treat the following as release acceptance:

| Target | Required host checks |
| --- | --- |
| Ableton Live / VST3 | Scan and instantiate; open/close/reopen editor; resize in both directions; fullscreen/maximized host layout; `Cmd+V`; full rip and accurate waveform; preview then stop; MIDI trigger; native WAV drag to an audio track; Reveal WAV; save/reopen project and restore active sample. |
| GarageBand / AUv2 | Component scan and instantiate; natural-size editor; open/close/reopen; `Cmd+V`; helper launch/network/download/transcode/cache; waveform; preview/stop; MIDI trigger; WAV drag/reveal; project restore. Do not require AUv2 host-driven resize. |
| Truce standalone | Launch; resize/maximize/fullscreen; full UI/worker/preview/drag/reveal flow; clean close and relaunch. |

For all three, repeat editor open/close/reopen enough times to catch UI-thread-affinity violations or stale WXP dispatch handles. Also instantiate multiple plugin instances: Truce constructs Params/DSP state per instance, and Rippr's one-time ring-buffer consumer handoff must not leak across instances.

## Licensing

Truce's license expressly permits building, shipping, and selling audio plug-ins with no permission, fee, splash screen, or revenue cap. The rider applies only to redistributing Truce itself as a commercial developer framework or operating its framework capabilities as a commercial service; an audio plugin is explicitly excluded ([license grant](https://github.com/truce-audio/truce/blob/v6.3.0/LICENSE#L1-L41), [not-covered products](https://github.com/truce-audio/truce/blob/v6.3.0/LICENSE#L72-L99)). Rippr's use is therefore allowed under the MIT-or-Apache grant with the normal obligations of the selected license.

Because Truce declares `LicenseRef-TruceLicense-1.0`, do not describe the dependency as plain MIT alone. Add the complete Truce License 1.0 plus the selected MIT or Apache-2.0 notice/text to the repository and generated third-party notices, and bundle those notices beside the helper binaries.

## Implementation order

1. Freeze `truce.toml` identity and pin the Truce family to `=6.3.0`.
2. Port params/shared state and DSP to `PluginLogic`; keep worker I/O outside Truce's shared background pool.
3. Replace the export macro and add the standalone entry point.
4. Add the raw-window-handle 0.6 bridge and WXP resource ownership strategy; prove open/close/reopen before adding more UI work.
5. Wire Truce resize callbacks to WXP bounds and validate responsive layout in VST3 and standalone.
6. Adapt helper resource staging and inside-out signing for `.vst3`, `.component`, and `.app`.
7. Run Rust/UI tests, `cargo truce validate`, and the Ableton/GarageBand/standalone acceptance matrix.
8. Resolve the `cargo truce package` resource gap before notarized distribution.

## Primary official sources

- [Truce v6.3.0 release](https://github.com/truce-audio/truce/releases/tag/v6.3.0)
- [Official documentation](https://truce.audio/docs/)
- [v6.3.0 source](https://github.com/truce-audio/truce/tree/v6.3.0)
- [Official raw-window editor guide](https://truce.audio/docs/guide/gui/raw-window-handle/)
- [Official roadmap](https://truce.audio/docs/roadmap/)
- [Official CLI reference](https://truce.audio/docs/reference/cli/)
- [Truce License 1.0](https://github.com/truce-audio/truce/blob/v6.3.0/LICENSE)
