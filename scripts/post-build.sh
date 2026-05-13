#!/bin/bash
set -e

APP_NAME="mac-timer"
BUNDLE_DIR="src-tauri/target/release/bundle"
APP_PATH="$BUNDLE_DIR/macos/${APP_NAME}.app"
DIST_DIR="$BUNDLE_DIR/dist"
ZIP_PATH="$DIST_DIR/${APP_NAME}.zip"

if [ ! -d "$APP_PATH" ]; then
  echo "오류: 빌드된 .app을 찾을 수 없습니다: $APP_PATH"
  exit 1
fi

echo "==> ad-hoc 코드 서명..."
codesign --force --deep --sign - "$APP_PATH"

echo "==> 배포 zip 생성..."
rm -rf "$DIST_DIR"
mkdir -p "$DIST_DIR/${APP_NAME}"

cp -R "$APP_PATH" "$DIST_DIR/${APP_NAME}/"
cp scripts/install.command "$DIST_DIR/${APP_NAME}/"
chmod +x "$DIST_DIR/${APP_NAME}/install.command"

cd "$DIST_DIR"
zip -r "${APP_NAME}.zip" "${APP_NAME}/"
rm -rf "${APP_NAME}/"
cd - > /dev/null

echo "==> 완료: $ZIP_PATH"
