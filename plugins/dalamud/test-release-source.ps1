# Standalone regression test. All Git writes are confined to a temporary fixture.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot 'release-source.ps1')

$fixture = Join-Path ([IO.Path]::GetTempPath()) ('ultros-release-source-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $fixture | Out-Null
function Invoke-FixtureGit([string[]]$Arguments) {
    & git -C $fixture @Arguments | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Fixture Git command failed.' }
}
function Assert-SourceDirty([bool]$Expected, [string]$Reason) {
    if ((Get-PluginReleaseSource $fixture).Dirty -ne $Expected) { throw $Reason }
}
try {
    Invoke-FixtureGit @('init', '--quiet')
    $inputs = @('ultros-api-types/src/plugin.rs', '.cargo/config.toml', 'rust-toolchain.toml', 'plugins/dalamud/global.json', 'Cargo.lock')
    foreach ($relative in $inputs) {
        $path = Join-Path $fixture $relative
        New-Item -ItemType Directory -Path (Split-Path $path) -Force | Out-Null
        [IO.File]::WriteAllText($path, "fixture`n")
    }
    [IO.File]::WriteAllText((Join-Path $fixture '.gitignore'), "target/`nartifacts/`n**/bin/`n**/obj/`n")
    Invoke-FixtureGit @('add', '--all')
    Invoke-FixtureGit @('-c', 'user.name=Release fixture', '-c', 'user.email=fixture@example.invalid', '-c', 'commit.gpgsign=false', 'commit', '--quiet', '-m', 'Synthetic release inputs')
    Assert-SourceDirty $false 'The clean fixture must be publishable.'
    foreach ($relative in $inputs) {
        $path = Join-Path $fixture $relative
        [IO.File]::WriteAllText($path, "changed`n")
        Assert-SourceDirty $true "Modified build input was missed: $relative"
        [IO.File]::WriteAllText($path, "fixture`n")
    }
    $newInput = Join-Path $fixture 'ultros-api-types/src/new.rs'
    [IO.File]::WriteAllText($newInput, "new dependency input`n")
    Assert-SourceDirty $true 'An untracked dependency source must block publication.'
    Remove-Item -LiteralPath $newInput
    foreach ($relative in @('target/native.dll', 'artifacts/dalamud/build.lock', 'plugins/dalamud/Ultros/bin/Ultros.dll', 'plugins/dalamud/Ultros/obj/project.assets.json')) {
        $path = Join-Path $fixture $relative
        New-Item -ItemType Directory -Path (Split-Path $path) -Force | Out-Null
        [IO.File]::WriteAllText($path, 'ignored output')
    }
    Assert-SourceDirty $false 'Ignored build outputs must not block publication.'
    # Staged edits must also be detected; status must inspect both index and tree.
    [IO.File]::WriteAllText((Join-Path $fixture $inputs[0]), "staged change`n")
    Invoke-FixtureGit @('add', '--', $inputs[0])
    Assert-SourceDirty $true 'A staged dependency change must block publication.'
    Write-Output 'PASS: dependency/toolchain changes and untracked/staged sources are dirty; ignored build outputs are allowed.'
}
finally {
    $resolved = [IO.Path]::GetFullPath($fixture)
    $temporaryRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
    if (!$resolved.StartsWith($temporaryRoot, [StringComparison]::OrdinalIgnoreCase) -or
        (Split-Path $resolved -Leaf) -notmatch '^ultros-release-source-[0-9a-f]{32}$') {
        throw 'Refusing to clean an unexpected fixture path.'
    }
    Remove-Item -LiteralPath $resolved -Recurse -Force
}
