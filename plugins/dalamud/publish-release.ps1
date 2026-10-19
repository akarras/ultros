param(
    [string]$PackageDirectory = (Join-Path $PSScriptRoot '../../artifacts/dalamud/.staging-plugin'),
    [string]$ReleaseDirectory = (Join-Path $PSScriptRoot '../../artifacts/dalamud/releases'),
    [string]$Changelog = 'Ultros lists, market prices, retainer listings and game notifications.'
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ([string]::IsNullOrWhiteSpace($Changelog) -or [Text.Encoding]::UTF8.GetByteCount($Changelog) -gt 4096) {
    throw 'Release notes must contain between 1 and 4096 UTF-8 bytes.'
}

$package = (Resolve-Path -LiteralPath $PackageDirectory).Path
$build = Get-Content -Raw -LiteralPath (Join-Path $package 'build-info.json') | ConvertFrom-Json
$manifest = Get-Content -Raw -LiteralPath (Join-Path $package 'Ultros.json') | ConvertFrom-Json
$version = ([xml](Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot 'Directory.Build.props'))).Project.PropertyGroup.UltrosPluginVersion
if ($version -notmatch '^(0|[1-9][0-9]{0,4})\.(0|[1-9][0-9]{0,4})\.(0|[1-9][0-9]{0,4})\.(0|[1-9][0-9]{0,4})$' -or
    @($version.Split('.') | Where-Object { [int]$_ -gt 65534 }).Count -ne 0) { throw 'Use a four-part release version, with components no greater than 65534.' }
if ($build.LocalhostEnabled -or $build.Configuration -ne 'Release' -or $build.ReusedInstalledNative) { throw 'Development, localhost-enabled and reused-native packages cannot be published.' }
if ($build.PluginVersion -ne $version -or $manifest.AssemblyVersion -ne $version -or $manifest.InternalName -ne 'Ultros' -or
    $manifest.Name -ne 'Ultros' -or $build.ServerProtocol -ne 1 -or $build.NativeAbi -ne 2 -or $manifest.DalamudApiLevel -ne 15) {
    throw 'Package identity, version, API level or protocol does not match the release source.'
}
if ([Reflection.AssemblyName]::GetAssemblyName((Join-Path $package 'Ultros.dll')).Version.ToString(4) -ne $version -or
    [Reflection.AssemblyName]::GetAssemblyName((Join-Path $package 'Ultros.Lists.dll')).Version.ToString(4) -ne $version) {
    throw 'Managed assembly versions do not match the release.'
}
if ((Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $package 'ultros_list_doc_ffi.dll')).Hash -ne $build.NativeSha256) {
    throw 'The native library differs from the tested package.'
}
$sourceStatus = & git -C (Join-Path $PSScriptRoot '../..') status --porcelain --untracked-files=normal -- plugins/dalamud ultros-list-doc ultros-list-doc-ffi vendor/loro-kv-store Cargo.toml Cargo.lock
if ($LASTEXITCODE -ne 0 -or $sourceStatus) { throw 'Commit and validate the release sources before publishing.' }
$commit = & git -C (Join-Path $PSScriptRoot '../..') rev-parse HEAD
if ($LASTEXITCODE -ne 0) { throw 'Could not identify the release commit.' }
if ($build.SourceDirty -or $build.SourceCommit -ne $commit) { throw 'Rebuild from the clean release commit before publishing.' }

New-Item -ItemType Directory -Path $ReleaseDirectory -Force | Out-Null
$root = (Resolve-Path -LiteralPath $ReleaseDirectory).Path
$lock = [IO.File]::Open((Join-Path $root '.publish.lock'), [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
$pending = Join-Path $root "Ultros-$version.pending.zip"
try {
    $files = @('Ultros.dll','Ultros.json','Ultros.deps.json','Ultros.Lists.dll','ultros_list_doc_ffi.dll','capture-compatibility.json','build-info.json','README.md','PORTABILITY.md','LICENSE') | Sort-Object
    $stream = [IO.File]::Open($pending, [IO.FileMode]::Create, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
    try {
        $archive = [IO.Compression.ZipArchive]::new($stream, [IO.Compression.ZipArchiveMode]::Create, $true)
        try {
            foreach ($name in $files) {
                $file = Get-Item -LiteralPath (Join-Path $package $name)
                if (!$file.Length -or $file.Length -gt 128MB) { throw "Unexpected package file size: $name" }
                $entry = $archive.CreateEntry($name, [IO.Compression.CompressionLevel]::Optimal)
                $entry.LastWriteTime = [DateTimeOffset]::new(2000,1,1,0,0,0,[TimeSpan]::Zero)
                $input = [IO.File]::OpenRead($file.FullName)
                $output = $entry.Open()
                try { $input.CopyTo($output) } finally { $input.Dispose(); $output.Dispose() }
            }
        } finally { $archive.Dispose() }
        $stream.Flush($true)
    } finally { $stream.Dispose() }
    $size = (Get-Item -LiteralPath $pending).Length
    if ($size -gt 128MB) { throw 'The release package exceeds the download limit.' }
    $sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $pending).Hash.ToLowerInvariant()
    $destination = Join-Path $root $version
    New-Item -ItemType Directory -Path $destination -Force | Out-Null
    $zip = Join-Path $destination 'Ultros.zip'
    $index = Join-Path $destination 'release.json'
    if (Test-Path -LiteralPath $zip) {
        if ((Get-FileHash -Algorithm SHA256 -LiteralPath $zip).Hash.ToLowerInvariant() -ne $sha256 -or !(Test-Path -LiteralPath $index)) {
            throw 'That version is already published with different content. Bump the version instead of replacing it.'
        }
        $release = Get-Content -Raw -LiteralPath $index | ConvertFrom-Json
        if ($release.version -ne $version -or $release.sha256 -ne $sha256 -or $release.size -ne $size -or
            $release.commit -ne $commit -or $release.protocol -ne 1 -or $release.native_abi -ne 2 -or
            $release.dalamud_api_level -ne 15 -or $release.published_at -le 0) {
            throw 'The immutable release index does not match this package. Restore its original index before publishing.'
        }
        Remove-Item -LiteralPath $pending
    } else {
        $release = [ordered]@{
            version=$version; protocol=1; native_abi=2; dalamud_api_level=15
            published_at=[DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
            sha256=$sha256; size=$size; commit=$commit; changelog=$Changelog
        }
        [IO.File]::Move($pending, $zip, $false)
        $release | ConvertTo-Json | Set-Content -LiteralPath "$index.pending" -Encoding utf8NoBOM
        [IO.File]::Move("$index.pending", $index, $false)
    }
    $release | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $root 'current.json.pending') -Encoding utf8NoBOM
    [IO.File]::Move((Join-Path $root 'current.json.pending'), (Join-Path $root 'current.json'), $true)
    Write-Output "Published Ultros $version ($sha256). Mount this release directory read-only in the web server."
} finally { $lock.Dispose() }
