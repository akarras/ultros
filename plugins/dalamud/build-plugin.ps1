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
    throw 'Published releases require a full Release build, without localhost support or reused native binaries.'
}

if (![System.Runtime.InteropServices.RuntimeInformation]::IsOSPlatform([System.Runtime.InteropServices.OSPlatform]::Windows)) {
    throw 'Build this Windows x64 plugin on Windows with the MSVC Rust toolchain.'
}
if ($DalamudHome) { $DalamudHome = (Resolve-Path -LiteralPath $DalamudHome).Path }

function Invoke-Checked([string]$Program, [string[]]$Arguments) {
    & $Program @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Program failed with exit code $LASTEXITCODE." }
}

function Publish-Plugin([string]$SourceDirectory, [string]$DestinationDirectory) {
    New-Item -ItemType Directory -Path $DestinationDirectory -Force | Out-Null
    $nativeSource = Join-Path $SourceDirectory 'ultros_list_doc_ffi.dll'
    $nativeDestination = Join-Path $DestinationDirectory 'ultros_list_doc_ffi.dll'
    $nativeUnchanged = (Test-Path -LiteralPath $nativeDestination) -and
        (Get-FileHash -Algorithm SHA256 -LiteralPath $nativeSource).Hash -eq
        (Get-FileHash -Algorithm SHA256 -LiteralPath $nativeDestination).Hash
    if ($nativeUnchanged) {
        Write-Output 'Native DLL unchanged; keeping the existing file for managed hot reload.'
    }
    else {
        $nativeInput = $null
        $nativeOutput = $null
        try {
            $nativeInput = [IO.File]::OpenRead($nativeSource)
            # Acquire the destination before truncating it. A loaded Rust DLL
            # must block this update before any managed files are published.
            $nativeOutput = [IO.File]::Open($nativeDestination, [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
            $nativeInput.CopyTo($nativeOutput)
            $nativeOutput.SetLength($nativeOutput.Position)
            $nativeOutput.Flush($true)
        }
        catch {
            throw "Could not update the Rust DLL. If the game has it loaded, exit the game and rerun this build using the same development path. $($_.Exception.Message)"
        }
        finally {
            if ($nativeOutput) { $nativeOutput.Dispose() }
            if ($nativeInput) { $nativeInput.Dispose() }
        }
    }

    # Complete dependencies and metadata before the watched plugin DLL. Rename
    # each complete file into place, then notify Dalamud with its final timestamp.
    foreach ($file in @('Ultros.Lists.dll', 'Ultros.deps.json', 'Ultros.json', 'capture-compatibility.json', 'build-info.json', 'README.md', 'PORTABILITY.md', 'LICENSE', 'Ultros.dll')) {
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
$target = 'x86_64-pc-windows-msvc'
$nativeConfiguration = $Configuration.ToLowerInvariant()
$buildLock = $null
Push-Location $repo
try {
    $initialSource = Get-PluginReleaseSource $repo
    if ($PublishRelease -and $initialSource.Dirty) { throw 'Commit and validate the release sources before publishing.' }
    New-Item -ItemType Directory -Path $artifactRoot -Force | Out-Null
    $buildLock = [IO.File]::Open((Join-Path $artifactRoot 'build.lock'), [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
    $installedInfo = $null
    if ($KeepInstalledNative) {
        # Explicit managed-only iteration: retest the exact resident ABI 2 DLL.
        # Never silently fall back to it when a normal native update is locked.
        $installedInfo = Get-Content -Raw -LiteralPath (Join-Path $package 'build-info.json') | ConvertFrom-Json
        $installedNative = Join-Path $package 'ultros_list_doc_ffi.dll'
        if ($installedInfo.NativeAbi -ne 2 -or $installedInfo.Target -ne $target -or
            $installedInfo.Configuration -ne $Configuration -or $installedInfo.CpuBaseline -ne 'x86-64' -or
            $installedInfo.PanicStrategy -ne 'unwind' -or
            $installedInfo.NativeSha256 -ne (Get-FileHash -Algorithm SHA256 -LiteralPath $installedNative).Hash) {
            throw 'The installed native library does not match the expected package. Run a full build with the game closed.'
        }
    }
    $metadataJson = & cargo metadata --locked --no-deps --format-version 1
    if ($LASTEXITCODE -ne 0) { throw 'Could not locate the Cargo output directory.' }
    $metadata = $metadataJson | ConvertFrom-Json
    $cargoArguments = @('build', '--locked', '-p', 'ultros-list-doc-ffi', '--target', $target)
    if ($Configuration -eq 'Release') { $cargoArguments += '--release' }
    $previousNativeFlags=$env:CARGO_ENCODED_RUSTFLAGS
    try {
        # Keep the game DLL portable across x64 machines and Wine translators,
        # even if this shell normally uses hotter server RUSTFLAGS.
        $env:CARGO_ENCODED_RUSTFLAGS=(@('--cfg=web_sys_unstable_apis','--cfg=erase_components','-C','target-cpu=x86-64','-C','panic=unwind') -join [char]0x1f)
        if (!$KeepInstalledNative) { Invoke-Checked 'cargo' $cargoArguments }
        Invoke-Checked 'cargo' @('test', '--locked', '-p', 'ultros-list-doc', '-p', 'ultros-list-doc-ffi', '--tests', '--target', $target)
    }
    finally { $env:CARGO_ENCODED_RUSTFLAGS=$previousNativeFlags }

    Push-Location $PSScriptRoot
    try {
        $dotnetArguments = @('build', 'Ultros/Ultros.csproj', '--configuration', $Configuration, '-p:RestoreLockedMode=true', '--nologo')
        if ($EnableLocalhost) { $dotnetArguments += '-p:UltrosLocalhostDebug=true' }
        if ($DalamudHome) {
            $references = (Resolve-Path -LiteralPath $DalamudHome).Path
            $dotnetArguments += "-p:DALAMUD_HOME=$references"
        }
        Invoke-Checked 'dotnet' $dotnetArguments

        # Test away from the watched directory, then publish to the same path
        # on every build so Dalamud can hot reload the managed plugin.
        New-Item -ItemType Directory -Path $staging -Force | Out-Null
        $managed = Join-Path $PSScriptRoot "Ultros/bin/$Configuration"
        foreach ($file in @('Ultros.dll', 'Ultros.json', 'Ultros.deps.json', 'Ultros.Lists.dll')) {
            Copy-Item -LiteralPath (Join-Path $managed $file) -Destination $staging
        }
        $native = if ($KeepInstalledNative) { $installedNative } else { Join-Path $metadata.target_directory "$target/$nativeConfiguration/ultros_list_doc_ffi.dll" }
        if ($KeepInstalledNative) { Write-Output 'Keeping the installed native library for managed-only hot reload; running interop checks against it.' }
        Copy-Item -LiteralPath $native -Destination $staging
        $referenceDirectory = if ($DalamudHome) { (Resolve-Path -LiteralPath $DalamudHome).Path } else { Join-Path $env:APPDATA 'XIVLauncher/addon/Hooks/dev' }
        $compatibility = @{ ClientStructsSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $referenceDirectory 'FFXIVClientStructs.dll')).Hash }
        $compatibility | ConvertTo-Json | Set-Content -Encoding utf8 -LiteralPath (Join-Path $staging 'capture-compatibility.json')
        $finalSource = Get-PluginReleaseSource $repo
        $buildInfo=@{
            PluginVersion=([xml](Get-Content -Raw (Join-Path $PSScriptRoot "Directory.Build.props"))).Project.PropertyGroup.UltrosPluginVersion; NativeAbi=2; ServerProtocol=1; Target=$target; Configuration=$Configuration
            LocalhostEnabled=($Configuration -eq 'Debug' -or $EnableLocalhost.IsPresent)
            RustVersion=$(if ($KeepInstalledNative) { $installedInfo.RustVersion } else { & rustc --version }); DotnetSdk=(& dotnet --version)
            ReusedInstalledNative=$KeepInstalledNative.IsPresent
            SourceCommit=$initialSource.Commit
            SourceDirty=($initialSource.Dirty -or $finalSource.Dirty -or $initialSource.Commit -ne $finalSource.Commit)
            ClientStructsSha256=$compatibility.ClientStructsSha256
            DalamudVersion=[Reflection.AssemblyName]::GetAssemblyName((Join-Path $referenceDirectory 'Dalamud.dll')).Version.ToString(4)
            NativeSha256=(Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $staging 'ultros_list_doc_ffi.dll')).Hash
            CpuBaseline='x86-64'; PanicStrategy='unwind'
        }
        $buildInfo | ConvertTo-Json | Set-Content -Encoding utf8 -LiteralPath (Join-Path $staging 'build-info.json')
        Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'README.md') -Destination $staging
        Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'PORTABILITY.md') -Destination $staging
        Copy-Item -LiteralPath (Join-Path $repo 'LICENSE') -Destination $staging

        # Run the real managed/native tests against the exact DLL being packaged.
        $smokeArguments = @('run', '--project', 'Ultros.Lists.Smoke/Ultros.Lists.Smoke.csproj', '--configuration', $Configuration)
        if ($EnableLocalhost) { $smokeArguments += '-p:UltrosLocalhostDebug=true' }
        Invoke-Checked 'dotnet' ($smokeArguments + @('--', $staging))
        Invoke-Checked 'dotnet' ($smokeArguments + @('--', '--native-shadow', $staging, (Join-Path $artifactRoot 'native-shadow-smoke')))
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
