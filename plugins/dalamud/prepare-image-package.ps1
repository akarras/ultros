param(
    [string]$DalamudHome,
    [string]$Repository = 'akarras/ultros',
    [string]$OutputDirectory = (Join-Path $PSScriptRoot 'distribution')
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot 'release-source.ps1')

function Assert-ImagePluginVersion([string]$Version) {
    if ($Version -notmatch '^(0|[1-9][0-9]{0,4})\.(0|[1-9][0-9]{0,4})\.(0|[1-9][0-9]{0,4})\.(0|[1-9][0-9]{0,4})$' -or
        @($Version.Split('.') | Where-Object { [int]$_ -gt 65534 }).Count) { throw 'Invalid plugin release version.' }
}

function Assert-ImagePluginPackage([string]$Directory, [string]$Version, [string]$References) {
    Assert-ImagePluginVersion $Version
    $release = Get-Content -Raw -LiteralPath (Join-Path $Directory 'release.json') | ConvertFrom-Json
    $zip = Get-Item -LiteralPath (Join-Path $Directory 'Ultros.zip')
    if ($release.version -cne $Version -or $release.protocol -ne 1 -or $release.native_abi -ne 2 -or
        $release.dalamud_api_level -ne 15 -or $release.published_at -le 0 -or
        $release.commit -notmatch '^[a-f0-9]{40}$' -or $release.sha256 -notmatch '^[a-f0-9]{64}$' -or
        $zip.Length -le 0 -or $zip.Length -gt 128MB -or $zip.Length -ne $release.size -or
        (Get-FileHash -Algorithm SHA256 -LiteralPath $zip.FullName).Hash -ine $release.sha256) {
        throw 'The plugin release index and ZIP do not match.'
    }
    $archive = [IO.Compression.ZipFile]::OpenRead($zip.FullName)
    try {
        $expected = @('Ultros.dll','Ultros.json','Ultros.deps.json','Ultros.Lists.dll','ultros_list_doc_ffi.dll','capture-compatibility.json','build-info.json','README.md','PORTABILITY.md','LICENSE') | Sort-Object
        if (@(Compare-Object $expected @($archive.Entries.FullName | Sort-Object)).Count) { throw 'Unexpected plugin ZIP contents.' }
        foreach ($entry in $archive.Entries) {
            if ($entry.Length -le 0 -or $entry.Length -gt 128MB) { throw 'Unexpected plugin ZIP entry size.' }
        }
        function Read-EntryJson([string]$Name) {
            $entry = $archive.GetEntry($Name)
            if ($entry.Length -gt 64KB) { throw 'Plugin metadata is too large.' }
            $reader = [IO.StreamReader]::new($entry.Open())
            try { $reader.ReadToEnd() | ConvertFrom-Json } finally { $reader.Dispose() }
        }
        $build = Read-EntryJson 'build-info.json'
        $manifest = Read-EntryJson 'Ultros.json'
        $capture = Read-EntryJson 'capture-compatibility.json'
        $native = $archive.GetEntry('ultros_list_doc_ffi.dll').Open()
        try { $nativeHash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($native)) } finally { $native.Dispose() }
        if ($build.SourceDirty -or $build.LocalhostEnabled -or $build.ReusedInstalledNative -or
            $build.Configuration -cne 'Release' -or $build.SourceCommit -cne $release.commit -or
            $build.PluginVersion -cne $Version -or $build.ServerProtocol -ne 1 -or $build.NativeAbi -ne 2 -or
            $build.NativeSha256 -ine $nativeHash -or $build.Target -cne 'x86_64-pc-windows-msvc' -or
            $build.ClientStructsSha256 -ine $capture.ClientStructsSha256 -or
            $manifest.InternalName -cne 'Ultros' -or $manifest.AssemblyVersion -cne $Version -or $manifest.DalamudApiLevel -ne 15) {
            throw 'The plugin release is not a clean, tested production package.'
        }
        if ($References) {
            $dalamud = [Reflection.AssemblyName]::GetAssemblyName((Join-Path $References 'Dalamud.dll')).Version.ToString(4)
            $structs = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $References 'FFXIVClientStructs.dll')).Hash
            if ($build.DalamudVersion -cne $dalamud -or $build.ClientStructsSha256 -ine $structs) {
                throw 'The published plugin uses older Dalamud references. Bump UltrosPluginVersion and rebuild with the checked stable references.'
            }
        }
    } finally { $archive.Dispose() }
    return $release
}

