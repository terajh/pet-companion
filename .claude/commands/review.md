---
description: 현재 변경사항을 4개 에이전트(Claude tauri-reviewer + Codex code-quality + Claude docs-sync + Claude test-coverage)로 병렬 리뷰. PR 만들기 전 또는 /journal 직전에 사용. .tsx/.css 변경 감지 시 /ui-check 권장 알림 출력.
argument-hint: [선택] 리뷰 범위 — 비우면 git diff, "staged" 면 staged 만, 또는 특정 파일/디렉터리 경로
---

# /review — 4개 에이전트 병렬 리뷰

너는 지금 pet-companion 의 변경사항을 **네 가지 관점에서 동시에** 리뷰해야 한다. 입력: `$ARGUMENTS` (없으면 전체 diff 대상).

## 사전 작업

1. 리뷰 범위 결정:
   - 입력 비어 있음 → `git diff main...HEAD` + `git diff` (스테이지/언스테이지 모두).
   - 입력 = "staged" → `git diff --staged` 만.
   - 입력 = 파일/디렉터리 경로 → 해당 경로의 `git diff` + 현재 내용.
2. `git status`, `git log --oneline -10` 로 작업 컨텍스트 확보.
3. 변경 규모가 0이면 사용자에게 알리고 종료.
4. **UI 변경 감지**: 변경 파일 목록에 `.tsx` 또는 `.css` 또는 `src/App.tsx` 가 있으면 플래그 `UI_CHANGED=true` 로 설정. 통합 리포트 마지막에 `/ui-check` 권장 알림 출력.

## 4개 에이전트 병렬 실행

**한 메시지 안에서 네 도구를 동시에 호출**한다 (순차 실행 금지 — 병렬 실행이 핵심).

### 1. `Agent(subagent_type="tauri-reviewer")`

- description: "pet-companion Tauri/Rust/macOS 회귀 점검"
- prompt: 다음을 포함하여 self-contained 하게 작성.
  - 리뷰 범위 (어떤 diff / 어떤 파일)
  - 사용자가 작업 중인 plan (`.claude/plans/active/` 의 가장 최근 파일이 있으면 경로 명시)
  - 출력 형식 지시 (tauri-reviewer.md 에 정의된 형식 따르기)

### 2. `mcp__codex__codex` (Codex MCP, code-quality 관점)

- `sandbox`: `"read-only"` (코드 수정 금지)
- `approval-policy`: `"never"`
- `cwd`: 현재 worktree 절대 경로
- `model`: 생략 (기본 모델 사용)
- `prompt`: 다음을 그대로 전달하되 변수는 실제 값으로 치환.

```
당신은 pet-companion 프로젝트의 코드 품질 리뷰어입니다. 다른 모델의 시각으로 독립적인 second opinion 을 제공하는 게 임무입니다.

리뷰 범위:
{여기에 git diff 또는 파일 경로 명시}

다음을 점검하세요:
1. **불변성**: 객체 변경 대신 새 객체 생성 패턴이 유지되는가? (Rust 의 mutable borrow 도 포함)
2. **에러 처리**: Result/Option 의 unwrap 남발, 무음 실패, eprintln 으로 흘리는 에러가 없는가?
3. **입력 검증**: IPC 진입점에서 사용자 입력이 검증되는가?
4. **보안**: 하드코딩된 시크릿, 셸 명령 인젝션 (특히 AppleScript escape), 경로 traversal 위험.
5. **함수 크기 / 복잡도**: 50줄 초과, 4단계 초과 nesting 함수.
6. **데드 코드 / TODO / FIXME**: 잔존하는가?
7. **하드코딩된 값**: 매직 넘버, 매직 스트링 (특히 좌표 임계값, 타임아웃).
8. **로그 잔존**: 임시 디버그 로그 (`println!`, `console.log`) 가 production 경로에 남아 있는가?

코드 수정은 하지 마세요. read-only 분석만 수행하고 결과를 마크다운으로 출력하세요.

출력 형식:
## 🔬 codex code-quality 리포트

### ✅ Pass
- (회귀 없음 카테고리 명시)

### ⚠️ Warn
- **[카테고리]** (파일:라인) — 문제 설명 + 수정 제안.

### 🚨 Fail
- **[카테고리]** (파일:라인) — 즉시 수정 필요.

### 💭 개선 제안 (선택)
- (스타일/구조 개선 제안 — 강제 아님)
```

### 3. `Agent(subagent_type="docs-sync")`

- description: "CLAUDE.md 동기화 필요 여부 점검"
- prompt: 다음을 포함.
  - 리뷰 범위 (어떤 diff)
  - 출력 형식 지시 (docs-sync.md 에 정의된 형식 따르기)

### 4. `Agent(subagent_type="test-coverage")`

- description: "변경사항 대응 테스트 누락 점검"
- prompt: 다음을 포함.
  - 리뷰 범위 (어떤 diff)
  - 변경 파일 목록과 카테고리 (TS 로직 / React 컴포넌트 / Rust 로직 / IPC 커맨드 / CSS / 설정)
  - 출력 형식 지시 (test-coverage.md 에 정의된 형식 따르기)

## 결과 통합

네 에이전트 결과가 모두 도착하면 다음과 같이 통합 출력한다.

```
# 📋 /review 통합 리포트

> 범위: {git diff 범위 설명}
> 변경 파일: N개 / 추가 +X / 삭제 -Y 라인

---

## 🎯 tauri-reviewer (Claude)
{tauri-reviewer 출력 그대로}

---

## 🔬 code-quality (Codex, read-only)
{codex 출력 그대로}

---

## 📚 docs-sync (Claude)
{docs-sync 출력 그대로}

---

## 🧪 test-coverage (Claude)
{test-coverage 출력 그대로}

---

# 🚨 액션 요약

## 즉시 수정 (Fail)
- (네 리포트의 Fail 항목 통합)

## 검토 후 수정 (Warn)
- (네 리포트의 Warn 항목 통합)

## CLAUDE.md 업데이트
- (docs-sync 결과에 따라 "필요" / "불필요")

## 테스트 보강
- (test-coverage 결과에 따라 누락 테스트 우선순위 목록)

{UI_CHANGED 가 true 인 경우 다음 알림 출력:}
## 🎨 UI 변경 감지
- 변경 파일에 `.tsx` / `.css` 가 포함되어 있습니다.
- 다른 터미널에서 `pnpm tauri dev` 또는 `pnpm dev` 를 실행한 후 `/ui-check` 로 시각적 검증을 권장합니다.

## 다음 단계 권장
- Fail 모두 수정 → 다시 /review
- (UI 변경 시) /ui-check 로 시각 검증
- Warn 결정 후 → /journal 로 CLAUDE.md 업데이트 + plan archive 이동
```

## 주의

- 네 에이전트는 **반드시 한 메시지에서 동시 호출**. 순차 호출하면 사용자 대기 시간이 4배.
- Codex 가 응답하지 못하면 (MCP 연결 실패 등) 그 사실을 명시하고 나머지 세 에이전트 결과는 계속 제공.
- 리뷰 자체에서는 코드를 수정하지 않는다. 수정은 사용자가 결정 후 다음 turn 에서 진행.
- `code-reviewer` 같은 글로벌 agent 가 있어도 이 명령은 **반드시 위 4개 에이전트만** 사용한다 (정의된 페르소나 일관성).
- UI 변경이 있어도 본 명령에서 자동으로 `/ui-check` 를 호출하지 않는다 — 알림만 출력하고 사용자가 선택하게 한다.
