$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot 'check-updates.ps1')

$current = Get-DalamudUpdate '15.0.0' '15.0.3.9' '15.0.3.9'
if (!$current.Compatible -or !$current.ReferencesCurrent -or $current.ApiLevel -ne 15) { throw 'Matching stable references were rejected.' }
$stale = Get-DalamudUpdate '15.0.0' '15.0.3.9' '15.0.3.8'
if (!$stale.Compatible -or $stale.ReferencesCurrent) { throw 'A same-API Dalamud update was missed.' }
$api = Get-DalamudUpdate '15.0.0' '16.0.0.0' '15.0.3.9'
if ($api.Compatible -or $api.ApiLevel -ne 16) { throw 'A new stable API did not require migration.' }
foreach ($invalid in @('', 'latest', '15.0.3.9/other', '015.0.3.9', '8.0.0.0')) {
    $rejected = $false
    try { Get-DalamudUpdate '15.0.0' $invalid '' | Out-Null } catch { $rejected = $true }
    if (!$rejected) { throw "Invalid upstream version accepted: $invalid" }
}
Write-Output 'PASS: current/stale references, stable API migration and malformed upstream version checks.'
