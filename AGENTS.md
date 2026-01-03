# Agent Instructions

This repository enforces strict CI checks. Before committing any code, you **must** run the `check_ci.sh` script located in the root directory.

## Instructions

1.  **Run `./check_ci.sh`** after making changes.
2.  **Fix any errors** reported by the script.
    - If `cargo fmt` fails, run `cargo fmt --all` to fix formatting automatically.
    - If `cargo clippy` fails, address the warnings/errors in your code.
3.  **Do not commit** until `./check_ci.sh` passes successfully.

Failure to follow these steps will result in CI failures.
