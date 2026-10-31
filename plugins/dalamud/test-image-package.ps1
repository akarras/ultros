$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot 'prepare-image-package.ps1')
$fixture = Join-Path ([IO.Path]::GetTempPath()) ('ultros-image-package-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $fixture | Out-Null
$version = '1.2.3.4'
$commit = 'a' * 40
$native = 'synthetic native library'
$nativeHash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($native)))
$build = @{
    SourceDirty=$false; LocalhostEnabled=$false; ReusedInstalledNative=$false; Configuration='Release'
    SourceCommit=$commit; PluginVersion=$version; ServerProtocol=1; NativeAbi=2
    NativeSha256=$nativeHash; Target='x86_64-pc-windows-msvc'; ClientStructsSha256=('b' * 64); DalamudVersion='15.0.3.9'
}
$manifest = @{ InternalName='Ultros'; AssemblyVersion=$version; DalamudApiLevel=15 }
$capture = @{ ClientStructsSha256=('b' * 64) }
function Write-Package {
    $zipPath = Join-Path $fixture 'Ultros.zip'
    $stream = [IO.File]::Open($zipPath, [IO.FileMode]::Create)
    $archive = [IO.Compression.ZipArchive]::new($stream, [IO.Compression.ZipArchiveMode]::Create)
    try {
        $files = @{
            'Ultros.dll'='managed'; 'Ultros.Lists.dll'='managed'; 'Ultros.deps.json'='{}'
            'ultros_list_doc_ffi.dll'=$native; 'README.md'='test'; 'PORTABILITY.md'='test'; 'LICENSE'='test'
            'build-info.json'=($build | ConvertTo-Json); 'Ultros.json'=($manifest | ConvertTo-Json)
            'capture-compatibility.json'=($capture | ConvertTo-Json)
        }
        foreach ($name in $files.Keys) {
            $entry = $archive.CreateEntry($name)
            $bytes = [Text.Encoding]::UTF8.GetBytes($files[$name])
            $output = $entry.Open()
            try { $output.Write($bytes) } finally { $output.Dispose() }
        }
    } finally { $archive.Dispose() }
    @{
        version=$version; protocol=1; native_abi=2; dalamud_api_level=15; published_at=1
        commit=$commit; sha256=(Get-FileHash -Algorithm SHA256 -LiteralPath $zipPath).Hash.ToLowerInvariant()
        size=(Get-Item -LiteralPath $zipPath).Length; changelog='Synthetic fixture'
    } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $fixture 'release.json') -Encoding utf8NoBOM
}
function Assert-Rejected([scriptblock]$Action, [string]$Reason) {
    $rejected = $false
    try { & $Action | Out-Null } catch { $rejected = $true }
    if (!$rejected) { throw $Reason }
}
function Invoke-FixtureGit([string[]]$Arguments) {
    & git -C $fixture @Arguments | Out-Null
    if ($LASTEXITCODE) { throw 'Fixture Git command failed.' }
}
try {
    Write-Package
    $null = Assert-ImagePluginPackage $fixture $version
    Assert-Rejected { Assert-ImagePluginPackage $fixture '../1.2.3.4' } 'A path was accepted as a release version.'
    [IO.File]::AppendAllText((Join-Path $fixture 'Ultros.zip'), 'changed')
    Assert-Rejected { Assert-ImagePluginPackage $fixture $version } 'An altered immutable ZIP was accepted.'
    foreach ($flag in @('SourceDirty','LocalhostEnabled','ReusedInstalledNative')) {
        $build[$flag] = $true
        Write-Package
        Assert-Rejected { Assert-ImagePluginPackage $fixture $version } "Nonproduction flag accepted: $flag"
        $build[$flag] = $false
    }
    foreach ($property in @('SourceCommit','NativeSha256','ClientStructsSha256')) {
        $original = $build[$property]
        $build[$property] = 'c' * $original.Length
        Write-Package
        Assert-Rejected { Assert-ImagePluginPackage $fixture $version } "Mismatched package metadata accepted: $property"
        $build[$property] = $original
    }
    Write-Package
    $refs = Join-Path $fixture 'references'
    New-Item -ItemType Directory -Path $refs | Out-Null
    Copy-Item -LiteralPath (Join-Path $PSHOME 'System.Management.Automation.dll') -Destination (Join-Path $refs 'Dalamud.dll')
    [IO.File]::WriteAllText((Join-Path $refs 'FFXIVClientStructs.dll'), 'different references')
    Assert-Rejected { Assert-ImagePluginPackage $fixture $version $refs } 'Stale Dalamud references were accepted.'

    # A web-only commit reuses the package; a dependency/native/managed change
    # must get a new version even when its protocol and API stay the same.
    # The fixture is a two-crate workspace like the real one: the plugin's
    # native library and a server crate sharing Cargo.toml and Cargo.lock.
    function Write-FixtureFile([string]$Path, [string]$Text) {
        $full = Join-Path $fixture $Path
        New-Item -ItemType Directory -Path (Split-Path $full) -Force | Out-Null
        [IO.File]::WriteAllText($full, $Text)
    }
    function Save-FixtureCommit([string]$Message) {
        Invoke-FixtureGit @('add','--all')
        Invoke-FixtureGit @('-c','user.name=Package fixture','-c','user.email=fixture@example.invalid','-c','commit.gpgsign=false','commit','--quiet','-m',$Message)
    }
    $workspace = "[workspace]`nmembers = ['ultros-list-doc-ffi', 'server']`nresolver = '3'`n`n[profile.release]`nopt-level = 'z'`n"
    $toolchain = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot '../../rust-toolchain')
    if ($toolchain -notmatch '(?m)^channel = "([^"]+)"') { throw 'Could not read the pinned toolchain.' }
    Invoke-FixtureGit @('init','--quiet')
    Write-FixtureFile '.gitignore' "target/`n"
    Write-FixtureFile 'rust-toolchain.toml' "[toolchain]`nchannel = `"$($Matches[1])`"`n"
    Write-FixtureFile 'Cargo.toml' $workspace
    Write-FixtureFile 'ultros-list-doc-ffi/Cargo.toml' "[package]`nname = 'ultros-list-doc-ffi'`nversion = '0.1.0'`nedition = '2024'`n"
    Write-FixtureFile 'ultros-list-doc-ffi/src/lib.rs' "pub fn answer() -> u32 { 42 }`n"
    Write-FixtureFile 'server/Cargo.toml' "[package]`nname = 'server'`nversion = '0.1.0'`nedition = '2024'`n"
    Write-FixtureFile 'server/src/main.rs' "fn main() {}`n"
    Write-FixtureFile 'plugins/dalamud/input.cs' 'original'
    Push-Location -LiteralPath $fixture
    try { & cargo generate-lockfile --offline --quiet; if ($LASTEXITCODE) { throw 'Could not lock the fixture workspace.' } } finally { Pop-Location }
    Save-FixtureCommit 'Original package'
    $sourceCommit = (& git -C $fixture rev-parse HEAD).Trim()
    Write-FixtureFile 'website.txt' 'web-only change'
    Save-FixtureCommit 'Website change'
    Assert-ImagePluginSource $fixture $sourceCommit
    # Server-only edits to the shared workspace manifest do not touch the
    # native build, and check-only scripts are not packaged, so the published
    # package is reused.
    Write-FixtureFile 'Cargo.toml' ($workspace + "`n[profile.server-release]`ninherits = 'release'`nopt-level = 3`n")
    Write-FixtureFile 'server/src/main.rs' "fn main() { println!(`"server change`"); }`n"
    Write-FixtureFile 'plugins/dalamud/test-policy.ps1' 'check-only script'
    Save-FixtureCommit 'Server-only workspace change'
    Assert-ImagePluginSource $fixture $sourceCommit
    $rejections = [ordered]@{
        'Cargo.toml' = @(($workspace -replace "opt-level = 'z'", 'opt-level = 3'), 'A release-profile change reused an old native library.')
        'ultros-list-doc-ffi/src/lib.rs' = @("pub fn answer() -> u32 { 43 }`n", 'A native source change reused an old package.')
        'plugins/dalamud/input.cs' = @('modified', 'Changed plugin inputs reused an old package.')
    }
    $accepted = (& git -C $fixture rev-parse HEAD).Trim()
    foreach ($path in $rejections.Keys) {
        Write-FixtureFile $path $rejections[$path][0]
        Save-FixtureCommit "Change $path"
        $message = $null
        try { Assert-ImagePluginSource $fixture $sourceCommit } catch { $message = $_.Exception.Message }
        if ($message -notmatch 'Bump UltrosPluginVersion') { throw "$($rejections[$path][1]) ($message)" }
        Invoke-FixtureGit @('reset','--quiet','--hard',$accepted)
    }
    Write-Output 'PASS: immutable ZIP integrity, clean production provenance, stable-reference mismatch and package source/version policy.'
} finally {
    $resolved = [IO.Path]::GetFullPath($fixture)
    $tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
    if (!$resolved.StartsWith($tempRoot, [StringComparison]::OrdinalIgnoreCase) -or
        (Split-Path $resolved -Leaf) -notmatch '^ultros-image-package-[0-9a-f]{32}$') { throw 'Refusing to clean an unexpected fixture path.' }
    Remove-Item -LiteralPath $resolved -Recurse -Force
}
# Actions' pwsh wrapper exits with LASTEXITCODE. The expected git-diff failure
# above must not turn a successful fixture into a failed workflow step.
$global:LASTEXITCODE = 0
