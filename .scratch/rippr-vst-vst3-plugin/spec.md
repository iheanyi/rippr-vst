---
title: "Build rippr-vst as an in-DAW sample acquisition instrument"
status: ready-for-agent
triage_label: ready-for-agent
tracker: local-markdown
---

## Problem Statement

Beat makers currently have to leave their DAW, acquire audio in a separate application, trim and convert it, find the resulting file, and then import it back into the project. That context switching makes quick sampling slow and breaks the creative flow.

The user needs a VST3 instrument that accepts a supported public media URL inside the DAW, prepares a sample away from the real-time audio thread, previews and trims it, and exposes the result as playable plug-in audio and as a DAW-importable WAV file.

The VST3 standard does not give a plug-in a portable way to create a host track or insert a clip on the host timeline. The product must therefore define “pass it into the DAW” as two explicit, host-compatible behaviors: emit the prepared sample through the plug-in's stereo output, and initiate a native file drag for the rendered WAV with a reveal-in-file-manager fallback.

## Solution

Build `rippr-vst` as a greenfield Rust workspace containing a VST3 stereo instrument, an isolated download/transcode worker, a reusable sample-acquisition core, a content-addressed local sample library, and a React/TypeScript editor embedded as a child WebView.

The user pastes a URL, reviews metadata, selects a trim range on a waveform, and starts a Rip Job. A helper process uses bundled acquisition and media tools to download and transcode the source without running network, process, file, decode, or allocation work in the DAW's real-time callback. When the job completes, a Prepared Sample is loaded in the background and transferred to the Playback Engine through a bounded lock-free handoff. The user can audition it, trigger it with MIDI, record/freeze/bounce the instrument output, drag its WAV into the DAW, or reopen it from the local Sample Library.

Use the active community `nice-plug` framework to export VST3 through the permissively licensed Rust `vst3` bindings. Build the editor with React, TypeScript, and Vite, embedded through `wxp`/Wry using the host-provided native parent window. `wxp` supplies Tauri-like `invoke` and `Channel` IPC, but the VST bundle will not run the Tauri application runtime. An optional standalone Tauri shell may be added later by sharing the same core and frontend bridge contracts.

## User Stories

