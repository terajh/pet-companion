# Pet Companion

- macOS 전용 Tauri 2 메뉴바 companion 앱.
- 목적: `Claude Desktop` 및 `Codex Desktop`의 로컬 세션 상태를 따라다니는 펫 오버레이를 표시한다.
- 펫 자산은 `~/.codex/pets`를 직접 읽는다.
- 기본 펫 소스는 `Codex`에서 선택된 `custom` 펫이며, fallback은 `bori`다.

## 현재 구현 범위

- 메뉴바 앱 + 투명 오버레이 창 + 설정 창
- `Claude` / `Codex` 자동 추적
- 자동 추적 규칙:
  - 포커스된 앱이 `Claude` 또는 `Codex`면 그 앱 우선
  - 둘 다 아니면 마지막으로 포커스된 앱 유지
  - 그것도 없으면 전체 세션 중 `lastActivityAt` 최신 세션 fallback
- 앱 선택 모드:
  - 자동 추적
  - `Claude`만 추적
  - `Codex`만 추적
  - `Claude` 세션 고정
  - `Codex` 세션 고정
- `Codex` 세션 선택:
  - `active-workspace-roots` 와 세션 `cwd`를 먼저 매칭
  - 매칭 실패 시 `Codex` 전체 최신 세션 fallback
- 상태 매핑:
  - 핵심 5상태: `idle`, `running`, `waiting`, `waving`, `jumping`
  - `completedTurns` 증가 시 완료 애니메이션 재생
- 카드 표시 규칙:
  - 진행 중 상태는 앱 포커스 여부와 관계없이 표시
  - 완료/유휴 상태는 기본적으로 카드 표시
  - **dismissed 정책**: 오버레이 카드를 클릭하면 (`cmd_focus_active_session` 호출 시점) 해당 세션의 `effective_state`가 `waving` 또는 `idle`이면 그 `session_id`를 `RuntimeModel.dismissed_sessions`에 등록한다. dismissed된 세션은 카드를 표시하지 않는다.
  - **dismissed 해제 조건**: 오직 해당 세션이 다시 `in_progress = true` (`Running` 또는 `Waiting`) 상태가 되었을 때만 해제. 다른 세션 전환, 새 turn 등으로는 해제되지 않는다.
  - dismissed 세션이 archive되거나 사라지면 자동으로 정리된다.
- 클릭 동작:
  - 단일 클릭(펫): 반응 애니메이션
  - 오버레이 카드 클릭: 해당 앱 세션 포커스 + Completed/Idle이면 카드 dismissed
  - 드래그: detached
  - 더블클릭: Claude/Codex 창에 재부착
  - 우클릭: 자동 추적 / 앱 고정 / 세션 고정 / 설정 메뉴

## 세션 데이터 소스

### Claude

- 메타데이터 + 메시지 프리뷰 (단일 소스):
  - `~/.claude/projects/<project-dir>/<uuid>.jsonl`
  - 각 디렉터리 = 프로젝트, 파일 stem = `cliSessionId`
  - 각 라인: `{"type":"user"|"assistant", "sessionId":"...", "cwd":"...", "timestamp":"ISO-8601", "message":{"role":"...", "content":...}, ...}`
  - `sessionId` 필드는 대부분 라인에 존재 → 첫 등장값 사용
  - `lastActivityAt` = 라인 중 가장 최근 `timestamp` (ISO-8601 파싱)
  - `completedTurns` = assistant 메시지 개수
  - `title` = 첫 user 메시지 텍스트 → fallback: `cwd` basename
  - `~/Library/Application Support/Claude/claude-code-sessions` 경로는 이 머신에 존재하지 않아 폐기

### Codex

- 메타데이터:
  - `~/.codex/sessions/**/*.jsonl`
- 전역 상태:
  - `~/.codex/.codex-global-state.json`
- 세션 선택:
  - `active-workspace-roots` 와 세션 `cwd`를 먼저 매칭
  - 매칭 실패 시 `Codex` 전체 최신 세션 fallback