$script:NativePackage = 'ultros-list-doc-ffi'
$script:NativeTarget = 'x86_64-pc-windows-msvc'

# Cargo's resolved build plan for the plugin's native library: every crate it
# compiles with its version, features, target and profile settings, plus the
# Git tree of each workspace crate among them. Paths are made relative to the
# checkout so two checkouts of the same sources produce the same plan.
# Requires the pinned nightly (`-Z unstable-options --unit-graph`).
function Get-NativeBuildPlan([string]$Root) {
    Push-Location -LiteralPath $Root
    try {
        $json = & cargo build --locked --quiet -Z unstable-options --unit-graph -p $script:NativePackage --release --target $script:NativeTarget
        if ($LASTEXITCODE) { throw 'Could not compute the native library build plan.' }
    } finally { Pop-Location }
    $graph = ($json -join "`n") | ConvertFrom-Json
    $rootPath = [IO.Path]::GetFullPath($Root)
    $packages = @{}
    $ids = @(foreach ($unit in $graph.units) {
        $id = $unit.pkg_id
        if ($id -match '^path\+(file:///[^#]+)#(.+)$') {
            $relative = [IO.Path]::GetRelativePath($rootPath, ([Uri]$Matches[1]).LocalPath).Replace('\', '/')
            if ($relative.StartsWith('..')) { throw "Native build uses a crate outside the repository: $relative" }
            $tree = (& git -C $Root rev-parse "HEAD:$relative").Trim()
            if ($LASTEXITCODE) { throw "Could not hash workspace crate $relative." }
            $packages[$relative] = $true
            $id = "path:$relative@$tree#$($Matches[2])"
        }
        $id
    })
    $lines = for ($index = 0; $index -lt $graph.units.Count; $index++) {
        $unit = $graph.units[$index]
        [ordered]@{
            package = $ids[$index]; mode = $unit.mode; platform = $unit.platform
            target = [ordered]@{ name = $unit.target.name; kind = $unit.target.kind; crate_types = $unit.target.crate_types; edition = $unit.target.edition }
            profile = $unit.profile; features = @($unit.features)
            dependencies = @($unit.dependencies | ForEach-Object { "$($_.extern_crate_name)=$($ids[$_.index])" } | Sort-Object)
        } | ConvertTo-Json -Compress -Depth 8
    }
    [pscustomobject]@{
        Hash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes((($lines | Sort-Object) -join "`n"))))
        WorkspaceCrates = @($packages.Keys | Sort-Object)
    }
}

function Assert-ImagePluginSource([string]$Root, [string]$Commit) {
    if ($Commit -notmatch '^[a-f0-9]{40}$') { throw 'Invalid package source commit.' }
    & git -C $Root cat-file -e "$Commit`^{commit}"
    if ($LASTEXITCODE) { throw 'Package source commit is missing. Fetch the complete repository history and release tags.' }
    $changed = 'Plugin package inputs changed after publication. Bump UltrosPluginVersion instead of replacing the published version.'
    # Changing any package input requires an explicit version bump. Ordinary
    # website-only commits reuse the exact published ZIP, including provenance.
    # The fixtures and this script only check packages; they are not in the ZIP.
    & git -C $Root diff --quiet $Commit HEAD -- plugins/dalamud ':(exclude)plugins/dalamud/test-*.ps1' ':(exclude)plugins/dalamud/prepare-image-package.ps1' rust-toolchain rust-toolchain.toml .cargo LICENSE integration/fixtures/list-compatibility.json
    if ($LASTEXITCODE) { throw $changed }
    # The workspace Cargo.toml and Cargo.lock are shared with the server, so a
    # line diff would demand a plugin release for server-only dependency or
    # profile edits. Compare what the native build compiles instead.
    $current = Get-NativeBuildPlan $Root
    & git -C $Root diff --quiet $Commit HEAD -- Cargo.toml Cargo.lock @($current.WorkspaceCrates)
    if (!$LASTEXITCODE) { return }
    $published = Join-Path ([IO.Path]::GetTempPath()) ('ultros-plugin-source-' + [Guid]::NewGuid().ToString('N'))
    $previousSmudge = $env:GIT_LFS_SKIP_SMUDGE
    $env:GIT_LFS_SKIP_SMUDGE = '1'
    try {
        & git -C $Root worktree add --quiet --detach $published $Commit
        if ($LASTEXITCODE) { throw 'Could not check out the published package source.' }
        $previous = Get-NativeBuildPlan $published
    } finally {
        $env:GIT_LFS_SKIP_SMUDGE = $previousSmudge
        & git -C $Root worktree remove --force $published 2>$null
        if (Test-Path -LiteralPath $published) { Remove-Item -LiteralPath $published -Recurse -Force }
    }
    if ($previous.Hash -cne $current.Hash) { throw $changed }
}