1. As a beat maker, I want to install one VST3 bundle, so that I can use rippr-vst without manually installing Python, yt-dlp, or FFmpeg.
2. As a beat maker, I want the plug-in to appear as a stereo instrument or generator, so that its prepared sample flows through an ordinary DAW mixer channel.
3. As a beat maker, I want to paste a supported public media URL into the plug-in, so that I can begin acquiring a sample without leaving my DAW.
4. As a beat maker, I want malformed and unsupported URLs rejected before work starts, so that failures are immediate and understandable.
5. As a beat maker, I want the plug-in to fetch title, creator, duration, and thumbnail metadata, so that I can confirm I selected the intended source.
6. As a beat maker, I want to edit the sample title and creator before saving it, so that my library uses meaningful names.
7. As a beat maker, I want acquisition limits shown before a job starts, so that I know when a source is too long or too large for the plug-in.
8. As a beat maker, I want a waveform overview after source preparation, so that I can find the useful section visually.
9. As a beat maker, I want to set precise trim-in and trim-out points, so that the Prepared Sample contains only the section I intend to use.
10. As a beat maker, I want trim handles to remain ordered and bounded by the source duration, so that I cannot create an invalid sample.
11. As a beat maker, I want to audition the selected region before finalizing it, so that I can refine the cut by ear.
12. As a beat maker, I want clear Rip Job stages and progress, so that metadata, download, transcode, analysis, and load work never look frozen.
13. As a beat maker, I want to cancel a Rip Job, so that I can stop a mistaken or unexpectedly large acquisition.
14. As a beat maker, I want partial files cleaned up after cancellation or failure, so that abandoned jobs do not consume disk space.
15. As a beat maker, I want actionable network, provider, disk, worker, and media-decoding errors, so that I know whether to retry or choose another source.
16. As a beat maker, I want a completed Prepared Sample to become the Active Sample automatically, so that it is immediately ready to play.
17. As a beat maker, I want the plug-in to output silence while no Active Sample is ready, so that loading and restoration never create noise.
18. As a beat maker, I want to trigger the Active Sample as a one-shot from MIDI, so that I can sequence or record it from my controller or piano roll.
19. As a beat maker, I want MIDI note-on timing honored at the event's sample offset, so that recorded playback is rhythmically precise.
20. As a beat maker, I want a UI preview trigger, so that I can audition a sample without creating a MIDI clip.
21. As a beat maker, I want a stop action with a short click-free fade, so that auditioning never produces a discontinuity.
22. As a beat maker, I want an automatable output-gain parameter, so that the DAW can save and automate level changes using standard host controls.
23. As a beat maker, I want the sample to keep its original speed and pitch in the first release, so that playback is predictable and artifact-free.
24. As a beat maker, I want to record, freeze, or bounce the instrument output, so that the sample can become ordinary DAW audio using host-native workflows.
25. As a beat maker, I want to drag the rendered WAV from the plug-in into a compatible DAW arrangement, so that I can create a clip with one gesture.
26. As a beat maker, I want a Reveal in Finder or Explorer fallback, so that I can import the WAV even when a host rejects native file drag.
27. As a beat maker, I want each successful rip stored in a searchable Sample Library, so that I can reuse earlier samples without reacquiring them.
28. As a beat maker, I want duplicate requests with identical source and trim settings to reuse a valid cached artifact, so that repeated work is fast and bandwidth-efficient.
29. As a beat maker, I want to see source, title, creator, duration, trim range, creation time, and file status for each Library Entry, so that I can identify and audit cached samples.
30. As a beat maker, I want to remove a Library Entry and its unreferenced media, so that I can reclaim disk space intentionally.
31. As a beat maker, I want a configurable cache quota and a clear-cache action, so that the plug-in cannot grow without bound.
32. As a beat maker, I want multiple plug-in instances to share the Sample Library safely, so that different projects do not corrupt common metadata or media.
33. As a beat maker, I want each plug-in instance to maintain its own Active Sample and playback state, so that instances do not trigger or replace one another.
34. As a beat maker, I want the DAW project to restore the Active Sample reference, trim metadata, and plug-in parameters, so that reopening a session returns to the same setup.
35. As a beat maker, I want project restoration to use the local cache without network access, so that existing sessions open offline.
36. As a beat maker, I want a visible Missing Sample state when cached media has moved or been deleted, so that silent output is explained.
37. As a beat maker, I want missing media to require an explicit reacquire action, so that opening a project never starts an unexpected network download.
38. As a beat maker, I want the plug-in to do no network or helper-process work during host scanning, validation, or headless rendering, so that DAW startup and render farms remain stable.
39. As a beat maker, I want Rip Jobs and waveform rendering to remain glitch-free while audio is playing, so that acquisition cannot interrupt the session.
40. As a beat maker, I want the editor to resize and scale correctly on supported displays, so that it is usable across DAW window sizes and DPI settings.
41. As a keyboard user, I want URL entry, buttons, trim controls, library navigation, focus states, and shortcuts to be accessible, so that the editor is operable without precise pointer input.
42. As a beat maker, I want the URL field and metadata editor to cooperate with DAW keyboard shortcuts, so that typing does not accidentally trigger host transport commands and unused keys still reach the host.
43. As a user, I want the first release to handle public, unauthenticated URLs without account setup, so that sampling remains immediate inside the DAW.
44. As a user, I want credentials, browser cookies, DRM, and authenticated sessions kept outside the first release, so that the plug-in has a smaller security and maintenance surface.
45. As a privacy-conscious user, I want media processing and library storage to remain local, so that my samples and project activity are not uploaded to a rippr-vst service.
46. As a user, I want production WebView navigation restricted to bundled assets and explicitly opened external links, so that pasted metadata cannot navigate the editor to untrusted pages.
47. As a user, I want a signed and notarized macOS build and a signed Windows build, so that installing the plug-in does not require weakening operating-system security.
48. As a developer, I want deterministic fake-provider fixtures, so that the complete request-to-audio behavior can be tested without live websites.
49. As a developer, I want the audio callback to fail tests when it allocates, locks, performs I/O, or blocks, so that later features cannot silently compromise real-time safety.
50. As a maintainer, I want acquisition-tool versions and third-party notices shipped with each release, so that provider breakage and licensing obligations can be audited.

## Implementation Decisions

