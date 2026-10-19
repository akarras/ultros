param(
    [ValidateRange(1024,65535)][int]$Port=8080,
    [switch]$TestAuth
)
$ErrorActionPreference='Stop'
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$env:HOSTNAME="http://localhost:$Port"
$env:PORT="$Port"
$env:LEPTOS_SITE_ADDR="127.0.0.1:$Port"
$env:ULTROS_PLUGIN_ENABLED='true'
$features='plugin-localhost'
if($TestAuth){$features+=',test-auth'}
Push-Location $repo
try{
    # DATABASE_URL and the ordinary Discord/cookie configuration come from
    # your development environment or .env. This script never rewrites them.
    $arguments=@('leptos','serve','--bin-features',$features)
    if($IsWindows){$arguments+='--bin-cargo-args=--config=profile.dev.package.ultros-app.debug=0'}
    & cargo @arguments
    if($LASTEXITCODE -ne 0){throw 'The local Ultros server stopped with an error.'}
}finally{Pop-Location}
