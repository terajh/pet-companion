# Card Visibility & UX Design — 2026-05-17

3개 사용자 보고를 묶어 단일 디자인으로 정리.

## 요구사항

1. **Watch 토글 즉시 반영**: Codex / Claude 세션 감시 체크박스를 OFF 하면 해당 앱의 카드가 **즉시** 숨겨져야 한다. 현재는 토글 후 수백 ms ~ 수 초 지연이 있다.
2. **'대기' 카드 노출 제거**: 카드는 (a) 진행 중(`running`) (b) 완료(`waving`, 사용자 클릭 전) 두 상태에서만 노출되어야 한다. 나머지(`waiting`, `idle`, `jumping`, `sleeping`, `review`, `failed`) 는 숨겨야 한다.
3. **카드 레이아웃**: 프로젝트 라벨을 작은 chip 으로 만들어 타이틀 같은 행 왼쪽에 두고, 타이틀은 1줄 ellipsis 로 표시한다.

## 비범위

- dismiss 정책 자체 변경 없음 (`dismissDecisionForVisualState` 유지).
- pet sprite 상태 계산 (`sessionVisualState`) 변경 없음 — 시각 표현은 그대로 두고 노출 조건만 좁힌다.
- 6 시간 최근 활동 fallback (`RECENT_ACTIVITY_WINDOW_MS`) 은 새 정책 하에서 dead code 가 되므로 제거.

## 성공 조건

- Settings 창에서 "Claude 세션 감시" 또는 "Codex 세션 감시" 체크박스 클릭 즉시(< 100ms) 해당 앱 카드가 사라진다.
- pet animation state 가 `running` 또는 `waving` 인 세션만 카드로 보인다. `waiting` / `idle` 카드는 보이지 않는다.
- 카드의 프로젝트 라벨이 chip 모양으로 타이틀 왼쪽에 붙고, 타이틀은 1줄로 표시된다.

---

## 설계

### Issue 1 — `cmd_set_watch_apps` 사이드채널 emit + 프런트 reconciliation

**근본 원인**: `cmd_set_watch_apps` 가 `tauri::async_runtime::spawn` 안에서 `state.model.lock().await` 후 `refresh_and_emit` 을 호출한다. 750ms refresh tick 이 같은 락을 `rebuild_payload` 전체 시간(수백 ms) 동안 보유해서 카드 사라짐이 늦다.

**수정**:

1. **백엔드** (`src-tauri/src/lib.rs`):
   - 새 이벤트 상수 `const WATCH_APPS_EVENT: &str = "companion:watch_apps";` (PET_SCALE_EVENT 옆).
   - 새 payload 구조체 `#[derive(Serialize, Clone)] #[serde(rename_all = "camelCase")] struct WatchAppsEvent { watch_claude: bool, watch_codex: bool }`.
   - `cmd_set_watch_apps` 본문 첫 줄(spawn 전)에서 `let _ = app.emit(WATCH_APPS_EVENT, WatchAppsEvent { watch_claude, watch_codex });` 발사.

2. **프런트** (`src/App.tsx` `usePayload`):
   - `pendingWatchRef = useRef<{ watchClaude: boolean; watchCodex: boolean; ts: number } | null>(null)` 추가.
   - `companion:watch_apps` listener 추가: `pendingWatchRef.current = { ...payload, ts: Date.now() }` 설정 + `setPayload(prev => ({ ...prev, config: { ...prev.config, watchClaude, watchCodex } }))` optimistic merge.
   - `companion:update` 핸들러에 watch reconciliation 분기 추가 (pet_override / pet_scale 분기와 동일 패턴):
     - `incoming.config.watchClaude === pending.watchClaude && incoming.config.watchCodex === pending.watchCodex` → 백엔드 따라잡음, pending clear, accept incoming.
     - `age > PENDING_INTENT_TTL_MS` → pending clear, accept incoming.
     - 그 외 (incoming stale) → `patched.config.watchClaude/watchCodex` 를 pending 값으로 덮어쓰기.

**불변 규칙 (CLAUDE.md 에 entry 추가)**: 사용자 토글이 "카드 표시 여부" 같은 즉시 시각 응답을 요구하는 모든 config IPC 는 v0.1.32 사이드채널 emit + v0.1.35 reconciliation 쌍을 동시에 추가해야 한다.

### Issue 2 — visualState 기반 카드 필터

**근본 원인**: `pickVisibleSessions` 가 `session.inProgress` 만 보고 노출 결정 → 백엔드의 `in_progress = true` 가 `running` 과 `waiting` 둘 다 포함하므로 `waiting` 카드가 보였다.

**수정** (`src/state.ts`):

1. 필터 본문을 visualState 기반으로 재작성:
   ```ts
   const visual = sessionVisualState(session, payload, clock);
   if (visual !== "running" && visual !== "waving") return false;
   if (dismissed.has(session.sessionId)) return false;
   return true;
   ```
2. archived 체크와 watch 필터는 visualState 계산 전에 유지 (early skip).
3. `RECENT_ACTIVITY_WINDOW_MS` 상수 + 관련 주석 제거.
4. 정렬 키는 `running` 우선, 그 다음 `lastActivityAt desc` 유지. visualState 가 `running`/`waving` 만 통과하므로 `inProgress` 정렬 키는 `visual === "running"` 정렬 키로 변경.