- 제목 우선순위:
  - 최근 `user_message`
  - `task_complete.last_agent_message`
  - `cwd` basename
- 프리뷰 우선순위:
  - 진행 중: `user_message`
  - 완료 후: `task_complete.last_agent_message`
  - 보조: `agent_message`

## 실행 / 빌드

```bash
pnpm install
pnpm tauri dev
```

```bash
pnpm build
cargo check --manifest-path src-tauri/Cargo.toml
pnpm tauri build --debug
```

디버그 번들:

- `src-tauri/target/debug/bundle/macos/Pet Companion.app`

## 주요 파일

- 프런트:
  - `src/App.tsx`
  - `src/App.css`
  - `src/types.ts`
- 백엔드:
  - `src-tauri/src/lib.rs`
  - `src-tauri/tauri.conf.json`
  - `src-tauri/Cargo.toml`

## 구현 메모

- 오버레이 스프라이트는 프런트에서 직접 파일 경로를 읽지 않고, Rust 커맨드가 파일을 읽어 `data:` URL로 넘긴다.
- 긴 세션 제목/메뉴 텍스트는 오버레이 창을 넘지 않도록 CSS ellipsis와 메뉴 위치 clamp를 사용한다.
- `Claude`/`Codex` 앱 배지를 카드에 붙여 현재 어떤 앱 세션을 보고 있는지 구분한다.
- Accessibility 권한이 없으면 detached 모드만 허용하고, 설정 창에서 시스템 설정 이동 버튼을 노출한다.
- **창 위치 좌표계 주의**: `window.outer_position()`은 물리 픽셀(PhysicalPixel)을 반환하지만 `set_position(LogicalPosition)`은 논리 좌표가 필요하다. Retina(2×) 디스플레이에서는 `/ scale_factor()`로 변환 필수. `ensure_detached_overlay_visible`에서 이 변환을 수행한다.
- **Codex 포커스**: AppleScript의 `tell process "Codex"` 프로세스명이 Codex 배포판에 따라 다를 수 있다. `focus_codex_window_by_title`은 `["Codex", "Codex CLI"]` 후보를 순차 시도하며, 각 시도의 stderr를 `eprintln!`으로 출력한다.
- **attached 모드 safe-home fallback**: 권한 미부여 또는 앱이 frontmost가 아닐 때 오버레이는 `move_overlay_to_safe_home`(primary monitor 우하단)으로 이동하여 항상 화면에 표시된다.
- **Codex 세션 중복 카드**: 같은 workspace(`cwd`)에 여러 `rollout-*.jsonl` 파일이 생기면 `session_id`가 달라 첫 번째 dedup을 통과해버린다. `rebuild_payload`에서 `dedup_sessions_by_id` 이후 `dedup_sessions_by_workspace`를 추가로 실행하여 `(app_kind, cwd)` 기준으로 한 번 더 합친다. `cwd`가 빈 세션은 `session_id`를 키로 사용해 패스스루한다.
- **스프라이트 안 보임 (🐾 placeholder 고착)**: Tauri 2 IPC는 커맨드 파라미터를 구조체로 래핑(`input: SpriteInput { path }`)할 경우 프론트에서 `{ input: { path } }` 형태로 전달해야 하지만, 기존 코드는 `{ path }` 형태로 넘겼다. 에러 메시지: `missing required key input`. `cmd_read_pet_sprite_data_url(input: SpriteInput)` → `cmd_read_pet_sprite_data_url(path: String)`으로 변경하여 해결.
- **스프라이트 로딩 타이밍**: 오버레이 WebView가 마운트될 때 `cmd_get_app_payload`를 호출하는데, Rust의 첫 `refresh_and_emit`이 아직 끝나지 않은 경우 `current_payload`가 None이라 에러를 반환했다. `cmd_get_app_payload`에서 최대 3초(50ms × 60) 폴링하도록 수정하여 race condition 해소.

## 구현 메모 (추가)