- Treat this as a greenfield product. Do not depend on or port an existing Rippr repository. Establish the domain terms Rip Request, Rip Job, Prepared Sample, Active Sample, Library Entry, Playback Engine, and Worker and use them consistently across Rust, TypeScript, IPC, tests, and documentation.
- Ship the first product as a VST3 stereo instrument/generator with no audio input, one stereo audio output, and MIDI event input. It emits the Active Sample as a one-shot and is intentionally not modeled as an insert effect.
- Define DAW handoff as plug-in audio output plus native WAV file drag. Do not claim that the plug-in can create a host track or insert a timeline clip through VST3 because no portable VST3 API provides that capability.
- Use the current stable `nice-plug` community framework and its current permissively licensed `vst3` bindings. Pin released versions in the lockfile and enable the framework's processing-allocation assertion in debug and CI builds. Do not use the maintenance-mode NIH-plug VST3 export path whose older bindings impose GPLv3 constraints.
- Keep the VST3 format adapter thin. The product domain, worker client, library store, sample preparation, and Playback Engine must not expose VST3 COM types outside the plug-in adapter.
- Build the editor with React, TypeScript, and Vite. Reuse normal web-platform layout and styling, but do not embed or start the Tauri runtime inside the DAW host process.
- Embed the editor with `wxp` on top of Wry using the native parent window supplied by the host through the `nice-plug` Editor interface. Pin `wxp` to a reviewed Git revision until its public API stabilizes.
- Treat the `nice-plug` plus `wxp` editor lifecycle as the first implementation milestone. The vertical slice must prove create, resize, focus, text entry, close, reopen, and asynchronous command delivery in at least one macOS and one Windows host before feature work expands.
- Use `wxp`'s Tauri-like `invoke` interface for request/response commands and `Channel` for Rip Job and library events. Define one versioned, serde-backed command/event contract shared by Rust and TypeScript; unknown messages must fail safely.
- Keep WebView assets local in production, disable developer tools in release builds, restrict navigation and custom protocols, and sanitize all metadata rendered by the editor. Opening a source URL in the system browser must be an explicit user action.
- Split the application into focused modules: a pure acquisition/domain core, a Worker executable, a Worker client and supervisor, a content-addressed library store, a Prepared Sample decoder/resampler, a real-time Playback Engine, the VST3 adapter, the embedded editor adapter, and release tooling.
- Run provider access and FFmpeg in a separately signed Worker process. The VST module must never load Python into the DAW process or invoke acquisition tools from the audio callback. The Worker is started only after an explicit user action from an open editor.
- Bundle standalone yt-dlp and FFmpeg executables for each supported platform instead of requiring a system installation. Pass arguments directly without shell interpolation. Do not implement in-place self-updates of signed bundled executables; acquisition-tool updates ship as full product updates.
- Use a versioned JSON-lines protocol over the Worker's standard input, standard output, and standard error streams. Requests include a job ID, canonical URL, trim range, output policy, and limits. Events include accepted, metadata, progress, prepared, cancelled, failed, and worker-exited outcomes.
- Have command handlers submit work to a dedicated supervisor and return promptly. Long-running work must not occupy the WebView run-loop thread, the host UI thread, or the plug-in framework's real-time task path.
- Enforce one active Rip Job per plug-in instance in the first release. A new request must either wait for or explicitly cancel the existing job. Batch queues and playlists are deferred.
- Use per-job temporary directories and atomically publish a completed artifact only after validation succeeds. Cancellation, worker crash, parse failure, quota failure, and media failure must remove or quarantine incomplete artifacts without damaging previously completed Library Entries.
- Render the canonical handoff artifact as a stereo WAV suitable for DAW import. Preserve source level by default, record the source and rendered sample rates in the manifest, and perform trim/transcode work in the Worker.
- Decode and resample a Prepared Sample on a background thread for the current host sample rate. The audio callback receives only immutable, already-prepared floating-point PCM and precomputed playback metadata.
- Transfer new Prepared Samples to the audio callback through bounded single-producer/single-consumer queues. When the audio thread replaces an Active Sample, it returns the old allocation through a reclamation queue so memory is freed on a background thread rather than in the callback.
- Preallocate trigger and reclamation queues. The audio callback may read parameters, consume MIDI/UI trigger messages, advance playheads, apply short fades and gain, and write output buffers. It may not allocate, lock, log, access the network, touch the filesystem, spawn processes, decode media, wait, or drop the final owner of a large allocation.
- Honor MIDI note-on sample offsets. Any note triggers the single Active Sample in the first release; note mapping, chromatic pitch, velocity layers, pads, slicing, and polyphonic voices are later features. UI preview triggers at the next audio block boundary.
- Expose output gain as the only automatable audio parameter in the first release. URL, metadata, library selection, trim bounds, job state, and cache paths are non-automatable application state.
- Use a content-addressed Sample Library keyed by the acquisition identity, provider/source identity, trim bounds, output format version, and toolchain version. Store human metadata separately from immutable media identity so title edits do not duplicate audio.
- Persist Library metadata in an embedded SQLite database configured for multi-instance access and transactional migrations. Store media in a dedicated application-data cache with atomic files and a configurable quota.
- Persist only the Active Sample identifier, provenance needed to explain it, trim metadata, editor state, and automatable parameters in the DAW project state. Do not embed media bytes in VST3 state. On restore, load from the cache asynchronously; if missing, remain silent and require explicit reacquisition.
- Implement native file drag below the WebView layer. On macOS initiate an `NSDraggingSession` carrying a file URL; on Windows initiate an OLE file drag carrying `CF_HDROP`. Start the operation only from a current pointer gesture. Always provide Reveal in Finder or Explorer and Copy Path as fallbacks.
- Set explicit maximum source duration, downloaded bytes, rendered bytes, concurrent work, and cache usage. Validate HTTPS URLs, reject local-file and private-network targets in URL acquisition commands, and never accept an executable path from the WebView.
- Do not support browser-cookie import, account credential capture, DRM handling, paywalled sources, or authenticated provider sessions in the first release. This is a technical scope boundary that keeps credentials out of the DAW host process and reduces provider-specific maintenance.
- Target macOS and Windows VST3 bundles first. Build architecture-specific Worker and media-tool resources, sign nested executables before signing the enclosing bundle, notarize the macOS distribution, and include third-party notices. Linux, AU, AAX, and release CLAP packages require separate compatibility decisions.
- Validate the bundle with the Steinberg VST3 validator and pluginval in CI. Run smoke tests in representative macOS and Windows DAWs for editor lifecycle, text focus, job completion, project restore, offline render, MIDI timing, native drag, and repeated open/close behavior.
- Keep an optional standalone UI feasible by defining a frontend bridge interface whose production implementations can be `wxp` and, later, Tauri. A standalone Tauri application is not required for the VST3 MVP and must not become a Worker prerequisite.

