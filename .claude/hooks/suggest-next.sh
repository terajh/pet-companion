#!/usr/bin/env bash
# pet-companion Stop hook — 매 응답 끝에 현재 하네스 상태를 진단하고 다음 단계 명령을 제안.
# stdin 으로 받은 hook payload(JSON) 는 사용하지 않고, 순수하게 워킹디렉터리 상태로 추론한다.
# 출력은 stderr 로 흘려서 transcript 에는 남기지만 모델 컨텍스트는 오염하지 않는다.

set -u

# 인터랙티브가 아니거나 git 저장소가 아니면 침묵.
if ! git rev-parse --show-toplevel >/dev/null 2>&1; then
  exit 0
fi

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT" || exit 0

ACTIVE_PLANS_DIR=".claude/plans/active"
ACTIVE_COUNT=0
if [ -d "$ACTIVE_PLANS_DIR" ]; then
  # .gitkeep 제외하고 .md 파일만 카운트
  ACTIVE_COUNT=$(find "$ACTIVE_PLANS_DIR" -maxdepth 1 -type f -name '*.md' 2>/dev/null | wc -l | tr -d ' ')
fi

UNCOMMITTED=$(git status --porcelain 2>/dev/null | wc -l | tr -d ' ')

LAST_COMMIT_SUBJECT=$(git log -1 --pretty=%s 2>/dev/null)

# 한 줄 진단
printf '\n[/cycle hint] active_plans=%s uncommitted=%s last="%s"\n' \
  "$ACTIVE_COUNT" "$UNCOMMITTED" "$LAST_COMMIT_SUBJECT" >&2

# 상태별 제안
if [ "$ACTIVE_COUNT" -eq 0 ] && [ "$UNCOMMITTED" -eq 0 ]; then
  printf '[/cycle hint] 클린 상태. 새 작업을 시작하려면: /cycle "작업 제목"\n' >&2
elif [ "$ACTIVE_COUNT" -eq 0 ] && [ "$UNCOMMITTED" -gt 0 ]; then
  printf '[/cycle hint] plan 없이 변경사항만 있음. /review 로 점검하거나, 작업을 정식화하려면 /cycle "제목"\n' >&2
elif [ "$ACTIVE_COUNT" -ge 1 ] && [ "$UNCOMMITTED" -eq 0 ]; then
  printf '[/cycle hint] plan 활성 / 변경 없음. plan 의 TodoWrite task 부터 진행. 완료되면 /cycle\n' >&2
elif [ "$ACTIVE_COUNT" -ge 1 ] && [ "$UNCOMMITTED" -gt 0 ]; then
  # UI 변경 감지
  UI_CHANGED=$(git status --porcelain 2>/dev/null | awk '{print $NF}' | grep -E '\.(tsx|css)$' | head -1)
  if [ -n "$UI_CHANGED" ]; then
    printf '[/cycle hint] plan + 변경 있음 (UI 포함). 다음: /cycle → /review 자동 진입 → 통과 후 /ui-check 권장\n' >&2
  else
    printf '[/cycle hint] plan + 변경 있음. 다음: /cycle → /review 자동 진입\n' >&2
  fi
fi

exit 0
