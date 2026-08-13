[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [ValidatePattern("^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$")]
    [string] $Version,

    [string] $OutputDirectory
)

$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent $PSScriptRoot
if (-not $OutputDirectory) {
    $OutputDirectory = Join-Path $repoRoot "dist"
}

foreach ($variable in @("RIPPR_WINDOWS_PFX_PATH", "RIPPR_WINDOWS_PFX_PASSWORD")) {
    if (-not [Environment]::GetEnvironmentVariable($variable)) {
        throw "$variable is required for Authenticode signing."
    }
}

$currentVersion = (& node (Join-Path $repoRoot "scripts/release-version.mjs") --check).Trim()
if ($currentVersion -ne $Version) {
    throw "Requested version $Version does not match repository version $currentVersion."
}

$cargoTruce = Get-Command cargo-truce -ErrorAction SilentlyContinue
if (-not $cargoTruce) {
    throw "Missing cargo-truce 6.3.0. Install it with: cargo install cargo-truce --version 6.3.0 --locked"
}

$toolDirectory = Join-Path $repoRoot "resources/tools"
foreach ($tool in @("yt-dlp.exe", "ffmpeg.exe")) {
    if (-not (Test-Path -LiteralPath (Join-Path $toolDirectory $tool) -PathType Leaf)) {
        throw "Missing resources/tools/$tool. Run scripts/prepare-tools-windows-x64.ps1 first."
    }
}

Push-Location (Join-Path $repoRoot "ui")
try {
    & npm ci
    if ($LASTEXITCODE -ne 0) { throw "npm ci failed." }
    & npm run build
    if ($LASTEXITCODE -ne 0) { throw "npm run build failed." }
} finally {
    Pop-Location
}

Push-Location $repoRoot
try {
    & cargo build --release -p rippr-worker
    if ($LASTEXITCODE -ne 0) { throw "rippr-worker release build failed." }
    & cargo truce build --vst3 -p rippr-plugin --target x86_64-pc-windows-msvc
    if ($LASTEXITCODE -ne 0) { throw "VST3 release build failed." }
} finally {
    Pop-Location
}

$bundle = Join-Path $repoRoot "target/bundles/Rippr.vst3"
$resourceDirectory = Join-Path $bundle "Contents/Resources"
New-Item -ItemType Directory -Force -Path $resourceDirectory | Out-Null

Copy-Item -LiteralPath (Join-Path $repoRoot "target/release/rippr-worker.exe") -Destination (Join-Path $resourceDirectory "rippr-worker.exe") -Force
Copy-Item -LiteralPath (Join-Path $toolDirectory "yt-dlp.exe") -Destination (Join-Path $resourceDirectory "yt-dlp.exe") -Force
Copy-Item -LiteralPath (Join-Path $toolDirectory "ffmpeg.exe") -Destination (Join-Path $resourceDirectory "ffmpeg.exe") -Force
Copy-Item -LiteralPath (Join-Path $repoRoot "THIRD_PARTY_NOTICES.md") -Destination $resourceDirectory -Force
Copy-Item -LiteralPath (Join-Path $repoRoot "THIRD_PARTY_LICENSES/Truce-License-1.0.txt") -Destination $resourceDirectory -Force
if (Test-Path -LiteralPath (Join-Path $toolDirectory "FFmpeg-Windows-License.txt")) {
    Copy-Item -LiteralPath (Join-Path $toolDirectory "FFmpeg-Windows-License.txt") -Destination $resourceDirectory -Force
}

$signtool = (Get-Command signtool.exe -ErrorAction SilentlyContinue).Source
if (-not $signtool) {
    $windowsKits = Join-Path ${env:ProgramFiles(x86)} "Windows Kits/10/bin"
    $signtool = Get-ChildItem -LiteralPath $windowsKits -Filter "signtool.exe" -File -Recurse |
        Sort-Object -Property FullName -Descending |
        Select-Object -ExpandProperty FullName -First 1
}
if (-not $signtool) {
    throw "signtool.exe was not found."
}

$signTargets = Get-ChildItem -LiteralPath $bundle -File -Recurse |
    Where-Object { $_.Extension -in @(".exe", ".dll", ".vst3") }
if (-not $signTargets) {
    throw "No signable binaries were found in Rippr.vst3."
}

foreach ($target in $signTargets) {
    & $signtool sign `
        /f $env:RIPPR_WINDOWS_PFX_PATH `
        /p $env:RIPPR_WINDOWS_PFX_PASSWORD `
        /fd SHA256 `
        /tr "http://timestamp.digicert.com" `
        /td SHA256 `
        $target.FullName
    if ($LASTEXITCODE -ne 0) { throw "Authenticode signing failed for $($target.FullName)." }
    & $signtool verify /pa /v $target.FullName
    if ($LASTEXITCODE -ne 0) { throw "Authenticode verification failed for $($target.FullName)." }
}

$productName = "Rippr-v$Version-Windows-x64"
$stagingDirectory = Join-Path ([System.IO.Path]::GetTempPath()) ("rippr-release-" + [guid]::NewGuid())
$payloadDirectory = Join-Path $stagingDirectory $productName
New-Item -ItemType Directory -Force -Path $payloadDirectory, $OutputDirectory | Out-Null
Copy-Item -LiteralPath $bundle -Destination (Join-Path $payloadDirectory "Rippr.vst3") -Recurse
Copy-Item -LiteralPath (Join-Path $repoRoot "packaging/INSTALL-Windows.txt") -Destination (Join-Path $payloadDirectory "INSTALL.txt")
Copy-Item -LiteralPath (Join-Path $repoRoot "LICENSE") -Destination (Join-Path $payloadDirectory "LICENSE.txt")
Copy-Item -LiteralPath (Join-Path $repoRoot "THIRD_PARTY_NOTICES.md") -Destination $payloadDirectory
Copy-Item -LiteralPath (Join-Path $repoRoot "THIRD_PARTY_LICENSES") -Destination $payloadDirectory -Recurse

$releaseArchive = Join-Path $OutputDirectory "$productName.zip"
Compress-Archive -LiteralPath $payloadDirectory -DestinationPath $releaseArchive -CompressionLevel Optimal -Force
$releaseSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $releaseArchive).Hash.ToLowerInvariant()
Set-Content -LiteralPath "$releaseArchive.sha256" -Value "$releaseSha256  $productName.zip" -Encoding ascii

Write-Output $releaseArchive
