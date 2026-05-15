---
description: pet-companion 하네스 파이프라인 오케스트레이터. 현재 상태(active plan, git diff, 최근 리뷰)를 보고 다음에 필요한 명령을 자동으로 실행한다. 매번 호출하면 다음 게이트까지 진행 후 사용자 입력 대기.
argument-hint: [최초 호출 시 작업 제목, 이후 호출은 인자 없이]
---

# /cycle — 파이프라인 오케스트레이터

너는 지금 pet-companion 작업의 **현재 상태를 진단**하고, 다음 단계를 자동으로 진행해야 한다. 입력: `$ARGUMENTS` (있으면 새 작업 시작, 없으면 진행 중 작업 이어가기).

## 사전 진단

다음을 모두 확인해서 현재 상태를 한 줄로 요약한 후 사용자에게 출력:

1. **활성 plan**: `ls .claude/plans/active/*.md 2>/dev/null` — 파일이 있나? 몇 개?
2. **변경사항**: `git status --porcelain` — uncommitted 가 있나?
3. **변경 범위**: `git diff --stat` — 라인 수, 파일 수.
4. **최근 커밋**: `git log --oneline -5` — 마지막 커밋이 이 작업 사이클의 일부인가?

진단 결과 한 줄 출력 예:
```
📊 상태: active plan 1개 / staged 3 파일 / +120 -45 라인 / 마지막 커밋: "feat: timestamp 표시"
```

## 상태별 분기 (state machine)

다음 상태 중 하나를 결정하고 해당 단계만 실행한다. **한 번에 한 단계만**. 다 끝나면 사용자에게 다음 액션 안내 후 종료.

### 상태 A: 활성 plan 없음 + 인자 있음 → 신규 작업 시작

→ `/plan $ARGUMENTS` 흐름 그대로 실행 (`.claude/commands/plan.md` 의 단계 1~3 수행). 종료 시 다음을 출력:

```
✅ Plan 생성 완료. 이제 코드 수정을 시작하세요.

다음 단계:
- TodoWrite 의 첫 task 부터 진행
- 작업 완료되면 다시 `/cycle` 호출 → 자동으로 /review 진입
```

### 상태 B: 활성 plan 없음 + 인자 없음 + uncommitted 변경 있음 → /plan 누락된 즉흥 작업

→ 사용자에게 알림:

```
⚠️ 진행 중인 plan 이 없습니다.
- 그냥 리뷰만 받고 싶으면: `/review` 직접 호출.
- 작업 의도를 정식 plan 으로 정리하고 싶으면: `/cycle "작업 제목"`.
```

→ 종료. 자동으로 /review 로 넘어가지 않음 (plan 없이 review 만 도는 건 사용자가 명시적으로 결정).

### 상태 C: 활성 plan 있음 + uncommitted 변경 없음 → 작업 미시작 또는 방금 시작

→ 사용자에게 안내:

```
📝 활성 plan: .claude/plans/active/{filename}
   변경사항이 아직 없습니다. plan 의 TodoWrite task 부터 진행하세요.
   작업 완료되면 다시 `/cycle` 호출.
```

→ 종료.

### 상태 D: 활성 plan 있음 + uncommitted 변경 있음 → /review 자동 실행

→ `/review` 흐름 그대로 실행 (`.claude/commands/review.md`). 4 에이전트 병렬 호출 + 통합 리포트.

리포트 출력 후 추가 분기:

#### D-1: Fail > 0
```
🚨 Fail N개. 먼저 이슈를 해결한 후 다시 `/cycle` 또는 `/review` 를 호출하세요.
```
→ 종료.

#### D-2: Fail == 0, UI 변경 감지
```
✅ /review 통과. UI 변경(.tsx/.css)이 감지되었습니다.

다음 단계 중 선택:
- `/ui-check` 로 시각 검증 (pnpm tauri dev 또는 pnpm dev 가 떠 있을 때)
- 시각 검증 skip 하고 바로 학습 기록: `/cycle` (자동으로 /journal 진입)
```
→ 종료.

#### D-3: Fail == 0, UI 변경 없음
```
✅ /review 통과. /journal 로 넘어갑니다.
```
→ 곧바로 `/journal` 흐름 실행 (`.claude/commands/journal.md`). 단계 2 (entry 초안 + 사용자 승인 요청) 까지 진행 후 사용자 응답 대기.

### 상태 E: 최근 /review 통과 기록 있음 + 활성 plan 있음 + 사용자가 "/cycle" 만 호출

(예: D-2 상태에서 UI 검증 skip 하고 다시 cycle 호출하는 경우)

→ `/journal` 흐름 직행. CLAUDE.md entry 초안 → 승인 대기.

### 상태 F: 활성 plan 이 archive 로 이동됨 + uncommitted 없음 → 사이클 완료

→ 출력:

```
🎉 사이클 완료. plan 이 archive 로 이동되었습니다.

다음 단계:
- git commit + (필요 시) ./scripts/release.sh
- 새 작업 시작: `/cycle "다음 작업 제목"`
```

→ 종료.

## 상태 판별 알고리즘 (의사 코드)

```
active_plans = list .claude/plans/active/*.md
uncommitted = git status --porcelain
has_arg = $ARGUMENTS != ""

if has_arg and len(active_plans) == 0:
    → 상태 A (/plan 실행)
elif len(active_plans) == 0 and uncommitted:
    → 상태 B (안내 후 종료)
elif len(active_plans) >= 1 and not uncommitted:
    → 상태 C (안내 후 종료, 또는 사이클 완료 직후라면 F)
elif len(active_plans) >= 1 and uncommitted:
    → 상태 D (/review 실행)
    → review 결과에 따라 D-1/D-2/D-3 분기
else:
    → 상태 F (사이클 완료)
```

## 주의

- **각 호출은 한 게이트까지만 진행**. /plan → /review → /journal 을 한 번의 /cycle 호출로 모두 진행하지 말 것. 사용자가 코드를 수정해야 하는 휴먼 게이트가 사이에 있기 때문.
- 사용자가 `/cycle` 사이에 직접 `/review`, `/journal` 을 호출해도 충돌 없음 — `/cycle` 은 단지 다음 단계를 추론할 뿐.
- 활성 plan 이 여러 개면 사용자에게 어느 것을 진행할지 묻는다 (자동 선택 금지).
- /plan, /review, /journal 의 세부 동작은 각 명령 파일을 따른다 — 여기서 재정의하지 않는다.
- `/cycle` 안에서 `/plan` 흐름을 실행할 때, 사용자 입력 (brainstorming 응답) 을 기다리는 것은 정상이다. 그 대기 자체가 휴먼 게이트.
