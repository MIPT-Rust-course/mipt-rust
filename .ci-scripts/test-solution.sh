#!/usr/bin/env bash

cd "$PROBLEM_ROOT"

cargo fmt -- --check || exit 1
cargo clippy -- -D warnings || exit 1
cargo test || exit 1
cargo criterion || exit 1

echo "[INFO]   Testing returned no errors"
