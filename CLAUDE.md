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

## 작업 규칙

- 컴파운드 엔지니어링을 참고해서 문제 해결한 내역이 있으면, 같은 실수를 반복하지 않도록 이 파일 또는 관련 문서에 반드시 남긴다.
- 세션 추적 규칙, 프리뷰 우선순위, 펫 로딩 방식 같은 런타임 판단 로직을 바꿨다면 `CLAUDE.md`도 함께 갱신한다.
- `Codex`/`Claude` 로컬 파일 포맷에 의존하는 로직을 수정할 때는 샘플 데이터 경로와 fallback 규칙을 같이 적는다.
