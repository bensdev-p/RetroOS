#!/bin/sh
# Every headless-Station scenario, as CI runs them (make test-host, ci.yml).
#   tests/host/run-all.sh [OUT_DIR]   (default: out) — screenshots in OUT_DIR/test-host*/
# Needs the binaries built first (the cargo line below); X11 scenario needs
# xwayland and x11-apps installed.
set -eu
OUT=${1:-out}
BIN=target/debug
cargo build -p station -p retro-console -p retrodos -p retro-login -p retro-edit -p retro-filer \
    -p retro-net -p retro-bar -p retro-progman -p retro-about -p retro-jot -p retro-cabinet -p retro-control -p retro-mixer -p retro-bt -p retro-library -p retro-media -p retro-arcade
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
run -mono tests/host/mono.steps
RETROOS_TEST_SOUND=1 run -sound tests/host/sound.steps
RETROOS_TEST_BT=1 run -bt tests/host/bt.steps
RETROOS_TEST_SOFTWARE=1 RETROOS_UPDATES_STATE="$PWD/tests/host/updates.state" run -library tests/host/library.steps
RETROOS_TEST_SOFTWARE=1 RETROOS_TEST_MEDIA=1 run -media tests/host/media.steps
RETROOS_NO_PROGMAN= run -progman tests/host/progman.steps
run -about tests/host/about.steps
RETROOS_NO_BAR=1 run -jot tests/host/jot.steps
RETROOS_NO_BAR=1 run -cabinet tests/host/cabinet.steps
RETROOS_NO_BAR=1 run -x11 tests/host/x11.steps
RETROOS_TEST_WIFI=1 RETROOS_STARTUP_CONF="~/startup.conf" run -control tests/host/control.steps
# A GTK 3 program in Station's frame and the Phosphor theme (D-056), where
# GTK is installed (CI: libgtk-3-bin).
if command -v gtk-builder-tool >/dev/null 2>&1; then
    cargo run -q -p retro-gtktheme -- "$OUT/gtk-data/themes" >/dev/null
    cargo run -q -p retro-fontc -- ttf "$OUT/gtk-data/Lantern.ttf"
    RETRO_GTK_DATA="$PWD/$OUT/gtk-data" RETRO_GTK_FONT="$PWD/$OUT/gtk-data/Lantern.ttf" \
        PATH="$PWD/tests/host/gtk:$PATH" run -gtk tests/host/gtk.steps
else
    echo ">> host test: -gtk skipped (no gtk-builder-tool)"
fi
# A 10.1" Touch Display 2: 1200x1920 portrait panel shown landscape (D-042).
RETROOS_NO_PROGMAN= RETROOS_ROTATE=auto STATION_ARGS="--headless 1200x1920" run -rotate tests/host/rotate.steps
# The luggable with an HDMI monitor too: two screens, the panel the main one (D-051).
RETROOS_NO_PROGMAN=1 RETROOS_ROTATE=auto STATION_ARGS="--headless 1200x1920+1920x1080" run -dual tests/host/dual.steps
echo ">> host tests: all passed"
