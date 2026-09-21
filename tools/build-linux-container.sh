#!/usr/bin/env bash
set -euo pipefail

export DEBIAN_FRONTEND=noninteractive
export APPIMAGE_EXTRACT_AND_RUN=1

apt-get update
apt-get install -y --no-install-recommends \
  ca-certificates curl build-essential file libappindicator3-dev librsvg2-dev \
  libssl-dev libwebkit2gtk-4.1-dev patchelf xdg-utils

curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
# shellcheck disable=SC1091
. /root/.cargo/env
npm ci
npx tauri build --bundles appimage,deb --config '{"bundle":{"createUpdaterArtifacts":false}}'
