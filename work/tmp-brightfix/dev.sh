#!/usr/bin/env bash
# Dev launcher for the GPUI app.
# - WAYLAND_DISPLAY= forces the X11/XWayland backend: gpui 0.2.2's Wayland
#   client panics (UnsupportedVersion) against the WSLg compositor.
# - RUSTFLAGS works around the missing -lxkbcommon-x11 symlink on this machine.
set -euo pipefail
cd "$(dirname "$0")"
export WAYLAND_DISPLAY=
export RUSTFLAGS="${RUSTFLAGS:-} -L /home/jemo/.local/lib"
exec cargo run -p perez-rs "$@"
