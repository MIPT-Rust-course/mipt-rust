#!/usr/bin/env bash

if [ -z "$CI" ];
then
    echo "Not in CI. Exiting"
    exit 1
fi

echo "[INFO]   Installing cargo criterion"
cargo install cargo-criterion || exit 1
echo "[INFO]   Installing cargo criterion success"

echo "[INFO]   Setup returned no errors"
