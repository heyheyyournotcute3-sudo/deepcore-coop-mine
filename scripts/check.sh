#!/usr/bin/env bash
# Deepcore's quality gate: format, lint, test. Run before every commit.
set -euo pipefail

cd "$(dirname "$0")/.."

echo "== cargo fmt =="
cargo fmt --all -- --check || {
    echo "formatting issues found; run 'cargo fmt --all'"
    exit 1
}

echo "== cargo clippy =="
cargo clippy --workspace --all-targets -- -D warnings

echo "== cargo test =="
cargo test --workspace

echo "all checks passed"
