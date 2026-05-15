---
description: 현재 작업 세션의 변경사항/교훈을 분석해 CLAUDE.md "구현 메모 (추가)" entry 초안을 작성. 사용자 승인 후 CLAUDE.md 에 append + active plan 을 archive 로 이동.
argument-hint: [선택] entry 의 짧은 제목 — 비우면 변경사항에서 자동 추출
---

# /journal — 학습 기록 + plan archive

너는 지금 pet-companion 작업이 끝난 사용자를 받았다. 이번 세션에서 무엇을 배웠는지 CLAUDE.md 에 기록하고, 진행 중이던 plan 을 archive 로 옮긴다.

입력: `$ARGUMENTS` (없으면 변경사항에서 제목 자동 추출).

## 사전 작업

1. 변경사항 수집:
   - `git log main..HEAD --oneline` 으로 이 작업의 커밋 목록.
   - `git diff main...HEAD` 로 누적 diff.
   - `git status` 로 미커밋 변경.
2. 활성 plan 확인:
   - `.claude/plans/active/` 에 파일이 있으면 가장 최근 파일 읽기.
   - 여러 개면 사용자에게 어느 plan 을 archive 할지 묻기.
3. 현재 버전 확인:
   - `package.json` 의 `version`.
   - `src-tauri/Cargo.toml` 의 `version`.
   - 변경사항 성격상 버전이 올라가야 하는데 그대로면 사용자에게 알림.
4. CLAUDE.md 의 "구현 메모 (추가)" 섹션 마지막 entry 들을 읽어 스타일 파악.

## 단계 1 — Entry 초안 작성

CLAUDE.md "구현 메모 (추가)" 의 기존 entry 패턴을 따른다:

```
- **[제목] (vX.Y.Z)**: [증상 / 사용자 보고] — [로그/근거] — [근본 원인] — [수정 내용] — [불변 규칙 / 향후 회귀 방지 노트].
```

필수 요소:
1. **버전 태그** — 변경된 버전이 있다면 `(v0.X.Y)` 명시.
2. **증상 / 사용자 보고** — 이 작업이 시작된 트리거. 사용자가 한 말이 있다면 인용.
3. **로그 / 진단 근거** — 핵심 stderr 라인, 핵심 좌표 값, 핵심 코드 경로.
4. **근본 원인** — "그래서 왜 그게 깨졌나" 한 문장 설명.
5. **수정** — 어느 파일의 어느 함수가 어떻게 바뀌었나.
6. **불변 규칙** — 다음에 같은 실수를 피하려면 무엇을 지켜야 하나. (CLAUDE.md 의 다른 entry 들이 이 패턴을 따름)

활성 plan 의 "근본 원인 가설" / "회귀 위험" 섹션을 entry 작성에 활용한다.

## 단계 2 — 사용자에게 초안 제시 + 승인 받기

다음과 같이 출력하고 **반드시 사용자 응답을 기다린다**:

```
## 📝 CLAUDE.md entry 초안

다음 entry 를 `CLAUDE.md` 의 "## 구현 메모 (추가)" 섹션 끝에 추가하려고 합니다.

---

- **[초안 entry 전체 텍스트]**

---

### 부가 액션
- 활성 plan 이동: `.claude/plans/active/{filename}` → `.claude/plans/archive/{filename}`
- (해당 시) 버전 bump 권고: `package.json` v0.1.X → v0.1.(X+1), `Cargo.toml` 동일.

승인하시면 "ok" 또는 수정 의견을 알려주세요.
```

사용자 응답 처리:
- "ok" / "진행" / "yes" → 단계 3 실행.
- 수정 의견 → entry 를 수정해서 다시 단계 2 제시.
- "cancel" / "취소" → 아무것도 변경하지 않고 종료.

## 단계 3 — 적용

승인 받은 경우에만 실행:

1. **CLAUDE.md 에 entry append**:
   - `Edit` 도구로 "구현 메모 (추가)" 섹션의 마지막 bullet 뒤에 새 entry 삽입.
   - 단순히 파일 끝에 붙이지 말 것 — "## 작업 규칙" 섹션이 항상 가장 마지막에 있어야 한다.
   - 정확한 위치: "## 작업 규칙" 헤더 직전에 entry 추가.
2. **활성 plan 이동**:
   - `Bash` 로 `mv .claude/plans/active/{filename} .claude/plans/archive/{filename}`.
   - 여러 plan 이 있었으면 단계 0 에서 선택한 것만 이동.
3. **(옵션) 버전 bump**:
   - 사용자가 명시적으로 승인한 경우에만 `package.json` / `Cargo.toml` 의 version 필드 수정.
   - 자동 진행 금지 — 반드시 사용자 확인.
4. **최종 출력**:

```
✅ /journal 완료

- CLAUDE.md: entry 추가됨 ("구현 메모 (추가)" 마지막)
- Plan archived: .claude/plans/archive/{filename}
- 버전 bump: {수행됨 / 생략됨}

다음 단계 권고:
- git add CLAUDE.md && git commit
- (버전 bump 했다면) ./scripts/release.sh 또는 수동 release
```

## 주의

- **CLAUDE.md 자동 수정 금지** — 반드시 사용자 승인 후에만 Edit 호출.
- "작업 규칙" 섹션은 항상 파일의 마지막에 있다. entry 는 그 직전에 들어간다.
- 활성 plan 이 없는 경우 (예: `/plan` 없이 즉흥 작업한 경우) plan 이동 단계는 건너뛴다.
- 변경사항이 trivial 한 경우 (오타, 단순 CSS) 에이전트 판단으로 "이번 변경은 CLAUDE.md 업데이트가 불필요" 라고 보고 entry 작성을 생략할 수 있다. 이때는 `/review` 의 docs-sync 결과를 인용한다.
- entry 작성 스타일은 한국어 + 영어 식별자 혼용. 기존 entry 들의 톤을 모방.
