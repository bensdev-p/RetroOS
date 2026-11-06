#!/bin/sh
# Every headless-Station scenario, as CI runs them (make test-host, ci.yml).
#   tests/host/run-all.sh [OUT_DIR]   (default: out) — screenshots in OUT_DIR/test-host*/
# Needs the binaries built first (the cargo line below); X11 scenario needs
# xwayland and x11-apps installed.
set -eu
OUT=${1:-out}
BIN=target/debug
cargo build -p station -p retro-console -p retrodos -p retro-login -p retro-edit -p retro-filer \
    -p retro-net -p retro-bar -p retro-progman -p retro-about -p retro-jot -p retro-cabinet
run() { name=$1; shift; echo ">> host test: $name"; python3 tests/host/station.py "$BIN" "$OUT/test-host$name" "$@"; }
# Program Manager only where a scenario is about it.
export RETROOS_NO_PROGMAN=1
run ""
run -launch tests/host/launch.steps
run -edit tests/host/edit.steps
run -filer tests/host/filer.steps
run -tasklist tests/host/tasklist.steps
RETROOS_NO_BAR=1 run -desktop tests/host/desktop.steps
RETROOS_NO_BAR=1 STATION_ARGS="--personality desktop" run -desktop-start tests/host/desktop-start.steps
run -bar tests/host/bar.steps
run -scheme tests/host/scheme.steps
RETROOS_NO_PROGMAN= run -progman tests/host/progman.steps
run -about tests/host/about.steps
RETROOS_NO_BAR=1 run -jot tests/host/jot.steps
RETROOS_NO_BAR=1 run -cabinet tests/host/cabinet.steps
RETROOS_NO_BAR=1 run -x11 tests/host/x11.steps
echo ">> host tests: all passed"
