#!/bin/sh
set -eu

OPENTASK_XPI_URL=https://github.com/inference-gateway/opentask/releases/latest/download/opentask-firefox.xpi
OPENTASK_ID=opentask@inference-gateway.github.io
OPENTASK_DIR=/opt/opentask
BRIDGE_PORT=52790
FIREFOX_DIR=/usr/lib/firefox-esr
INFER_HOME=/home/infer

install_firefox_esr() {
  install -d -m 0755 /etc/apt/keyrings
  curl -fsSL https://packages.mozilla.org/apt/repo-signing-key.gpg -o /etc/apt/keyrings/packages.mozilla.org.asc
  echo "deb [signed-by=/etc/apt/keyrings/packages.mozilla.org.asc] https://packages.mozilla.org/apt mozilla main" \
    >/etc/apt/sources.list.d/mozilla.list
  apt-get update
  apt-get install -y --no-install-recommends firefox-esr
}

download_opentask() {
  install -d -m 0755 "$OPENTASK_DIR"
  cd "$OPENTASK_DIR"
  curl -fsSLO "$OPENTASK_XPI_URL"
  curl -fsSL "$OPENTASK_XPI_URL.sha256" | sha256sum -c -
  cd - >/dev/null
}

write_firefox_policies() {
  install -d "$FIREFOX_DIR/distribution"
  cat >"$FIREFOX_DIR/distribution/policies.json" <<EOF
{
  "policies": {
    "SkipTermsOfUse": true,
    "OverrideFirstRunPage": "",
    "OverridePostUpdatePage": "",
    "DontCheckDefaultBrowser": true,
    "DisableTelemetry": true,
    "DisableFirefoxStudies": true,
    "UserMessaging": {
      "SkipOnboarding": true,
      "WhatsNew": false,
      "FeatureRecommendations": false
    },
    "Preferences": {
      "xpinstall.signatures.required": {
        "Value": false,
        "Status": "locked"
      }
    },
    "ExtensionSettings": {
      "$OPENTASK_ID": {
        "installation_mode": "force_installed",
        "install_url": "file://$OPENTASK_DIR/opentask-firefox.xpi"
      }
    },
    "3rdparty": {
      "Extensions": {
        "$OPENTASK_ID": {
          "bridge-port": "$BRIDGE_PORT",
          "bridge-token": "$1",
          "bridge-project-dir": "/workspace"
        }
      }
    }
  }
}
EOF
}

start_firefox_on_login() {
  install -d /etc/xdg/autostart
  printf '[Desktop Entry]\nType=Application\nName=Firefox\nExec=firefox-esr about:blank\n' \
    >/etc/xdg/autostart/firefox-esr.desktop
}

enable_browser_use() {
  sed -i \
    -e 's/^enabled: .*/enabled: true/' \
    -e 's/^backend: .*/backend: extension/' \
    -e "s/^  port: .*/  port: $BRIDGE_PORT/" \
    -e "s/^  token: .*/  token: $1/" \
    "$INFER_HOME/.infer/browser_use.yaml"
}

bridge_token=$(od -An -N16 -tx1 /dev/urandom | tr -d ' \n')
install_firefox_esr
download_opentask
write_firefox_policies "$bridge_token"
start_firefox_on_login
enable_browser_use "$bridge_token"
