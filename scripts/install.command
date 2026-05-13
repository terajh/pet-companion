#!/bin/bash
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
APP_NAME="Pet Companion.app"
PROCESS_NAME="claude-pet-companion"
APP_PATH="$SCRIPT_DIR/$APP_NAME"

if [ ! -d "$APP_PATH" ]; then
  echo "오류: $APP_NAME 을 찾을 수 없습니다."
  exit 1
fi

echo "Pet Companion 설치 중..."

if pgrep -x "$PROCESS_NAME" > /dev/null 2>&1; then
  echo "실행 중인 Pet Companion을 종료합니다..."
  pkill -x "$PROCESS_NAME"
  sleep 1
  pgrep -x "$PROCESS_NAME" > /dev/null 2>&1 && pkill -9 -x "$PROCESS_NAME" && sleep 1
fi

# quarantine 해제
xattr -cr "$APP_PATH"

cp -R "$APP_PATH" /Applications/
xattr -cr "/Applications/$APP_NAME"

echo "설치 완료! Pet Companion을 실행합니다."
open "/Applications/$APP_NAME"

rm -rf "$SCRIPT_DIR"