## Testing Decisions

- A good test asserts observable product behavior and stable protocol contracts. Tests must not assert private struct layout, framework call order, SQL implementation details, component names, exact progress percentages, or WebView internals.
- Use one primary acceptance seam: run the real session/controller, Worker protocol client, library store, Prepared Sample loader, and Playback Engine against a deterministic fake Worker. Submit a Rip Request, consume metadata and progress, publish the fixture WAV, activate it, send a MIDI note at a nonzero sample offset, render offline, and assert the emitted PCM, state transitions, and persisted Library Entry.
- Extend that same seam with scenarios for cancellation, worker exit, corrupt output, quota failure, duplicate-cache reuse, missing-cache restoration, two independent plug-in instances, and project state round trips. Keep live provider/network behavior out of deterministic CI.
- Test the Worker as a black-box executable using local fixture media and a fake acquisition executable. Assert the versioned JSON-lines request/event contract, direct argument passing, cancellation, limits, temporary-file cleanup, and atomic publication.
- Test the Playback Engine with fixed input events and golden numeric expectations. Assert silence with no sample, sample-accurate MIDI start, one-shot completion, click-free stop, gain application, channel behavior, sample replacement, queue saturation behavior, and zero allocations in processing.
- Test the frontend against a mock implementation of the same typed bridge. Assert the user-visible URL, metadata, trim, progress, cancellation, error, library, missing-sample, drag fallback, focus, and restoration flows without coupling tests to DOM structure or CSS class names.
- Add contract generation or compile-time checks so Rust command/event payload changes cannot silently diverge from TypeScript consumers.
- Treat Steinberg validator and pluginval at a meaningful strictness as automated release gates. Their purpose is lifecycle and host-contract validation, not proof of product behavior.
- Maintain a small manual host matrix on current macOS and Windows releases. Verify at least one host from different vendors on each platform, with special attention to child-window parenting, DPI, keyboard focus, native file drag, project save/reopen, offline bounce, and repeated editor teardown.
- Add long-running stress checks that acquire and replace fixture samples while rendering audio, repeatedly open and close the editor, cancel at each Worker stage, and terminate the host-side instance. Fail on deadlocks, audio discontinuities beyond the expected trigger/stop envelope, leaked worker processes, or unbounded cache growth.
- There is no existing greenfield project test prior art. Use the `nice-plug` example plug-ins for host and parameter conventions, the `wxp` examples for WebView command/channel lifecycle, and official VST3 validators as external prior art rather than copying an unrelated application test structure.