- **in_progress 고착 (B-medium 규칙)**: Codex 세션이 mid-turn에 종료되면 `task_complete` 없이 `in_progress=true`가 유지된다. `clear_stale_in_progress` 함수가 `rebuild_payload`에서 `dedup_sessions_by_workspace` 후에 실행되며, (1) 앱 프로세스가 실행 중이 아닌 경우 또는 (2) `last_activity_at`이 3분 이상 오래된 경우 `in_progress = false`로 강제 전환한다. 프로세스 실행 여부는 JXA 스크립트에서 `System Events` processes 목록을 통해 감지하며 `FrontWindowState.claude_running / codex_running`에 저장한다.
- **카드 프리뷰 (P2+dedup 규칙)**: `sessionPreview`에서 active 세션은 `payload.overlay.messagePreview`를 사용하고, 비활성 세션은 `inProgress ? userPreview : assistantPreview ?? completedPreview`를 사용한다. 후보 프리뷰가 세션 title과 normalizeText 기준으로 동일하면 `null`을 반환해 프리뷰 행을 숨긴다.
- **스프라이트 크롭 (canonical projection)**: `PetSprite`에서 `naturalWidth/naturalHeight` 기반 scaleX/scaleY 계산을 제거하고 canonical 8×9 그리드 (`canonicalW = SPRITE_WIDTH * 8`, `canonicalH = SPRITE_HEIGHT * 9`)로 고정. `background-size`를 canonical 사이즈로 지정하면 retina 처리는 브라우저 device pixel ratio가 자동으로 담당한다.
- **카드 접기 버튼**: `OverlayApp`에 `collapsed: boolean` 상태 추가. `.pet-shell` 우측 상단에 22px 원형 버튼 배치 (절대 위치 `top: -8px; right: -8px`). `data-tauri-drag-region="false"` 설정으로 drag region이 클릭 이벤트를 삼키지 않도록 처리. 접힌 상태에서는 카드 개수, 펼친 상태에서는 "▾" 표시.
- **세션 카드 클릭 창 포커스 (K1 matching chain)**: `focus_claude_window_by_title` / `focus_codex_window_by_title` 시그니처를 `(cwd: &str, title: &str)`로 변경하고 `try_raise_window_applescript` 헬퍼로 2단계 AppleScript 매칭을 수행한다. Step 1: `cwd` basename으로 `System Events → tell process → first window whose title contains <basename> → AXRaise`. Step 2: title 앞 20자로 동일한 방식 재시도. 두 단계 모두 실패하면 `open -a`(LaunchServices) fallback. stderr에 `-1743` 포함 시 Automation 권한 미부여로 판단하여 `eprintln!`으로 명시 경고 후 즉시 fallback. Codex는 `["Codex", "Codex CLI"]` 두 프로세스명을 순차 시도한다. **TCC 주의**: `pnpm tauri dev`는 매 빌드마다 재서명하여 Automation TCC가 무효화되므로 AppleScript가 -1743으로 실패한다. 디버그 번들(`src-tauri/target/debug/bundle/macos/Pet Companion.app`)은 서명이 안정적이어서 Automation 권한을 한 번만 승인하면 유지된다.
- **드래그 snap-back 수정 (backend-side drag detection)**: JS `pointermove` 이벤트는 OS 주도 `data-tauri-drag-region` 드래그 중 창-로컬 좌표가 거의 변하지 않아 신뢰할 수 없다. 대신 Rust `WindowEvent::Moved` 핸들러에서 드래그를 감지한다. `RuntimeModel`에 `expected_attached_position: Option<(i32, i32)>` 필드를 추가하고, `sync_overlay_window`가 `set_position`을 호출할 때마다 해당 논리 좌표를 반환하여 호출측(`refresh_and_emit`)이 모델에 기록한다. `move_overlay_to_safe_home`도 동일하게 기록한다. `Moved` 핸들러에서 `!detached`인 경우 새 위치와 `expected_attached_position`의 차이가 **6 논리 픽셀** 초과이면 사용자 드래그로 판단하여 `detached=true`로 전환하고 `refresh_and_emit`을 호출한다. 6px 이하의 delta는 프로그래밍적 `set_position` 에코로 간주하여 무시하므로 단일 클릭이 드래그로 오인되지 않는다.
- **Claude `in_progress` 판별 (B-T1-R1 규칙)**: Claude `.jsonl` 파일에는 Codex의 `task_start`/`task_complete` 마커가 없다. 대신 `read_claude_sessions` (`lib.rs`)에서 라인 순회 중 `type == "user"` 타임스탬프의 최댓값(`latest_user_at`)과 `type == "assistant"` 타임스탬프의 최댓값(`latest_assistant_at`)을 `i64`로 추적한다. 루프 후 `in_progress = latest_user_at > latest_assistant_at`으로 계산한다. 파싱 가능한 타임스탬프가 없으면 두 값 모두 0이어서 `0 > 0 = false`로 자연스럽게 처리된다. 임계값 레이어는 추가하지 않았으며, 3분 비활성 stale 정리는 기존 `clear_stale_in_progress` (B-medium 규칙)가 담당한다. 카드 최대 표시 수는 `MAX_VISIBLE_CARDS` 4 → **6**으로 상향하여 Claude·Codex 카드가 동시에 표시될 여유를 확보한다.
- **카드 클릭 포커스 백그라운드화 (P3)**: `cmd_focus_session_by_id`는 dismissed 처리(`dismissed_sessions.insert`)만 모델 락 안에서 동기로 수행하고, AppleScript window raise(`focus_codex_window_by_title` / `focus_claude_window_by_title`)와 후속 `refresh_and_emit`은 `tauri::async_runtime::spawn`으로 백그라운드 발사한다. IPC가 즉시 `Ok(())`를 반환하므로 카드 클릭 → dismissed 반영이 즉각 일어나고, 실제 창 raise는 곧이어 따라온다. `State<'_, AppState>` 캡처는 `app.clone()`한 뒤 spawned block 안에서 `app_clone.state::<AppState>()`로 획득하는 방식을 사용했다(`WindowEvent::Moved` 핸들러와 동일한 패턴).
- **드래그 위치 드리프트 방지**: `ensure_detached_overlay_visible`이 매 refresh마다 `clamp_overlay_position` → `set_position`을 호출하던 것을 "창이 모든 모니터 작업 영역과 40px 미만 오버랩일 때만" 재배치하도록 좁힘. 정상 위치(어느 모니터에서든 40px 이상 겹침)는 건드리지 않으므로 사용자가 드래그한 위치가 틱마다 리셋되지 않는다. 이동이 필요한 경우에만 `config.detached_position`을 갱신하고 `persist_config`를 호출한다.
- **카드 다중 노출 (CLAUDE.md 정책 일치)**: `pickVisibleSessions`가 `in_progress` 또는 (`!archived && !dismissed`)인 모든 세션을 노출하도록 변경. 기존엔 비-진행 세션을 active 1개만 노출하던 것을 정정. dismissed → in_progress 부활 규칙은 변경 없음.
- **앱 이름 변경 (Claude Pet Companion → Pet Companion)**: 사용자 가시 영역은 모두 "Pet Companion"으로 변경. Bundle identifier(`com.carterp.claudepetcompanion`)와 Cargo crate 이름(`claude-pet-companion`)은 TCC 권한 / 빌드 경로 안정성을 위해 유지. 빌드 산출물 경로는 `src-tauri/target/debug/bundle/macos/Pet Companion.app`.
- **카드 가독성 개선**: `.overlay-card` 패딩 `8px 10px` → `10px 12px`, `min-height: 64px` 추가. `.overlay-card__title` 단일행 ellipsis에서 `-webkit-line-clamp: 2` 멀티라인으로 변경. `.overlay-card__preview` line-clamp `1` → `2`. `.overlay-card-stack` gap `6px` → `8px`, width `min(232px, …)` → `min(280px, …)`.

