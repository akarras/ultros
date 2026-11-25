param(
    [ValidateSet('Debug', 'Release')]
    [string]$Configuration = 'Debug',
    [string]$DalamudHome,
    [switch]$EnableLocalhost,
    [switch]$KeepInstalledNative,
    [switch]$PublishRelease,
    [switch]$StageOnly
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot 'release-source.ps1')
if ($PublishRelease -and $StageOnly) { throw 'Choose staging or release publication.' }
if ($PublishRelease -and ($Configuration -ne 'Release' -or $EnableLocalhost -or $KeepInstalledNative)) {
    throw 'Published releases require a full Release build, without localhost support or reused document modules.'
}

if (![System.Runtime.InteropServices.RuntimeInformation]::IsOSPlatform([System.Runtime.InteropServices.OSPlatform]::Windows)) {
    throw 'Build this Dalamud plugin package on Windows.'
}
if ($DalamudHome) { $DalamudHome = (Resolve-Path -LiteralPath $DalamudHome).Path }

function Invoke-Checked([string]$Program, [string[]]$Arguments) {
    & $Program @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Program failed with exit code $LASTEXITCODE." }
}

function Publish-Plugin([string]$SourceDirectory, [string]$DestinationDirectory) {
    New-Item -ItemType Directory -Path $DestinationDirectory -Force | Out-Null
    # Clean up any legacy native DLL left behind by older dev builds.
    $legacyNative = Join-Path $DestinationDirectory 'ultros_list_doc_ffi.dll'
    if (Test-Path -LiteralPath $legacyNative) {
        try { Remove-Item -LiteralPath $legacyNative -Force }
        catch { Write-Warning "Could not remove legacy ultros_list_doc_ffi.dll (exit the game once to remove it): $($_.Exception.Message)" }
    }

    # Complete dependencies and metadata before the watched plugin DLL. Rename
    # each complete file into place, then notify Dalamud with its final timestamp.
    foreach ($file in @('WebAssembly.dll', 'UltrosListDoc.WasmIL.dll', 'Ultros.Lists.dll', 'Ultros.deps.json', 'Ultros.json', 'capture-compatibility.json', 'build-info.json', 'README.md', 'PORTABILITY.md', 'LICENSE', 'Ultros.dll')) {
        $destination = Join-Path $DestinationDirectory $file
        $temporary = "$destination.pending"
        [IO.File]::Copy((Join-Path $SourceDirectory $file), $temporary, $true)
        [IO.File]::Move($temporary, $destination, $true)
    }
    [IO.File]::SetLastWriteTimeUtc((Join-Path $DestinationDirectory 'Ultros.dll'), [DateTime]::UtcNow)
}

