#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
tool_directory="$repo_root/resources/tools"
bundle_directory="$repo_root/target/bundles"
install_bundles=0

if [ "${1:-}" = "--install" ]; then
  install_bundles=1
elif [ "$#" -ne 0 ]; then
  echo "Usage: $0 [--install]" >&2
  exit 1
fi

if ! command -v cargo-truce >/dev/null 2>&1; then
  echo "Missing cargo-truce 6.3.0. Install it with: cargo install cargo-truce --version 6.3.0 --locked" >&2
  exit 1
fi

for tool in yt-dlp ffmpeg; do
  if [ ! -x "$tool_directory/$tool" ]; then
    echo "Missing $tool_directory/$tool. Run scripts/prepare-tools-macos-arm64.sh first." >&2
    exit 1
  fi
done

(cd "$repo_root/ui" && npm ci && npm run build)
(cd "$repo_root" && cargo build --release -p rippr-worker)
(cd "$repo_root" && cargo truce build --vst3 --au2 -p rippr-plugin)
(cd "$repo_root" && cargo truce run -p rippr-plugin -- --help >/dev/null)

for bundle in \
  "$bundle_directory/Rippr.vst3" \
  "$bundle_directory/Rippr.component" \
  "$bundle_directory/Rippr.app"; do
  case "$bundle" in
    *.app) plugin_executable="rippr-vst-standalone" ;;
    *) plugin_executable="Rippr" ;;
  esac
  resource_directory="$bundle/Contents/Resources"
  mkdir -p "$resource_directory"
  install -m 755 "$repo_root/target/release/rippr-worker" "$resource_directory/rippr-worker"
  install -m 755 "$tool_directory/yt-dlp" "$resource_directory/yt-dlp"
  install -m 755 "$tool_directory/ffmpeg" "$resource_directory/ffmpeg"
  install -m 644 "$repo_root/THIRD_PARTY_NOTICES.md" "$resource_directory/THIRD_PARTY_NOTICES.md"
  install -m 644 "$repo_root/THIRD_PARTY_LICENSES/Truce-License-1.0.txt" "$resource_directory/Truce-License-1.0.txt"

  for executable in rippr-worker yt-dlp ffmpeg; do
    codesign --force --sign - "$resource_directory/$executable"
  done
  codesign --force --sign - "$bundle/Contents/MacOS/$plugin_executable"
  codesign --force --sign - "$bundle"
  codesign --verify --deep --strict --verbose=2 "$bundle"
done

if [ "$install_bundles" -eq 1 ]; then
  vst3_directory="$HOME/Library/Audio/Plug-Ins/VST3"
  component_directory="$HOME/Library/Audio/Plug-Ins/Components"
  application_directory="$HOME/Applications"
  mkdir -p "$vst3_directory" "$component_directory" "$application_directory"
  ditto "$bundle_directory/Rippr.vst3" "$vst3_directory/Rippr.vst3"
  ditto "$bundle_directory/Rippr.component" "$component_directory/Rippr.component"
  ditto "$bundle_directory/Rippr.app" "$application_directory/Rippr.app"
  echo "Installed Rippr.vst3, Rippr.component, and Rippr.app for the current user."
fi

echo "$bundle_directory/Rippr.vst3"
echo "$bundle_directory/Rippr.component"
echo "$bundle_directory/Rippr.app"