- **컨텍스트 메뉴 단순화**: 우클릭 메뉴에서 "설정 열기" 버튼 하나만 남기고 나머지(포커스, 재부착, 앱/세션 핀 선택) 항목을 전부 제거. `MENU_WIDTH` 248→140px, 폰트 12px→10.5px, 패딩 축소.
- **캐릭터 크기 슬라이더 (pet_scale)**: `PersistedConfig.pet_scale: f32` (기본 1.0, 범위 0.5~2.0) 추가. `cmd_set_pet_scale` IPC 커맨드로 저장+emit. 오버레이 `.overlay-root`에 `--pet-scale` CSS 변수로 주입. `.pet-shell` 크기 및 `.overlay-card-stack` bottom 위치가 이 변수에 연동되고, `PetSprite`에 `transform: scale(var(--pet-scale, 1))` 적용. 설정창 `petSource` 섹션에 슬라이더 UI 추가.
- **설정창 클릭 무반응 수정 (IPC 래퍼 미스매치 + CSS pointer-events 누수)**: 슬라이더 무반응의 근본 원인은 `cmd_set_pet_scale` 프론트 호출이 `{ scale }` 단독 전달이었으나 Rust 시그니처가 `input: PetScaleInput { scale }` 래퍼를 요구해 IPC가 무음 실패했음. `{ input: { scale } }`로 수정. 체크박스 클릭 불가는 `.overlay-root`의 `user-select: none`이 같은 CSS 번들을 쓰는 settings 창에서도 엘리먼트 선택을 방해할 수 있는 CSS 누수가 원인. `.settings-root`에 `pointer-events: auto !important` 및 `.settings-root *`에 `pointer-events: auto; user-select: auto` 명시 추가로 차단. **같은 실수 방지**: Rust 커맨드에 구조체 인자가 있을 때는 반드시 `{ input: { ... } }` 래핑 여부를 확인할 것 (기존 `SpriteInput` 미스매치 선례와 동일 패턴).
- **오버레이 hit-test (투명 영역 click-through)**: `setIgnoreCursorEvents` 동적 토글로 투명 배경은 click-through, 인터랙티브 요소(.pet-shell, .overlay-card-stack, .context-menu, .menu-backdrop)만 마우스를 수신하도록 구현.
  - **CSS 레이어**: `.overlay-root`에 `pointer-events: none` 기본값 설정. `.pet-shell`, `.menu-backdrop`, `.context-menu`에 `pointer-events: auto` 명시. `.overlay-card-stack > *`는 기존에 이미 `pointer-events: auto` 적용됨.
  - **JS 경로 이중화** (`useOverlayHitTest` 훅):
    1. **ignore=false (정상 hit-test)**: 네이티브 `mousemove` 이벤트를 수신 → `document.elementFromPoint()`로 hit 판단 → 투명 영역이면 `setIgnoreCursorEvents(true)` 전환.
    2. **ignore=true (pass-through)**: OS가 mousemove를 삼켜 이벤트가 오지 않음 → 50ms `setInterval`로 Rust `cmd_cursor_position_in_overlay` 폴링 → 인터랙티브 요소 위에 들어오면 `setIgnoreCursorEvents(false)` 재전환 → 이후 네이티브 mousemove가 복구.
  - **좌표계 변환 함정** (반드시 숙지):
    - `cursor_position()` → `PhysicalPosition<f64>`: 전역 화면 물리 픽셀 (macOS의 NSScreen 좌표계, 좌상단 원점, device pixel ratio 적용됨).
    - `outer_position()` → `PhysicalPosition<i32>`: 창 좌상단 모서리의 전역 물리 픽셀 좌표.
    - `scale_factor()` → `f64`: Retina 디스플레이에서 2.0, 일반 디스플레이에서 1.0.
    - **올바른 변환**: `logical_x = (cursor.x − origin.x) / scale`, `logical_y = (cursor.y − origin.y) / scale`.
    - 이 변환 결과가 `document.elementFromPoint(x, y)`의 인자(`clientX`/`clientY` 기준 논리 픽셀)와 일치한다. **scale_factor로 나누지 않으면** Retina에서 좌표가 실제보다 2배 큰 값이 되어 hit-test가 오동작한다.
    - **잘못된 예**: `(cursor.x − origin.x)` — scale 변환 누락 시 Retina 2배 오류.
    - **잘못된 예**: `LogicalPosition::new(cursor.x, cursor.y)` — origin 빼지 않으면 창-로컬 좌표가 아니라 전역 좌표가 됨.
  - **context menu 열림 시 ignore 비활성화 규칙**: `menu !== null`이면 `useOverlayHitTest(true)`로 호출되어 ignore를 강제로 false로 유지한다. 메뉴 backdrop이 화면 전체를 덮는데, ignore=true이면 backdrop 클릭이 뒤 앱 창으로 통과해서 메뉴가 닫히지 않는다. **메뉴 열림 상태에서는 반드시 ignore=false를 유지해야 한다.** 이를 위해 `useOverlayHitTest`의 `menuOpen` 파라미터를 항상 `menu !== null`로 전달한다.
  - **Capabilities 추가**: `core:window:allow-set-ignore-cursor-events` 권한을 `src-tauri/capabilities/default.json`에 추가. 이 권한 없이는 프론트에서 `setIgnoreCursorEvents()` 호출 시 "not allowed" 오류가 발생한다.
  - **미해결 위험 — 멀티 모니터 / 혼합 DPI**: 커서가 서로 다른 scale_factor를 가진 모니터 사이를 이동할 때, `outer_position()`과 `cursor_position()` 간의 좌표 계산에 단일 `scale_factor()`를 사용하면 좌표 오차가 발생할 수 있다. 현재 구현은 단일 scale_factor 가정이며, 멀티 모니터 혼합 DPI 환경에서는 hit-test가 약간 어긋날 수 있다. Tauri 2에서 모니터별 DPI를 가져오는 API(`MonitorHandle`)가 존재하나, 현재 단일 모니터 사용 환경에서는 충분하다.

