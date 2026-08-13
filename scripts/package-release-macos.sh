#!/bin/sh
set -eu

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
version=${1:-}
output_directory=${2:-"$repo_root/dist"}

if [ -z "$version" ] || [ "$#" -gt 2 ]; then
  echo "Usage: $0 <version> [output-directory]" >&2
  exit 1
fi

if [ "${RIPPR_CODESIGN_IDENTITY:-}" = "" ] || [ "$RIPPR_CODESIGN_IDENTITY" = "-" ]; then
  echo "RIPPR_CODESIGN_IDENTITY must name a Developer ID Application certificate." >&2
  exit 1
fi

for variable in APPLE_ID APPLE_TEAM_ID APPLE_APP_PASSWORD; do
  eval "value=\${$variable:-}"
  if [ -z "$value" ]; then
    echo "$variable is required for notarization." >&2
    exit 1
  fi
done

current_version=$(node "$repo_root/scripts/release-version.mjs" --check)
if [ "$current_version" != "$version" ]; then
  echo "Requested version $version does not match repository version $current_version." >&2
  exit 1
fi

staging_directory=$(mktemp -d)
trap 'trash "$staging_directory"' EXIT HUP INT TERM
product_name="Rippr-v$version-macOS-arm64"
payload_directory="$staging_directory/$product_name"
submission_archive="$staging_directory/$product_name-notarization.zip"
release_archive="$output_directory/$product_name.zip"

mkdir -p "$payload_directory" "$output_directory"
if [ "${RIPPR_VALIDATE_RELEASE:-0}" = "1" ]; then
  (cd "$repo_root" && cargo truce validate --pluginval --auval -p rippr-plugin)
fi
"$repo_root/scripts/bundle-macos.sh"

for bundle_name in Rippr.vst3 Rippr.component Rippr.app; do
  ditto "$repo_root/target/bundles/$bundle_name" "$payload_directory/$bundle_name"
done
install -m 644 "$repo_root/packaging/INSTALL-macOS.txt" "$payload_directory/INSTALL.txt"
install -m 644 "$repo_root/LICENSE" "$payload_directory/LICENSE.txt"
install -m 644 "$repo_root/THIRD_PARTY_NOTICES.md" "$payload_directory/THIRD_PARTY_NOTICES.md"
ditto "$repo_root/THIRD_PARTY_LICENSES" "$payload_directory/THIRD_PARTY_LICENSES"

ditto -c -k --sequesterRsrc --keepParent "$payload_directory" "$submission_archive"
xcrun notarytool submit "$submission_archive" \
  --apple-id "$APPLE_ID" \
  --team-id "$APPLE_TEAM_ID" \
  --password "$APPLE_APP_PASSWORD" \
  --wait

for bundle_name in Rippr.vst3 Rippr.component Rippr.app; do
  bundle="$payload_directory/$bundle_name"
  xcrun stapler staple "$bundle"
  xcrun stapler validate "$bundle"
  codesign --verify --deep --strict --verbose=2 "$bundle"
done

ditto -c -k --sequesterRsrc --keepParent "$payload_directory" "$release_archive"
(
  cd "$output_directory"
  shasum -a 256 "$(basename "$release_archive")" > "$(basename "$release_archive").sha256"
)

echo "$release_archive"
