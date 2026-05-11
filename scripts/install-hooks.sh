#!/usr/bin/env bash
# One-shot: point git at the tracked hooks under scripts/hooks/.
# Re-run safe.
set -e

repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"

chmod +x scripts/hooks/pre-commit scripts/hooks/pre-push 2>/dev/null || true
git config core.hooksPath scripts/hooks

echo "Hooks installed: core.hooksPath=scripts/hooks"
echo "  pre-commit  -> cargo fmt --all -- --check"
echo "  pre-push    -> ./check_ci.sh (fmt + clippy)"
echo
echo "To bypass once: git commit --no-verify  /  git push --no-verify"
echo "To uninstall:   git config --unset core.hooksPath"
