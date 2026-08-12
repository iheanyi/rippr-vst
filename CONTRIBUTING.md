# Contributing to rippr-vst

Thanks for helping improve rippr-vst. Keep changes focused and preserve the
real-time and process-isolation boundaries described in the
[product specification](docs/product-spec.md).

## Development setup

Install a stable Rust toolchain and Node.js 24. Then install dependencies and
run the deterministic test suites:

```sh
(cd ui && npm ci)
cargo test --workspace
(cd ui && npm test && npm run build)
```

The macOS VST3, AUv2, and standalone development bundles additionally require
`cargo-truce 6.3.0` and the pinned media tools:

```sh
cargo install cargo-truce --version 6.3.0 --locked
./scripts/prepare-tools-macos-arm64.sh
./scripts/bundle-macos.sh
```

## Before opening a pull request

Run the same checks as CI:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p rippr-plugin --features rt-paranoid
(cd ui && npm test && npm run build)
cargo truce validate --pluginval --auval -p rippr-plugin
```

If the editor changes, commit the regenerated `ui/dist/index.html`; it is
embedded into the plug-in binary at compile time. Do not commit downloaded
media tools, DAW projects, credentials, cookies, or rendered samples.

## Architectural boundaries

- Never perform network, process, filesystem, decoding, allocation, or locking
  work on the real-time audio callback.
- Keep acquisition in the Worker process and pass command arguments directly;
  do not interpolate shell commands.
- Keep Truce and host-format types in the plug-in adapter rather than the
  reusable core.
- Treat native WAV drag and plug-in audio output as the DAW handoff contract;
  VST3 does not portably create host tracks or timeline clips.
- Keep the frontend bridge shell-agnostic so an optional Tauri shell could
  reuse the UI without making Tauri part of a plug-in runtime.

Provider changes are expected. Keep live-provider checks out of deterministic
tests and use fixture executables and local WAVs instead.
