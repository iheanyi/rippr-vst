#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
tool_directory="$repo_root/resources/tools"
bundle="$repo_root/target/bundled/rippr-vst.vst3"
resource_directory="$bundle/Contents/Resources"

if ! command -v cargo-nice-plug >/dev/null 2>&1; then
  echo "Missing cargo-nice-plug 0.1.1. Install it with: cargo install cargo-nice-plug --version 0.1.1 --locked" >&2
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
(cd "$repo_root" && cargo nice-plug bundle rippr-plugin --release)

mkdir -p "$resource_directory"
install -m 755 "$repo_root/target/release/rippr-worker" "$resource_directory/rippr-worker"
install -m 755 "$tool_directory/yt-dlp" "$resource_directory/yt-dlp"
install -m 755 "$tool_directory/ffmpeg" "$resource_directory/ffmpeg"
install -m 644 "$repo_root/THIRD_PARTY_NOTICES.md" "$resource_directory/THIRD_PARTY_NOTICES.md"

for executable in rippr-worker yt-dlp ffmpeg; do
  codesign --force --sign - "$resource_directory/$executable"
done
codesign --force --deep --sign - "$bundle"
codesign --verify --deep --strict --verbose=2 "$bundle"

echo "$bundle"
