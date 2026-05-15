#!/bin/bash
# Pet Companion 디버그 번들 실행 스크립트
#
# 사용법:
#   bash scripts/dev-run.sh          # 기존 디버그 번들 그대로 실행
#   bash scripts/dev-run.sh --build  # 빌드 후 실행
#   bash scripts/dev-run.sh --test   # 테스트 + 빌드 후 실행
#
# stderr가 터미널에 그대로 흘러나오므로 [InProgress], [focus],
# [set_position] 같은 진단 로그를 실시간으로 볼 수 있다.

set -e

APP_NAME="Pet Companion"
BUNDLE_PATH="src-tauri/target/debug/bundle/macos/${APP_NAME}.app"
BINARY_PATH="${BUNDLE_PATH}/Contents/MacOS/claude-pet-companion"

# ── 옵션 파싱 ──────────────────────────────────────
RUN_TESTS=0
RUN_BUILD=0
for arg in "$@"; do
  case "$arg" in
    --test|--tests)  RUN_TESTS=1; RUN_BUILD=1 ;;
    --build)         RUN_BUILD=1 ;;
    *) echo "알 수 없는 옵션: $arg"; exit 1 ;;
  esac
done

# ── 테스트 ─────────────────────────────────────────
if [ "$RUN_TESTS" -eq 1 ]; then
  echo "🧪 vitest 실행..."
  pnpm test
  echo "🦀 cargo test 실행..."
  cargo test --manifest-path src-tauri/Cargo.toml --lib
fi

# ── 빌드 ───────────────────────────────────────────
if [ "$RUN_BUILD" -eq 1 ]; then
  echo "🔨 디버그 번들 빌드..."
  pnpm build
  pnpm tauri build --debug
fi

# ── 번들 존재 확인 ──────────────────────────────────
if [ ! -x "$BINARY_PATH" ]; then
  echo "⚠️  디버그 번들이 없다: $BUNDLE_PATH"
  echo "    --build 플래그로 다시 실행하거나 직접 빌드해라:"
  echo "    pnpm build && pnpm tauri build --debug"
  exit 1
fi

# ── 기존 인스턴스 종료 ──────────────────────────────
if pgrep -f "${APP_NAME}.app" > /dev/null 2>&1; then
  echo "🛑 기존 ${APP_NAME} 인스턴스 종료..."
  pkill -f "${APP_NAME}.app" 2>/dev/null || true
  sleep 0.3
fi

# ── 실행 ───────────────────────────────────────────
echo "🚀 실행: $BINARY_PATH"
echo "   (Ctrl-C로 종료, stderr 로그가 그대로 보인다)"
echo "───────────────────────────────────────────────"
exec "$BINARY_PATH"