**보존**:
- `sessionVisualState` 시그니처/로직 변경 없음.
- `dismissDecisionForVisualState` 변경 없음.
- active session 도 `visualState === "running" | "waving"` 일 때만 노출 — 사용자 의도와 일치.

**CLAUDE.md "카드 표시 규칙" 갱신**: "진행 중(running) 과 완료(waving) 상태만 카드 노출. 대기(waiting) / 유휴(idle) 등은 숨김. dismiss 정책은 그대로."

### Issue 3 — 카드 레이아웃 (project chip + 1줄 title)

**수정**:

1. **`src/App.tsx` `OverlayCard`**: project 와 title 을 같은 flex row 로 묶기.
   ```jsx
   <span className="overlay-card__title-row">
     {project ? (
       <span className="overlay-card__project" title={session.cwd}>
         {project}
       </span>
     ) : null}
     <span className="overlay-card__title" title={session.title}>
       {session.title}
     </span>
   </span>
   ```
2. **`src/App.css`**:
   - 새 클래스 `.overlay-card__title-row` — `display: flex; align-items: center; gap: 6px; min-width: 0; margin-top: 2px;`.
   - `.overlay-card__project` 변경: 기존 block 스타일 → chip 스타일. 예: `display: inline-flex; align-items: center; padding: 2px 6px; border-radius: 6px; background: rgba(255,255,255,0.08); color: rgba(255,255,255,0.7); font-size: 10px; font-weight: 500; white-space: nowrap; flex-shrink: 0; max-width: 80px; overflow: hidden; text-overflow: ellipsis;`.
   - `.overlay-card__title` 변경: `-webkit-line-clamp: 2` 제거 → `white-space: nowrap; overflow: hidden; text-overflow: ellipsis; min-width: 0; flex: 1;`. `display: -webkit-box`, `-webkit-box-orient`, `word-break` 제거.

---

## 회귀 위험

- **v0.1.30 "Codex 최근 활동 카드 노출"**: 6 시간 fallback 제거되므로 앱 재시작 후 직전 Codex 세션이 카드로 안 남는다. 사용자가 명시적으로 "그 외엔 미노출" 을 요청했으므로 의도된 회귀.
- **active session 카드 사라짐**: active session 도 `running`/`waving` 만 통과 → `effective_state` 가 `idle` 인 active session 은 카드에서 사라짐. CLAUDE.md "카드 표시 규칙" 의 "active 는 항상 표시" 도 폐기됨. 의도된 변경.
- **watch_apps reconciliation 적용 누락**: 같은 패턴이 누락된 다른 config 토글이 있다면 동일 회귀가 재발한다 — entry 에 명시.

## 검증 / 테스트 전략

1. **단위 테스트** (`src/state.test.ts`):
   - `pickVisibleSessions` 가 `waiting` 세션을 숨기는 케이스 추가.
   - `pickVisibleSessions` 가 `idle` 세션을 숨기는 케이스 추가 (6h fallback 제거 확인).
   - 기존 `RECENT_ACTIVITY_WINDOW_MS` 관련 테스트 케이스는 새 정책과 충돌하면 제거 또는 수정.
2. **수동 확인**:
   - Codex/Claude 세션 감시 토글 → 카드 즉시 사라짐 확인.
   - 30 초 이상 대기 중인 세션 카드가 안 보이는지 확인.
   - 완료된 세션 카드 클릭 → dismiss 동작 확인.
   - 카드 UI 가 [chip] title 한 행 + 1줄 ellipsis 로 렌더링되는지 확인.
3. **빌드**:
   - `pnpm vitest run`
   - `cargo check --manifest-path src-tauri/Cargo.toml`
   - `pnpm build`

---

## Atomic Tasks

- [ ] [test] `src/state.test.ts` — `pickVisibleSessions` `waiting` 숨김 케이스 추가 (RED)
- [ ] [test] `src/state.test.ts` — `pickVisibleSessions` `idle` 숨김 케이스 추가 (RED)
- [ ] [impl] `src/state.ts` — visualState 기반 필터 + `RECENT_ACTIVITY_WINDOW_MS` 제거 (GREEN)
- [ ] [refactor] `src/state.ts` — 주석/정렬 정리
- [ ] [no-test: IPC 핫패스 동기 emit] `src-tauri/src/lib.rs` — `WATCH_APPS_EVENT` 상수 + `WatchAppsEvent` 구조체 + `cmd_set_watch_apps` 사이드채널 emit
- [ ] [impl] `src/App.tsx` `usePayload` — `pendingWatchRef` + `companion:watch_apps` listener + `companion:update` reconciliation 분기
- [ ] [no-test: CSS only] `src/App.tsx` `OverlayCard` — `.overlay-card__title-row` JSX 구조 변경
- [ ] [no-test: CSS only] `src/App.css` — `.overlay-card__title-row` 추가, project chip 스타일, title 단일행 ellipsis
- [ ] [no-test: docs] `CLAUDE.md` — "카드 표시 규칙" 섹션 갱신, 구현 메모에 v0.1.39 entry 추가
