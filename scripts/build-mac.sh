#!/bin/bash
# Build NoopFlow on a Mac. Run from the project root:  bash scripts/build-mac.sh
set -euo pipefail

if ! xcode-select -p >/dev/null 2>&1; then
  echo "Xcode 명령줄 도구가 필요해요. 설치 창이 뜨면 설치 후 다시 실행하세요."
  xcode-select --install || true
  exit 1
fi

if ! command -v cargo >/dev/null 2>&1; then
  echo "Rust가 필요해요. https://rustup.rs 의 설치 명령을 실행한 뒤 새 터미널에서 다시 실행하세요."
  exit 1
fi

if ! cargo tauri --version >/dev/null 2>&1; then
  cargo install tauri-cli --version "^2" --locked
fi

cargo test -p noopflow-core
cd src-tauri
cargo tauri build --bundles app

APP="../target/release/bundle/macos/NoopFlow.app"
echo
echo "완료: $APP"
echo "응용 프로그램 폴더로 옮기려면:  cp -R \"$APP\" /Applications/"
