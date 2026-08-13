[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent $PSScriptRoot
$toolDirectory = Join-Path $repoRoot "resources/tools"
$buildDirectory = Join-Path ([System.IO.Path]::GetTempPath()) ("rippr-tools-" + [guid]::NewGuid())

$ytDlpVersion = "2026.07.04"
$ytDlpSha256 = "52fe3c26dcf71fbdc85b528589020bb0b8e383155cfa81b64dd447bbe35e24b8"
$ffmpegRelease = "autobuild-2026-08-12-13-15"
$ffmpegAsset = "ffmpeg-n8.1.2-34-g9b6c8969e0-win64-lgpl-8.1.zip"
$ffmpegSha256 = "2c1363eaf5238c2bf6d1c4815b87fc305d161931ced08748fac634b6580e74e0"

New-Item -ItemType Directory -Force -Path $toolDirectory, $buildDirectory | Out-Null

function Get-VerifiedDownload {
    param(
        [Parameter(Mandatory)] [string] $Uri,
        [Parameter(Mandatory)] [string] $Destination,
        [Parameter(Mandatory)] [string] $Sha256
    )

    Invoke-WebRequest -Uri $Uri -OutFile $Destination
    $actualSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $Destination).Hash.ToLowerInvariant()
    if ($actualSha256 -ne $Sha256) {
        throw "SHA-256 mismatch for $Uri. Expected $Sha256, received $actualSha256."
    }
}

$ytDlpDownload = Join-Path $buildDirectory "yt-dlp.exe"
Get-VerifiedDownload `
    -Uri "https://github.com/yt-dlp/yt-dlp/releases/download/$ytDlpVersion/yt-dlp.exe" `
    -Destination $ytDlpDownload `
    -Sha256 $ytDlpSha256
Copy-Item -LiteralPath $ytDlpDownload -Destination (Join-Path $toolDirectory "yt-dlp.exe") -Force

$ffmpegArchive = Join-Path $buildDirectory $ffmpegAsset
$ffmpegExtracted = Join-Path $buildDirectory "ffmpeg"
Get-VerifiedDownload `
    -Uri "https://github.com/BtbN/FFmpeg-Builds/releases/download/$ffmpegRelease/$ffmpegAsset" `
    -Destination $ffmpegArchive `
    -Sha256 $ffmpegSha256
Expand-Archive -LiteralPath $ffmpegArchive -DestinationPath $ffmpegExtracted

$ffmpegExecutable = Get-ChildItem -LiteralPath $ffmpegExtracted -Filter "ffmpeg.exe" -File -Recurse | Select-Object -First 1
if (-not $ffmpegExecutable) {
    throw "The pinned FFmpeg archive did not contain ffmpeg.exe."
}
Copy-Item -LiteralPath $ffmpegExecutable.FullName -Destination (Join-Path $toolDirectory "ffmpeg.exe") -Force

$ffmpegLicense = Get-ChildItem -LiteralPath $ffmpegExtracted -File -Recurse |
    Where-Object { $_.Name -in @("LICENSE.txt", "COPYING.LGPLv2.1", "COPYING.LGPLv3") } |
    Select-Object -First 1
if ($ffmpegLicense) {
    Copy-Item -LiteralPath $ffmpegLicense.FullName -Destination (Join-Path $toolDirectory "FFmpeg-Windows-License.txt") -Force
}

& (Join-Path $toolDirectory "yt-dlp.exe") --version
if ($LASTEXITCODE -ne 0) { throw "The pinned yt-dlp executable failed its version check." }
& (Join-Path $toolDirectory "ffmpeg.exe") -version | Select-Object -First 1
if ($LASTEXITCODE -ne 0) { throw "The pinned FFmpeg executable failed its version check." }