## Out of Scope

- Automatically creating a DAW track or inserting a clip through a nonstandard host API.
- Guaranteeing native file drag in every VST3 host; supported hosts are documented from the compatibility matrix and all others retain file-manager fallbacks.
- Using an existing Rippr repository as a code, architecture, UI, schema, or test baseline.
- Shipping a full Tauri runtime inside the VST3 plug-in process.
- A standalone Tauri desktop product in the first release.
- AU, AUv3, AAX, Linux packages, mobile targets, and cloud-hosted processing in the first release.
- Batch URL queues, playlists, account logins, imported browser cookies, DRM, paywall bypasses, and private/authenticated media.
- Time stretching, tempo synchronization, pitch shifting, chromatic sampling, slicing, pads, multiple sample slots, velocity layers, polyphony, looping, effects, and stem separation.
- Embedding full sample media in DAW project state or guaranteeing project portability to a machine without the same cache.
- Automatically updating yt-dlp or FFmpeg inside an installed signed plug-in bundle.
- A commercial launch, paid licensing, telemetry, accounts, cloud sync, or an updater service.

## Further Notes

### Feasibility assessment

- **Rust VST3 instrument and real-time sample playback: high feasibility.** The current Rust ecosystem has permissively licensed VST3 bindings and an active community framework with VST3 export, state, parameters, MIDI, background-task support, bundling, and custom editors.
- **Acquisition, trim, analysis, and cache work inside a DAW: high feasibility with process isolation.** The risky version is performing that work in the plug-in callback or loading a scripting runtime into the host. A separately signed Worker and a lock-free Prepared Sample handoff make the boundary tractable.
- **React/WebView editor: medium-to-high feasibility.** Wry supports child WebViews, and `wxp` specifically adds Tauri-like commands/channels and plug-in-host lifecycle fixes. `wxp` is still alpha and revision-pinned, so the editor lifecycle vertical slice is a mandatory early gate.
- **Full Tauri embedded in VST3: low feasibility and not recommended.** Tauri assumes an application runtime and event loop, while VST3 gives the plug-in a host-owned parent view and host-controlled lifecycle. Reuse the React UI and Tauri interaction style through `wxp`; reserve Tauri itself for a later standalone shell.
- **Automatic host track creation: not portable.** VST3 standardizes processor, controller, parameters, buses, events, state, and an attached child view, not DAW project editing. Audio output, host bounce, and native file drag are the portable product contract.

### Key product risks

- Provider extraction changes frequently. Pin acquisition tools, surface their versions in diagnostics, build fixture-based fallbacks, and ship full product updates rather than silently mutating an installed plug-in.
- Native WebViews reduce frontend rewrite cost but inherit host-specific focus, run-loop, DPI, and teardown behavior. The compatibility matrix is part of the feature, not post-launch polish.
- A VST3 bundle containing a Worker, yt-dlp, and FFmpeg will be materially larger than a normal audio plug-in. Installer, signing, notarization, antivirus reputation, third-party notices, and update bandwidth must be planned from the beginning.
- External cache references make project state small and host-friendly but reduce project portability. A future “collect sample into project folder” workflow could improve portability without embedding media in VST state.

### Primary references

- VST3 processor/controller separation and threading model: https://steinbergmedia.github.io/vst3_dev_portal/pages/Technical%2BDocumentation/API%2BDocumentation/Index.html
- VST3 host-owned plug-in view contract: https://steinbergmedia.github.io/vst3_doc/base/classSteinberg_1_1IPlugView.html
- Active Rust plug-in framework: https://codeberg.org/RustAudio/nice-plug
- Permissively licensed Rust VST3 bindings: https://crates.io/crates/vst3
- Tauri-like WebView IPC for audio plug-ins: https://github.com/novonotes/wxp
- Wry child WebViews and platform constraints: https://github.com/tauri-apps/wry
- Cross-platform plug-in validation: https://github.com/Tracktion/pluginval