- **AppleScript `set frontmost to true` 버그 수정 (v0.1.2)**: macOS Sequoia에서 `tell process "X"` 블록 안의 `set frontmost to true`가 프로세스 객체가 아닌 LHS 타입 불일치로 `-10006` 에러를 발생시킨다. `try_raise_window_applescript`의 Step 1과 Step 2 스크립트 모두 `set frontmost of process "{proc}" to true`(블록 밖 명시적 형식)로 수정. 이 패턴은 `tell process` 블록 안에서 `frontmost`를 직접 할당하는 모든 AppleScript에 동일하게 적용됨.
- **pet_scale 진단 로그 추가 (v0.1.2)**: `cmd_set_pet_scale` 진입 시 `eprintln!("[pet_scale] received scale=…")`로 IPC 수신 여부를 stderr에 출력. `persist_config`와 `refresh_and_emit` 실패 시도 각각 명시 로그 추가. 슬라이더 무반응 근본 원인 추가 분석: React CSS 커스텀 프로퍼티 inline style에서 숫자 값(`petScale: number`)이 아닌 문자열이어야 올바르게 적용되는 경우가 있어 `String(payload.config.petScale)` 형변환 추가. `PetScaleInput`에 `#[serde(rename_all = "camelCase")]`는 이미 적용되어 있어 IPC 직렬화 자체 문제는 없었음.
- **Codex 프로세스명 후보군 확장 (기존 적용 확인)**: `query_supported_front_windows` JXA 스크립트에서 `codexRunning` 판별 시 `"Codex"` 및 `"Codex CLI"` 둘 다 확인하는 코드가 이미 존재함(`procNames.indexOf("Codex") !== -1 || procNames.indexOf("Codex CLI") !== -1`). `clear_stale_in_progress`가 `codex_running` 플래그를 통해 이 결과를 사용하므로 별도 수정 불필요.
- **포커스 매칭 강화 (v0.1.3)**: `try_raise_window_applescript`에 세 가지 개선 추가.
  1. **프로세스 존재 사전 체크**: Step 1/2 실행 전 `tell application "System Events" to exists process "X"` 를 먼저 실행한다. `false`이면 조용히 스킵(`eprintln!("[focus] process X not running, skipping")`만 남김). 덕분에 "Codex CLI"가 없는 환경에서 Step 1/2가 -10006을 뿌리는 시끄러운 로그가 사라지고, 진짜 권한 거부(-1743)와 창 매칭 실패(-1719)만 명확하게 보인다.
  2. **Step 3 activate-only fallback 추가**: Step 1(cwd-basename), Step 2(title-prefix) 모두 실패해도 `set frontmost of process "X" to true`(창 매칭 없음)를 시도한다. Codex Desktop·Claude Desktop처럼 메인 창이 한 개인 앱에서는 이것만으로 포커스가 완성된다. -1719(invalid index) 같은 창 title 불일치 시 최후의 보루 역할. Step 3 성공 시 `[focus] Step 3 (activate process only) succeeded for X` 로그 출력.
  3. **early return 보장**: 어느 단계에서든 성공하면(또는 -1743 권한 거부 시) 즉시 반환하여 후속 후보 프로세스 시도를 건너뜀. Codex의 `["Codex", "Codex CLI"]` 순차 시도에서도 첫 번째가 성공하면 두 번째를 시도하지 않음.
  - **가정**: 이 패턴은 Claude Desktop / Codex Desktop처럼 메인 창이 1개인 앱에서 적합하다. 향후 Codex가 멀티 윈도우를 지원하면 Step 3가 잘못된 창을 frontmost로 만들 수 있음 — 미래 버전에서는 Step 1/2 매칭을 개선하거나 Step 3를 조건부로 비활성화할 것.