$repo = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$artifactRoot = Join-Path $repo 'artifacts/dalamud'
$package = Join-Path $artifactRoot 'dev'
$staging = Join-Path $artifactRoot '.staging-plugin'
$wasmTarget = 'wasm32-wasip1'
$hostTarget = 'x86_64-pc-windows-msvc'
$rustProfileDir = $Configuration.ToLowerInvariant()
$pluginVersion = ([xml](Get-Content -Raw (Join-Path $PSScriptRoot 'Directory.Build.props'))).Project.PropertyGroup.UltrosPluginVersion
$buildLock = $null
Push-Location $repo
try {
    $initialSource = Get-PluginReleaseSource $repo
    if ($PublishRelease -and $initialSource.Dirty) { throw 'Commit and validate the release sources before publishing.' }
    New-Item -ItemType Directory -Path $artifactRoot -Force | Out-Null
    $buildLock = [IO.File]::Open((Join-Path $artifactRoot 'build.lock'), [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
    $installedInfo = $null
    $installedModule = $null
    if ($KeepInstalledNative) {
        # Explicit managed-only iteration: reuse and retest the installed WasmIL module.
        $installedInfo = Get-Content -Raw -LiteralPath (Join-Path $package 'build-info.json') | ConvertFrom-Json
        $installedModule = Join-Path $package 'UltrosListDoc.WasmIL.dll'
        if ($installedInfo.NativeAbi -ne 2 -or $installedInfo.Target -ne $wasmTarget -or
            $installedInfo.Configuration -ne $Configuration -or
            !(Test-Path -LiteralPath $installedModule) -or
            $installedInfo.NativeSha256 -ne (Get-FileHash -Algorithm SHA256 -LiteralPath $installedModule).Hash) {
            throw 'The installed WasmIL module does not match the expected package. Run a full build.'
        }
    }
    if (!$KeepInstalledNative) {
        $installedTargets = & rustup target list --installed
        if ($LASTEXITCODE -ne 0 -or !($installedTargets -contains $wasmTarget)) {
            throw "Rust target '$wasmTarget' is not installed. Run: rustup target add $wasmTarget"
        }
    }
    $metadataJson = & cargo metadata --locked --no-deps --format-version 1
    if ($LASTEXITCODE -ne 0) { throw 'Could not locate the Cargo output directory.' }
    $metadata = $metadataJson | ConvertFrom-Json

    $previousFlags = $env:CARGO_ENCODED_RUSTFLAGS
    try {
        if (!$KeepInstalledNative) {
            # Cap WASM linear memory at 128 MiB (2048 pages), well above the 32 MiB
            # decompression budget while preventing unbounded growth inside the game.
            $env:CARGO_ENCODED_RUSTFLAGS = (@('--cfg=web_sys_unstable_apis', '--cfg=erase_components', '-C', 'link-arg=--max-memory=134217728') -join [char]0x1f)
            Invoke-Checked 'cargo' @('clippy', '--locked', '-p', 'ultros-list-doc-ffi', '--target', $wasmTarget, '--lib', '--', '-D', 'warnings')
            $cargoArguments = @('build', '--locked', '-p', 'ultros-list-doc-ffi', '--target', $wasmTarget)
            if ($Configuration -eq 'Release') { $cargoArguments += '--release' }
            Invoke-Checked 'cargo' $cargoArguments
        }
        # Run all shared CRDT and FFI unit/convergence tests on the host target.
        $env:CARGO_ENCODED_RUSTFLAGS = (@('--cfg=web_sys_unstable_apis', '--cfg=erase_components', '-C', 'target-cpu=x86-64', '-C', 'panic=unwind') -join [char]0x1f)
        Invoke-Checked 'cargo' @('test', '--locked', '-p', 'ultros-list-doc', '-p', 'ultros-list-doc-ffi', '--tests', '--target', $hostTarget)
    }
    finally { $env:CARGO_ENCODED_RUSTFLAGS = $previousFlags }

    Push-Location $PSScriptRoot
    try {
        $dotnetArguments = @('build', 'Ultros/Ultros.csproj', '--configuration', $Configuration, '-p:RestoreLockedMode=true', '--nologo')
        if ($EnableLocalhost) { $dotnetArguments += '-p:UltrosLocalhostDebug=true' }
        if ($DalamudHome) {
            $references = (Resolve-Path -LiteralPath $DalamudHome).Path
            $dotnetArguments += "-p:DALAMUD_HOME=$references"
        }
        Invoke-Checked 'dotnet' $dotnetArguments

        # Rebuild staging cleanly so stale legacy files cannot leak into a package.
        if (Test-Path -LiteralPath $staging) { Remove-Item -LiteralPath $staging -Recurse -Force }
        New-Item -ItemType Directory -Path $staging -Force | Out-Null
        $managed = Join-Path $PSScriptRoot "Ultros/bin/$Configuration"
        foreach ($file in @('Ultros.dll', 'Ultros.json', 'Ultros.deps.json', 'Ultros.Lists.dll', 'WebAssembly.dll')) {
            Copy-Item -LiteralPath (Join-Path $managed $file) -Destination $staging
        }
        $wasmModuleDestination = Join-Path $staging 'UltrosListDoc.WasmIL.dll'
        if ($KeepInstalledNative) {
            Write-Output 'Keeping the installed WasmIL module for managed-only iteration; running interop checks against it.'
            Copy-Item -LiteralPath $installedModule -Destination $wasmModuleDestination
        } else {
            $wasmBinary = Join-Path $metadata.target_directory "$wasmTarget/$rustProfileDir/ultros_list_doc_ffi.wasm"
            Invoke-Checked 'dotnet' @('run', '--project', 'Ultros.Lists.WasmCompiler/Ultros.Lists.WasmCompiler.csproj', '--configuration', 'Release', '--', $wasmBinary, $wasmModuleDestination, $pluginVersion)
        }

        $referenceDirectory = if ($DalamudHome) { (Resolve-Path -LiteralPath $DalamudHome).Path } else { Join-Path $env:APPDATA 'XIVLauncher/addon/Hooks/dev' }
        $compatibility = @{ ClientStructsSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $referenceDirectory 'FFXIVClientStructs.dll')).Hash }
        $compatibility | ConvertTo-Json | Set-Content -Encoding utf8 -LiteralPath (Join-Path $staging 'capture-compatibility.json')
        $finalSource = Get-PluginReleaseSource $repo
        $buildInfo = @{
            PluginVersion = $pluginVersion; NativeAbi = 2; ServerProtocol = 1; Target = $wasmTarget; Configuration = $Configuration
            LocalhostEnabled = ($Configuration -eq 'Debug' -or $EnableLocalhost.IsPresent)
            RustVersion = $(if ($KeepInstalledNative) { $installedInfo.RustVersion } else { & rustc --version }); DotnetSdk = (& dotnet --version)
            ReusedInstalledNative = $KeepInstalledNative.IsPresent
            SourceCommit = $initialSource.Commit
            SourceDirty = ($initialSource.Dirty -or $finalSource.Dirty -or $initialSource.Commit -ne $finalSource.Commit)
            ClientStructsSha256 = $compatibility.ClientStructsSha256
            DalamudVersion = [Reflection.AssemblyName]::GetAssemblyName((Join-Path $referenceDirectory 'Dalamud.dll')).Version.ToString(4)
            NativeSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $wasmModuleDestination).Hash
            WebAssemblySha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $staging 'WebAssembly.dll')).Hash
            CpuBaseline = 'managed-il'; PanicStrategy = 'wasm-trap'
        }
        $buildInfo | ConvertTo-Json | Set-Content -Encoding utf8 -LiteralPath (Join-Path $staging 'build-info.json')
        Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'README.md') -Destination $staging
        Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'PORTABILITY.md') -Destination $staging
        Copy-Item -LiteralPath (Join-Path $repo 'LICENSE') -Destination $staging

        # Run the full managed document, UI, and collectible-ALC / trap checks against the exact staged package.
        $smokeArguments = @('run', '--project', 'Ultros.Lists.Smoke/Ultros.Lists.Smoke.csproj', '--configuration', $Configuration)
        if ($EnableLocalhost) { $smokeArguments += '-p:UltrosLocalhostDebug=true' }
        Invoke-Checked 'dotnet' ($smokeArguments + @('--', $staging))
        Invoke-Checked 'dotnet' @('run', '--project', 'Ultros.Ui.Smoke/Ultros.Ui.Smoke.csproj', '--configuration', $Configuration, "-p:DALAMUD_HOME=$referenceDirectory", '--', (Join-Path $artifactRoot 'ui'))
        Invoke-Checked 'dotnet' ($smokeArguments + @('--', '--wasm-runtime', $staging, (Join-Path $artifactRoot 'wasm-runtime-smoke')))
        if ($PublishRelease) {
            & (Join-Path $PSScriptRoot 'publish-release.ps1') -PackageDirectory $staging
        } elseif ($StageOnly) {
            Write-Output "Tested package staging: $staging"
        } else {
            Publish-Plugin $staging $package
            $archive = "$package.zip"
            Compress-Archive -Path (Join-Path $staging '*') -DestinationPath $archive -Force
            Write-Output "Plugin DLL: $(Join-Path $package 'Ultros.dll')"
            Write-Output "Package: $archive"
        }
    }
    finally { Pop-Location }
}
finally {
    if ($buildLock) { $buildLock.Dispose() }
    Pop-Location
}
