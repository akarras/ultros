param(
    [switch]$DownloadReferences,
    [string]$ReferenceDirectory
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Get-DalamudUpdate([string]$SdkVersion, [string]$LatestVersion, [string]$ReferenceVersion) {
    foreach ($value in @($SdkVersion, $LatestVersion)) {
        if ($value -notmatch '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(\.(0|[1-9][0-9]*))?$') {
            throw 'Dalamud SDK/distribution returned an invalid version.'
        }
    }
    $sdk = [version]$SdkVersion
    $latest = [version]$LatestVersion
    if ($latest.Major -lt 9) { throw 'Unsupported Dalamud version/API mapping.' }
    [pscustomobject]@{
        SdkVersion = $SdkVersion
        LatestVersion = $LatestVersion
        ApiLevel = $latest.Major
        Compatible = $sdk.Major -eq $latest.Major
        ReferencesCurrent = $ReferenceVersion -eq $LatestVersion
    }
}

# The fixture imports the version policy without network or filesystem writes.
if ($MyInvocation.InvocationName -eq '.') { return }

$repo = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$project = [xml](Get-Content -Raw (Join-Path $PSScriptRoot 'Ultros/Ultros.csproj'))
if ($project.Project.Sdk -notmatch '^Dalamud.NET.Sdk/(.+)$') { throw 'Cannot find the plugin Dalamud SDK pin.' }
$sdkVersion = $Matches[1]

# Pin metadata and binaries to one official stable distribution commit. A
# release moving while this check runs cannot mix two generations of references.
$remote = & git ls-remote --exit-code https://github.com/goatcorp/dalamud-distrib.git HEAD
if ($LASTEXITCODE -ne 0 -or $remote -isnot [string] -or $remote -notmatch '^([0-9a-f]{40})\s+HEAD$') {
    throw 'Could not resolve the official stable Dalamud distribution.'
}
$commit = $Matches[1]
$distribution = "https://raw.githubusercontent.com/goatcorp/dalamud-distrib/$commit"
$metadata = Invoke-RestMethod -Uri "$distribution/version" -TimeoutSec 30
$update = Get-DalamudUpdate $sdkVersion $metadata.AssemblyVersion ''
if (!$update.Compatible) {
    throw "Stable Dalamud $($update.LatestVersion) requires API $($update.ApiLevel); this plugin pins SDK $sdkVersion. Update the SDK, API-level checks, references and any affected readers, then rebuild and test."
}

if ($DownloadReferences) {
    if (!$ReferenceDirectory) { $ReferenceDirectory = Join-Path $repo ('artifacts/dalamud/references/' + [guid]::NewGuid().ToString('N')) }
    $ReferenceDirectory = [IO.Path]::GetFullPath($ReferenceDirectory)
    if ((Test-Path -LiteralPath $ReferenceDirectory) -and (Get-ChildItem -LiteralPath $ReferenceDirectory -Force | Select-Object -First 1)) {
        throw 'Download references into an empty directory, separate from the live Dalamud installation.'
    }
    New-Item -ItemType Directory -Path $ReferenceDirectory -Force | Out-Null
    $archive = Join-Path $ReferenceDirectory 'references.pending.zip'
    try {
        Invoke-WebRequest -Uri "$distribution/latest.zip" -OutFile $archive -TimeoutSec 180
        Expand-Archive -LiteralPath $archive -DestinationPath $ReferenceDirectory
    }
    finally { if (Test-Path -LiteralPath $archive) { Remove-Item -LiteralPath $archive } }
} elseif (!$ReferenceDirectory) {
    $ReferenceDirectory = Join-Path $env:APPDATA 'XIVLauncher/addon/Hooks/dev'
}
$referenceVersion = [Reflection.AssemblyName]::GetAssemblyName((Join-Path $ReferenceDirectory 'Dalamud.dll')).Version.ToString(4)
$update = Get-DalamudUpdate $sdkVersion $metadata.AssemblyVersion $referenceVersion
if (!$update.ReferencesCurrent) {
    throw "Stable Dalamud is $($update.LatestVersion), but these references are $referenceVersion. Run this check with -DownloadReferences, then pass that directory to build-plugin.ps1 -DalamudHome."
}
$structsHash = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $ReferenceDirectory 'FFXIVClientStructs.dll')).Hash
Write-Output "Stable Dalamud $referenceVersion (API $($update.ApiLevel)); SDK $sdkVersion; distribution $commit."
Write-Output "FFXIVClientStructs SHA-256: $structsHash"
Write-Output "References: $([IO.Path]::GetFullPath($ReferenceDirectory))"