# Fixtures import the validators without network access or publication.
if ($MyInvocation.InvocationName -eq '.') { return }
if (!$DalamudHome) { throw 'Pass the freshly checked stable Dalamud reference directory.' }
$DalamudHome = (Resolve-Path -LiteralPath $DalamudHome).Path
$root = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '../..')).Path
if ((Get-PluginReleaseSource $root).Dirty) { throw 'Commit and validate the image/package sources first.' }
$version = ([xml](Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot 'Directory.Build.props'))).Project.PropertyGroup.UltrosPluginVersion
Assert-ImagePluginVersion $version
$tag = "dalamud-v$version"
$cache = Join-Path $root "artifacts/dalamud/image-$([Guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Path $cache -Force | Out-Null

function Get-HostedRelease([string]$TagName) {
    $response = & gh api "repos/$Repository/releases/tags/$TagName" 2>&1
    if ($LASTEXITCODE -ne 0) {
        if (($response -join "`n") -match '\(HTTP 404\)') { return $null }
        throw 'Could not check the hosted plugin release. Refusing to replace it.'
    }
    $response -join "`n" | ConvertFrom-Json
}
function Download-HostedPackage([string]$TagName, [string]$Destination) {
    New-Item -ItemType Directory -Path $Destination -Force | Out-Null
    & gh release download $TagName --repo $Repository --pattern Ultros.zip --pattern release.json --dir $Destination
    if ($LASTEXITCODE) { throw 'Could not download the immutable plugin package.' }
}

$hosted = Get-HostedRelease $tag
if ($hosted) {
    if ($hosted.draft) { throw 'The plugin release is still a draft.' }
    $package = Join-Path $cache $version
    Download-HostedPackage $tag $package
} else {
    & (Join-Path $PSScriptRoot 'build-plugin.ps1') -Configuration Release -StageOnly -DalamudHome $DalamudHome
    & (Join-Path $PSScriptRoot 'publish-release.ps1') -ReleaseDirectory $cache
    $package = Join-Path $cache $version
}
$release = Assert-ImagePluginPackage $package $version $DalamudHome
Assert-ImagePluginSource $root $release.commit
New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
function Copy-ImagePackage([string]$Source, [string]$Version) {
    $destination = Join-Path $OutputDirectory $Version
    New-Item -ItemType Directory -Path $destination -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $Source 'Ultros.zip'), (Join-Path $Source 'release.json') -Destination $destination -Force
}
Copy-ImagePackage $package $version
# Retain old versioned URLs across image replacements, even after a new plugin
# version becomes current. Old releases need not match today's game structs.
$tags = & gh api --paginate "repos/$Repository/releases?per_page=100" --jq '.[] | select(.draft == false and (.tag_name | startswith("dalamud-v"))) | .tag_name'
if ($LASTEXITCODE) { throw 'Could not enumerate previous plugin releases.' }
foreach ($oldTag in $tags) {
    $oldVersion = $oldTag.Substring('dalamud-v'.Length)
    Assert-ImagePluginVersion $oldVersion
    if ($oldVersion -eq $version) { continue }
    $previous = Join-Path $cache $oldVersion
    Download-HostedPackage $oldTag $previous
    $null = Assert-ImagePluginPackage $previous $oldVersion
    Copy-ImagePackage $previous $oldVersion
}
Copy-Item -LiteralPath (Join-Path $package 'release.json') -Destination (Join-Path $OutputDirectory 'current.json') -Force
Write-Output "Image package ready: Ultros $version, SHA-256 $($release.sha256)."