- **모니터 경계 드래그 oscillation 수정 (v0.1.25)**: WKWebView의 `event.screenY`는 모니터별 origin이 다르게 적용되어 (상단 모니터에서 음수 → 클램프 → 다시 음수) 위쪽 모니터 경계 부근에서 `cmd_set_overlay_position`이 두 위치를 깜빡거리며 클램프하는 oscillation이 발생. 해결: 드래그 전체를 Rust로 이관. 프런트는 단 한 번 `cmd_begin_drag(grabOffsetX, grabOffsetY)`만 호출하고, Rust가 `tokio::time::interval(16ms)`로 `window.cursor_position()`(단일 전역 좌표계 — 모니터 경계와 무관)을 폴링해서 직접 `set_position`을 부른다. 마우스 떼면 프런트는 `cmd_finalize_drag_position`을 호출, Rust가 cancel 신호 플립 → 최종 위치 스냅샷 → persist. **클램프 알고리즘은 `clamp_overlay_position`에서 모든 모니터 frame의 union bounding rect를 사용** — 모니터별 closest fallback은 logical-coord gap에서 펫을 source 모니터로 yank해버려 위쪽 모니터 진입을 막는다.
- **드래그 model-lock 경합 수정 (v0.1.27)**: v0.1.26에서 cursor-polling 루프는 살아있는데도 펫이 안 움직이는 회귀. 원인은 `cmd_begin_drag`가 `state.model.lock().await`(async `tokio::sync::Mutex`)를 잡으려고 대기 중일 때, 750ms `sync_overlay_window` 틱이 같은 락을 길게(~1.5s) 보유하고 있어 IPC 자체가 멈춤. 그 사이 user가 마우스를 떼면 `cmd_finalize_drag_position`이 큐에 쌓이고, `begin_drag`가 락을 따자마자 루프를 spawn → 직후 finalize가 cancel하여 루프가 **첫 틱에 EXIT**(`reason=cancel ticks=1`). 해결:
  1. **`drag_cancel` 핸들을 `RuntimeModel`에서 분리해 `AppState`의 `std::sync::Mutex<Option<Arc<AtomicBool>>>`로 이동**. begin/finalize는 이 가벼운 sync mutex로 cancel 신호를 즉시(마이크로초) 교환하므로 무거운 model lock을 거치지 않는다.
  2. **`cmd_begin_drag`의 무거운 작업(`detached=true` persist + `refresh_and_emit`)을 `tauri::async_runtime::spawn` 배경 태스크로 분리**. 핫패스는 `state.model.try_lock()`로 `pet_scale`만 즉시(컨텐션 시 1.0 fallback) 얻고 곧바로 루프 spawn. IPC는 마이크로초 안에 반환된다.
  3. **`cmd_finalize_drag_position`도 동일 패턴**: cancel 신호 플립 + 최종 위치 스냅샷은 즉시, persist + refresh는 배경 spawn.
  - **불변 규칙 (반드시 지킬 것)**: 사용자 입력 핫패스에서 `state.model.lock().await`를 직접 호출하지 말 것. 750ms refresh 틱이 같은 락을 잡고 있는 동안 IPC가 ≥1s 지연되어 다음 IPC가 큐잉되는 패턴이 재발한다. 가벼운 cancel 신호나 단일 값 조회는 `AppState`의 별도 `std::sync::Mutex` 슬롯이나 `try_lock` fallback을 사용하고, 무거운 작업은 배경 spawn으로 분리해 IPC를 즉시 반환한다.
