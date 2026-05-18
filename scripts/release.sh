#!/bin/bash
set -e

APP_NAME="Pet Companion"
ZIP_NAME="Pet-Companion"

# ── 버전 결정 ──────────────────────────────────────
PKG_VERSION=$(node -p "require('./package.json').version")
LATEST_TAG=$(git tag -l 'v*' --sort=-v:refname | head -1)
LATEST_TAG_VERSION="${LATEST_TAG#v}"

if [ -z "$LATEST_TAG_VERSION" ]; then
  VERSION="$PKG_VERSION"
elif [ "$PKG_VERSION" != "$LATEST_TAG_VERSION" ]; then
  VERSION="$PKG_VERSION"
else
  IFS='.' read -r MAJOR MINOR PATCH <<< "$LATEST_TAG_VERSION"
  PATCH=$((PATCH + 1))
  VERSION="$MAJOR.$MINOR.$PATCH"
fi

echo "📦 릴리즈 버전: v$VERSION"

# ── 버전 동기화 (package.json, tauri.conf.json, Cargo.toml) ──
node -e "
  const fs = require('fs');
  const pkg = JSON.parse(fs.readFileSync('package.json', 'utf8'));
  pkg.version = '$VERSION';
  fs.writeFileSync('package.json', JSON.stringify(pkg, null, 2) + '\n');
"

node -e "
  const fs = require('fs');
  const conf = JSON.parse(fs.readFileSync('src-tauri/tauri.conf.json', 'utf8'));
  conf.version = '$VERSION';
  fs.writeFileSync('src-tauri/tauri.conf.json', JSON.stringify(conf, null, 2) + '\n');
"

sed -i '' "s/^version = \".*\"/version = \"$VERSION\"/" src-tauri/Cargo.toml
echo "✅ 버전 동기화 완료: $VERSION"

# ── 빌드 ───────────────────────────────────────────
echo "🔨 빌드 시작..."
pnpm tauri build
bash scripts/post-build.sh

DIST_DIR="src-tauri/target/release/bundle/dist"
ZIP_PATH="$DIST_DIR/${ZIP_NAME}.zip"

# ── 커밋 + 태그 + 푸시 ────────────────────────────
git add package.json src-tauri/tauri.conf.json src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "release: v$VERSION" --allow-empty
git tag "v$VERSION"
git push github "$(git rev-parse --abbrev-ref HEAD)"
git push github "v$VERSION"
echo "🏷️  태그 v$VERSION 푸시 완료"

# ── 릴리즈 노트 ────────────────────────────────────
PREVIOUS_TAG="$LATEST_TAG"
NOTES_FILE=$(mktemp "/tmp/pet-companion-release-notes.XXXXXX.md")

if [ -n "$PREVIOUS_TAG" ]; then
  CHANGE_HINTS=$(git log --no-merges --pretty=format:'- %s' "$PREVIOUS_TAG..HEAD")
else
  CHANGE_HINTS=$(git log --no-merges --pretty=format:'- %s')
fi

cat > "$NOTES_FILE" <<EOF
## 변경 사항
$CHANGE_HINTS

## 설치 방법

ad-hoc 서명 빌드라 macOS Gatekeeper가 zip을 격리(quarantine)합니다. 터미널에서 격리 해제 후 설치 스크립트를 실행하세요.

\`\`\`bash
cd ~/Downloads
unzip -o ${ZIP_NAME}.zip
xattr -cr ${ZIP_NAME}
bash ${ZIP_NAME}/install.sh
\`\`\`

설치되면 \`/Applications/Pet Companion.app\`이 자동 실행됩니다. 다음 릴리즈도 동일한 방식으로 덮어쓰기 설치합니다.
EOF

# ── GitHub 릴리즈 ──────────────────────────────────
if [ ! -f "$ZIP_PATH" ]; then
  echo "⚠️  빌드 산출물이 없습니다: $ZIP_PATH"
  rm -f "$NOTES_FILE"
  exit 1
fi

gh release create "v$VERSION" "$ZIP_PATH" \
  --title "v$VERSION" \
  --notes-file "$NOTES_FILE"

echo "🚀 릴리즈 v$VERSION 생성 완료!"
rm -f "$NOTES_FILE"
