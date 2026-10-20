# Keep build provenance and publication checks on the same source boundary.
# All tracked files (including toolchain/build configuration and transitive
# workspace dependencies) and nonignored untracked files belong to that boundary.
# Git-ignored output such as target/, bin/, obj/ and artifacts/ is intentional.
function Get-PluginReleaseSource([string]$Repository) {
    $commit = & git -C $Repository rev-parse --verify HEAD
    if ($LASTEXITCODE -ne 0) { throw 'Could not identify the release commit.' }
    $status = & git -C $Repository status --porcelain --untracked-files=all --ignore-submodules=none
    if ($LASTEXITCODE -ne 0) { throw 'Could not check the release sources.' }
    [pscustomobject]@{
        Commit = $commit.Trim()
        Dirty = [bool]$status
    }
}
