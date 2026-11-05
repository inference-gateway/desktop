#!/bin/sh
set -e

Xvfb "$DISPLAY" -screen 0 1280x800x24 -nolisten tcp >/dev/null 2>&1 &
until [ -e "/tmp/.X11-unix/X${DISPLAY#:}" ]; do sleep 0.1; done

openbox >/dev/null 2>&1 &
firefox --no-remote --width 1280 --height 800 about:blank >/dev/null 2>&1 &

exec infer "$@"
