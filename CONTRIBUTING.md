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

The macOS VST3 development bundle additionally requires
`cargo-nice-plug 0.1.1` and the pinned media tools:

```sh
cargo install cargo-nice-plug --version 0.1.1 --locked
./scripts/prepare-tools-macos-arm64.sh
./scripts/bundle-macos.sh
```

## Before opening a pull request

Run the same checks as CI:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
(cd ui && npm test && npm run build)
```

If the editor changes, commit the regenerated `ui/dist/index.html`; it is
embedded into the plug-in binary at compile time. Do not commit downloaded
media tools, DAW projects, credentials, cookies, or rendered samples.

## Architectural boundaries

- Never perform network, process, filesystem, decoding, allocation, or locking
  work on the real-time audio callback.
- Keep acquisition in the Worker process and pass command arguments directly;
  do not interpolate shell commands.
- Keep VST3 types in the plug-in adapter rather than the reusable core.
- Treat native WAV drag and plug-in audio output as the DAW handoff contract;
  VST3 does not portably create host tracks or timeline clips.
- Keep the frontend bridge shell-agnostic so a future standalone Tauri shell
  can reuse the UI without making Tauri part of the VST runtime.

Provider changes are expected. Keep live-provider checks out of deterministic
tests and use fixture executables and local WAVs instead.
