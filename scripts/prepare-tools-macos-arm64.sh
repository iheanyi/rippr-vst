#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
tool_directory="$repo_root/resources/tools"
build_directory=$(mktemp -d)
ffmpeg_version=8.1.2
ffmpeg_source_sha=464beb5e7bf0c311e68b45ae2f04e9cc2af88851abb4082231742a74d97b524c
yt_dlp_version=2026.07.04
yt_dlp_sha=498bd0dae17855c599d371d68ec5bafc439a9d8640e838be25c765a9792f261b

mkdir -p "$tool_directory"

curl --fail --location --silent --show-error \
  "https://github.com/yt-dlp/yt-dlp/releases/download/$yt_dlp_version/yt-dlp_macos" \
  --output "$tool_directory/yt-dlp"
printf '%s  %s\n' "$yt_dlp_sha" "$tool_directory/yt-dlp" | shasum -a 256 -c
chmod +x "$tool_directory/yt-dlp"

curl --fail --location --silent --show-error \
  "https://ffmpeg.org/releases/ffmpeg-$ffmpeg_version.tar.xz" \
  --output "$build_directory/ffmpeg.tar.xz"
printf '%s  %s\n' "$ffmpeg_source_sha" "$build_directory/ffmpeg.tar.xz" | shasum -a 256 -c
tar -xf "$build_directory/ffmpeg.tar.xz" -C "$build_directory"

ffmpeg_source="$build_directory/ffmpeg-$ffmpeg_version"
(
  cd "$ffmpeg_source"
  ./configure \
    --disable-everything \
    --disable-doc \
    --disable-debug \
    --disable-network \
    --disable-autodetect \
    --disable-iconv \
    --enable-ffmpeg \
    --disable-ffprobe \
    --disable-ffplay \
    --enable-avcodec \
    --enable-avformat \
    --enable-avfilter \
    --enable-swresample \
    --enable-protocol=file \
    --enable-demuxer=mov,matroska,ogg,mp3,flac,wav \
    --enable-parser=aac,flac,mpegaudio,opus,vorbis \
    --enable-decoder=aac,alac,flac,mp3,mp3float,opus,vorbis,pcm_f32le,pcm_f64le,pcm_s16be,pcm_s16le,pcm_s24be,pcm_s24le,pcm_s32be,pcm_s32le \
    --enable-encoder=pcm_f32le \
    --enable-muxer=wav \
    --enable-filter=aformat,aresample,anull
  make -j"$(sysctl -n hw.logicalcpu)" ffmpeg
)
install -m 755 "$ffmpeg_source/ffmpeg" "$tool_directory/ffmpeg"

"$tool_directory/yt-dlp" --version
"$tool_directory/ffmpeg" -version | head -n 1
