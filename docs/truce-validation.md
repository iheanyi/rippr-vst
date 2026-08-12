# Truce migration validation

Validation date: 2026-08-12

Branch: `feat/truce-migration`
Framework: Truce 6.3.0

## Automated checks

The migrated plug-in passed:

- `cargo test --workspace`
- `cargo test -p rippr-plugin --features rt-paranoid`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo check -p rippr-plugin --no-default-features --features au`
- `cargo check -p rippr-plugin --no-default-features --features standalone`
- UI Vitest suite and production single-file build
- `cargo truce validate --pluginval --auval -p rippr-plugin`
- strict deep signature verification for `Rippr.vst3`, `Rippr.component`, and
  `Rippr.app`, after staging the Worker and media-tool resources

The Truce-specific tests cover metadata, the instrument bus contract, parameter
defaults and IDs, parameter state round-trip, editor factory reuse and size
consistency, and zero allocations inside `process` under `rt-paranoid`.

## Host and runtime checks

### Ableton Live 12.4.3 / VST3

- Live rescanned the bundle under `Iheanyi Ekechukwu / Rippr`.
- Live instantiated the new Truce VST3 class successfully.
- The runtime log reported CID `DAA50B63-A125-B8B9-0C04-FFB5A41FA2A2` and no
  load error.
- pluginval passed the installed VST3.

The automated desktop controller cannot reliably focus or capture Live's
accessibility-opaque floating plug-in window, and it could not keep the AppKit
drag session held while crossing into Live. Complete the resize/fullscreen,
`Cmd+V`, and native WAV drop checks manually before release.

### GarageBand / AUv2

- GarageBand discovered Rippr under `AU Instruments / Iheanyi Ekechukwu`.
- The component loaded as a stereo software instrument.
- The real WXP editor rendered correctly at its natural AUv2 size.
- Closing and reopening the editor repeatedly succeeded without a
  `SendWrapper` thread-affinity failure.
- Apple's full `auval` suite passed UI discovery, parameter state, channel
  layout, render rates, MIDI, and initialization.

GarageBand's plug-in WebView is not exposed as editable accessibility elements,
so the desktop controller could not address the URL field for a direct `Cmd+V`
test. Truce 6.3.0 does not provide host-driven AUv2 resize; natural-size editor
behavior is the expected result.

### Truce standalone

- The editor opened, resized down to its declared 640-by-480 minimum, maximized,
  and remained responsive at both sizes.
- `Cmd+V` copied and restored a URL in the real WebView input.
- The shared local library loaded a real cached 15-second WAV.
- The UI rendered its persisted analyzed waveform rather than placeholder bars.
- The handoff path used a friendly title-based filename.
- Preview changed to Stop while playing, and Stop returned it to Preview.
- The primary native drag handle and Reveal fallback were both enabled for the
  active sample.

## Final manual release gates

1. In Ableton, open/close/reopen the VST3 editor, resize it in both directions,
   and exercise Live's maximized/fullscreen layout.
2. Paste a URL with `Cmd+V` in both Ableton and GarageBand.
3. Press and hold **Drag WAV into your DAW**, cross the plug-in boundary, and
   drop the friendly-named WAV on an Ableton audio track.
4. Run one fresh network acquisition from each hosted format to prove helper
   launch and resource discovery from the installed bundles.
5. Save and reopen one project in each host to verify active-sample restore.

These are manual gates because they depend on host-owned, accessibility-opaque
windows or a cross-application native drag gesture. They are not replaced by a
green unit suite or format validator.
