---
description: pet-companion 작업의 brainstorming + plan.md 작성 + atomic task 분할까지 수행한다. 복잡한 버그/기능 시작 전 사용. 입력 예 — /plan "혼합 DPI 드래그 oscillation"
argument-hint: <작업 제목 또는 짧은 설명>
---

# /plan — 기획 + 설계 + 작업 분할

너는 지금 pet-companion 의 **새 작업을 시작**하려는 사용자를 받았다. 사용자 입력: `$ARGUMENTS`.

이 명령은 **항상 brainstorming 부터** 시작한다 (옵션 A 정책). 짧은 입력이든 긴 입력이든 요구사항을 먼저 명확화한다. 입력이 비어 있으면 사용자에게 한 줄 요약을 먼저 묻는다.

## 단계 1 — Brainstorming (요구사항 명확화)

1. `superpowers:brainstorming` skill 을 Skill 도구로 호출한다.
2. brainstorming 이 끝나면 다음을 한 단락으로 요약한다:
   - 문제 (증상 / 발생 조건)
   - 영향 범위 (어떤 사용자, 어떤 상태)
   - 성공 조건 (이게 고쳐졌다고 어떻게 판단하나)
   - 비범위 (이번 작업에서 안 건드릴 것)

## 단계 2 — plan.md 작성

1. `superpowers:writing-plans` skill 을 Skill 도구로 호출하여 plan 을 작성한다.
2. plan 의 컨텍스트로 다음을 반드시 주입한다:
   - 단계 1에서 정리한 요구사항 요약
   - `CLAUDE.md` 의 관련 회귀 패턴 (구현 메모 섹션에서 키워드로 grep)
   - 변경이 예상되는 주요 파일 경로 (CLAUDE.md "주요 파일" 섹션 참고)
   - **`docs/STRATEGY.md` 존재 여부 확인 후 placeholder 가 아닌 실제 내용이 있으면 plan 컨텍스트에 포함** (큰 기능/방향성 작업일수록 중요. 단순 버그 수정엔 비어 있어도 무방).
3. plan 출력 위치:
   - 디렉터리: `.claude/plans/active/`
   - 파일명: `YYYY-MM-DD-{slug}.md` (오늘 날짜 + 입력으로부터 추출한 kebab-case slug)
   - slug 가 모호하면 사용자에게 확인 후 진행
4. plan.md 의 필수 섹션:
   - **요구사항** (단계 1 결과)
   - **재현 / 증상** (해당하면)
   - **근본 원인 가설** (여러 개 가능, 검증 방법 포함)
   - **설계 / 변경 계획** (파일별)
   - **회귀 위험** (CLAUDE.md 의 어느 불변 규칙이 깨질 수 있나)
   - **검증 / 테스트 전략**
   - **Atomic Tasks** (단계 3에서 채움)

## 단계 3 — Atomic Task 분할 (TDD 강제)

1. plan.md 의 "설계 / 변경 계획" 을 보고, 한 번에 한 파일 / 한 책임만 건드리는 단위로 쪼갠다.
2. 각 task 는 **최대 1개 파일 + 최대 1개 함수/컴포넌트** 수준이어야 한다 (GSD 원칙).
3. **TDD 흐름 강제**: 각 변경 단위가 testable 한 코드(순수 함수, 로직 모듈, IPC 커맨드, 분리 가능한 React 컴포넌트 props)면 다음 3-단계로 자동 분할:
   - `[test]` **테스트 먼저 작성** (RED) — 실패하는 테스트 케이스.
   - `[impl]` **구현** (GREEN) — 테스트 통과시키는 최소 코드.
   - `[refactor]` **리팩터** — 통과 상태 유지하며 정리.
   - testable 하지 않은 변경(OS API 직결, GUI hit-test, AppleScript 호출, 단순 CSS 조정)은 단일 task 로 두고 task 설명에 `[no-test: 사유]` 라벨을 붙인다.
4. task 들을 두 곳에 동시에 기록한다:
   - **plan.md 의 "Atomic Tasks" 섹션** — 체크박스 리스트 (`- [ ] [test] ...`, `- [ ] [impl] ...`).
   - **`TodoWrite` 도구** — 같은 task 들을 status="pending" 으로 등록.
5. 각 task 는 다음 형식:
   - content (imperative): "[test] src/state.test.ts 에 mergeSession dismissed 해제 케이스 추가" 또는 "[impl] src-tauri/src/lib.rs 의 run_drag_cursor_loop 시그니처에 primary_scale 인자 추가"
   - activeForm (진행형): "state.test.ts dismissed 해제 테스트 작성 중" / "lib.rs run_drag_cursor_loop 시그니처 변경 중"

## 출력 형식

마지막에 다음을 화면에 출력한다:

```
✅ Plan 작성 완료

- 파일: .claude/plans/active/YYYY-MM-DD-{slug}.md
- Atomic tasks: N개
- TodoWrite 등록 완료

다음 단계:
1. plan.md 검토 후 필요하면 수정
2. 작업 시작 — TodoWrite 의 첫 task 부터 진행
3. 완료 후 /review 로 회귀 점검
4. 최종 /journal 로 CLAUDE.md 업데이트 + plan archive 이동
```

## 주의

- 단계 1 brainstorming 결과가 충분히 명확하지 않으면 단계 2로 넘어가지 말고 추가 질문한다.
- plan.md 가 이미 존재하면 (같은 slug) 덮어쓰기 전에 사용자 확인.
- plan 작성 후 자동으로 코드 수정으로 넘어가지 말 것. 이 명령은 "계획"까지만 책임진다.