- **진단 로그 정리 (v0.1.28)**: v0.1.25 oscillation 추적용 임시 `cmd_diag_log` IPC + 프런트 `diagLog` helper + `[drag-diag]` 라인 + `[clamp]` 매-틱 dump 제거. 남긴 마커는 (1) `[drag-loop] EXIT reason=… ticks=…`(루프 lifetime 한 줄, "안 움직임" 회귀 즉시 판별), (2) rate-limited cursor/scale/set_position 에러, (3) `[set_position] requested=… actual=…`(클램프 1px 초과 시), (4) `[set_position] ns_window class/level`(`std::sync::Once` 프로세스당 1회).
- **Claude 세션 타이틀 갱신 + 프로젝트 라벨 (v0.1.29)**: Claude `.jsonl`에는 사용자가 Claude Desktop 사이드바에서 세션 이름을 명시적으로 바꾸면 `{"type":"custom-title","customTitle":"…","sessionId":"…"}` 라인이 매번 누적된다(이름 변경 1회당 한 줄이 아니라 여러 번 기록될 수 있다 — 같은 값으로도 반복 등장). `read_claude_session_file`이 이 라인을 순회하며 `customTitle`을 매번 덮어써(가장 마지막 등장 값이 최종 타이틀) 새 `custom_title: Option<String>` 변수에 보관한다. 최종 title 우선순위: **(1) custom_title → (2) first_user_text → (3) cwd basename**. `<command-message>…</command-message>\n<command-name>/dev-post</command-name>\n<command-args>…</command-args>` 같은 슬래시 커맨드 raw XML이 title에 그대로 보였던 회귀가 사용자가 한 번이라도 이름을 바꾼 세션에서는 자연히 해소된다(슬래시 커맨드 정제는 별도 이슈로 남김 — `extract_preview_text`는 XML 태그를 strip하지 않는다). Codex는 `custom-title` 라인이 없어 변경 없이 `latest_user`를 유지(매 turn마다 갱신). 카드 UI는 `cwd` basename을 작은 회색 라벨(`.overlay-card__project`, 10px / `rgba(255,255,255,0.55)` / `nowrap+ellipsis`)로 title 위에 추가 표시하여 어느 프로젝트 기반 세션인지 한눈에 보이게 한다 (예: `claude-pet-companion` / `pet companion`). 프런트 헬퍼 `projectLabel(cwd)`는 `/` 분할 후 마지막 세그먼트만 반환한다.

## 작업 규칙

- 컴파운드 엔지니어링을 참고해서 문제 해결한 내역이 있으면, 같은 실수를 반복하지 않도록 이 파일 또는 관련 문서에 반드시 남긴다.
- 세션 추적 규칙, 프리뷰 우선순위, 펫 로딩 방식 같은 런타임 판단 로직을 바꿨다면 `CLAUDE.md`도 함께 갱신한다.
- `Codex`/`Claude` 로컬 파일 포맷에 의존하는 로직을 수정할 때는 샘플 데이터 경로와 fallback 규칙을 같이 적는다.
