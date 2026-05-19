# Pet State → Sprite Row Mapping Design — 2026-05-19

드래그 방향 sprite row 정확화 + 캐논과 어긋난 `sleeping` 상태 제거.

## 요구사항

1. **드래그 방향 정확한 sprite**: 펫을 오른쪽으로 드래그할 때 hatch-pet 캐논 명세의 row 1 (`running-right`, 8 프레임)이, 왼쪽으로 드래그할 때 row 2 (`running-left`, 8 프레임)이 보여야 한다. 현재는 row 7 (`running` = 직립 working pose) + CSS `scaleX(-1)` 반전으로 좌우를 처리하고 있어 (a) 자세가 달리는 모션이 아니고 (b) CSS flip 으로 hand chirality 같은 비대칭 디테일이 깨진다.
2. **`sleeping` 상태 제거**: 현재 `App.tsx::STATE_ROWS` 의 `sleeping` 은 row 1을 가리키지만 캐논 명세상 row 1 은 `running-right`. 따라서 idle + 진행 중 세션 없음 상태에서 펫이 "자는 모습" 이 아니라 "달리는 모습" 으로 보인다. 캐논 명세에 `sleeping` 행이 없으므로 상태 자체를 제거하고 idle 로 흡수한다.

## 비범위

- **`failed` (row 5) / `review` (row 8) 트리거 추가**: 두 상태는 spritesheet 에 있지만 backend 가 트리거하지 않는 dead surface 다. 사용자 요청 범위가 아니므로 이번 작업에서 변경하지 않는다.
- **구버전 pet 호환성**: hatch-pet v2 캐논 (8×9, 9개 상태) 이전에 생성된 pet (예: 초기 `bori`) 은 row 1/2 가 빈 셀일 수 있다. 이번 작업은 캐논을 만족하는 pet (예: `mai-sakurajima-v2`) 기준으로 검증하고, 구버전 pet 의 빈 row 1/2 fallback (CSS flip 유지 등) 은 별도 follow-up.
- **방향 반전 시 frame phase 보존**: 좌→우 / 우→좌 반전 시 `useAnimationFrameCount` 의 `useEffect([state])` 가 frame 을 0 으로 리셋하는 것은 그대로 둔다 (보고 시 후속 작업).
- **드래그 IPC 흐름 변경 없음**: `cmd_begin_drag` / `companion:facing` / `run_drag_cursor_loop` / `cmd_finalize_drag_position` 모두 시그니처 그대로. v0.1.37 의 `initialFacingLeft` race 가드도 유지.

## 성공 조건

- 펫 idle 상태 (모든 세션 `in_progress=false`) → row 0 (조용한 idle, 6 프레임) 이 보인다. row 1 우향 달리기가 idle 시 더 이상 보이지 않는다.
- 펫을 오른쪽으로 드래그 → row 1 (running-right, 8 프레임) 애니메이션. 마우스 떼는 순간 effective state 로 복귀.
- 펫을 왼쪽으로 드래그 → row 2 (running-left, 8 프레임) 애니메이션 (CSS flip 이 아닌 실제 좌향 자산).
- 드래그 중 방향 반전 → row 1 ↔ row 2 즉시 전환.
- 기존 cargo test 20 개, vitest 57 개 모두 회귀 없이 통과.

---

## 설계

### Change 1 — Backend `PetAnimationState` enum 재구성

**파일**: `src-tauri/src/lib.rs`

**변경**:

- `enum PetAnimationState` (line ~91):
  - 변형 제거: `Sleeping`
  - 변형 추가: `RunningRight`, `RunningLeft`
  - serde attribute: `#[serde(rename_all = "lowercase")]` → `#[serde(rename_all = "snake_case")]`. 단일어 변형 (`Idle`, `Running`, `Waiting`, `Waving`, `Jumping`, `Review`, `Failed`) 은 동일하게 lowercase 로 직렬화. 새 다단어 변형은 `running_right` / `running_left`.
- `refresh_and_emit` 안 effective state 후처리 분기 (line ~1989-1997):
  ```rust
  let effective_state = if effective_state == PetAnimationState::Idle && !any_in_progress {
      PetAnimationState::Sleeping
  } else { ... };
  ```
  전체 블록 제거. `effective_state` 는 `latch` / `map_base_state(base_state)` 결과를 그대로 사용.
- `compute_base_state` / `map_base_state` 변경 없음 — 이들은 `Idle/Running/Waiting/Completed→Waving` 만 다루고 `Sleeping` 을 emit 한 적이 없다.

**테스트** (`src-tauri/src/lib.rs` 모듈 내 `#[cfg(test)]`):

- `PetAnimationState::RunningRight` 직렬화 결과가 `"running_right"` 인지 (`serde_json::to_string`).
- `PetAnimationState::RunningLeft` 직렬화 결과가 `"running_left"` 인지.
- 기존 변형들 (`Idle` → `"idle"` 등) 이 그대로인지 (snake_case 가 단일어에서 lowercase 와 동일함을 회귀 보장).

### Change 2 — Frontend `PetAnimationState` union 재구성

**파일**: `src/types.ts`

**변경**:

```ts
export type PetAnimationState =
  | "idle"
  | "running"
  | "running_right"
  | "running_left"
  | "waiting"
  | "waving"
  | "jumping"
  | "review"
  | "failed";
```

`"sleeping"` 제거, `"running_right"` / `"running_left"` 추가. backend serde 결과와 1:1 일치.

### Change 3 — `STATE_ROWS` 매핑 갱신

**파일**: `src/App.tsx` (line ~61)

**변경**:

```ts
const STATE_ROWS: Record<PetAnimationState, { frames: number; fps: number; row: number }> = {
  idle:          { row: 0, frames: 6, fps: 3 },
  running_right: { row: 1, frames: 8, fps: 8 },
  running_left:  { row: 2, frames: 8, fps: 8 },
  waving:        { row: 3, frames: 4, fps: 6 },
  jumping:       { row: 4, frames: 5, fps: 8 },
  failed:        { row: 5, frames: 8, fps: 4 },
  waiting:       { row: 6, frames: 6, fps: 2 },
  running:       { row: 7, frames: 6, fps: 7 },
  review:        { row: 8, frames: 6, fps: 3 },
};
```

`sleeping` 행 삭제. `running_right` (8 프레임, 120ms/frame ≈ 8fps) 과 `running_left` (동일) 추가. 다른 행은 기존 값 유지.

### Change 4 — `stateLabel` 라벨 갱신

**파일**: `src/App.tsx::stateLabel` (line ~191)

**변경**: `sleeping` 항목 제거. `running_right` / `running_left` 추가. 라벨은 사용자에게 거의 노출되지 않지만 (드래그 중 카드 상태 라벨이 보이는 짧은 순간 정도) 타입 시스템을 만족시키기 위해 필요.

- en: `running_right: "Running right"`, `running_left: "Running left"`
- ko: `running_right: "달리는 중 (우)"`, `running_left: "달리는 중 (좌)"`

### Change 5 — 드래그 흐름에서 `flipHorizontal` 제거 + state row override

**파일**: `src/App.tsx::OverlayApp` (line ~1112), `src/App.tsx::PetSprite` (line ~558)

**변경**:

1. `OverlayApp` 의 drag state 결정:
   ```tsx
   const facing = usePetFacing();
   ...
   <PetSprite
     state={
       facing.isDragging
         ? (facing.facingLeft ? "running_left" : "running_right")
         : payload.overlay.effectiveState
     }
     pet={payload.overlay.pet}
     onLoaded={...}
   />
   ```
   `flipHorizontal` prop 제거.

2. `PetSprite` 시그니처:
   - `flipHorizontal?: boolean` 제거.
   - 인라인 style 에서 `transform: flipHorizontal ? "scaleX(-1)" : undefined` 라인 제거.
   - row 7 single-direction 가정을 설명하는 주석 (line ~626-628) 제거.

### Change 6 — 백엔드 unit test 케이스의 `Sleeping` 참조 정리

`src-tauri/src/lib.rs` 의 기존 단위 테스트 / fixture 가 `PetAnimationState::Sleeping` 을 직접 참조하는 곳이 없는지 확인하고 있으면 제거. 현재 grep 결과로는 enum 정의 + effective_state 후처리 분기 2 곳 외에는 참조가 없으나 컴파일 시 dead code / type 에러로 잡힐 것.

---

## 회귀 위험

### R1 — v0.1.34 entry 의 "단방향 스프라이트는 CSS scaleX(-1)" 불변 규칙 폐기

v0.1.34 CLAUDE.md entry 는 "스프라이트 시트는 right-facing running row(row 7) 하나뿐이므로 row 스왑이 아니라 CSS transform: scaleX(-1) 로 좌우 반전한다" 는 불변 규칙을 명시했다. 이번 변경은 그 가정 자체를 폐기 — 캐논 명세는 row 1/2 양방향 자산을 정의하고 있다. **/journal 단계에서 CLAUDE.md 의 v0.1.34 entry 에 deprecation 노트를 붙이고 새 entry 로 row 1/2 직접 사용 정책을 기록한다.**

### R2 — 구버전 hatch-pet pet 의 빈 row 1/2

hatch-pet v2 캐논 도입 이전에 생성된 pet (예: 초기 `bori`) 은 row 1/2 가 빈 셀일 수 있다. 사용자가 그런 pet 을 선택하고 드래그하면:

- row 1 / row 2 영역이 비어 있어 `backgroundPosition` 이 transparent 또는 wrong content 영역을 가리킨다.
- 결과적으로 "드래그 중 펫이 깜빡임 / 사라짐 / 다른 캐릭터 노출" 로 보일 수 있다.

**완화책**: 이번 작업은 캐논 호환 pet 기준 검증. 구버전 pet 호환은 별도 follow-up. /journal 에서 CLAUDE.md 에 "v2 캐논 이전 pet 사용 시 회귀 가능 — pet 재생성 권장" 노트 추가.

### R3 — TypeScript breaking change

`PetAnimationState` union 에서 `"sleeping"` 제거. 외부 코드가 `"sleeping"` 리터럴 비교를 하면 컴파일 에러. 현재 grep 결과로는 다음 3 파일만 해당:

- `src/types.ts` — 정의
- `src/App.tsx` — `STATE_ROWS`, `stateLabel` 만 참조 (이번 PR 에서 함께 수정)
- `src/state.ts` — `pickVisibleSessions`, `sessionVisualState`, `dismissDecision` 에서 sleeping 미사용 (변경 없음)

새 변형 `"running_right"`, `"running_left"` 추가는 union 확장이라 기존 비교 코드를 깨지 않는다 (exhaustive switch 도 없음 — `STATE_ROWS` 가 record 라 모든 변형 정의 필요).

### R4 — 백엔드 enum 변경 시 직렬화 회귀

`#[serde(rename_all = "lowercase")]` → `"snake_case"` 변경은 단일어 변형 (`Idle` → `"idle"`) 에는 영향 없음. 단, 외부 (CLAUDE.md, 테스트 fixture, frontend `types.ts`) 에서 `"sleeping"` literal 을 검색하면 발견되는 위치는 모두 이 PR 안에서 함께 제거. cargo test 안의 serde encoding 테스트가 회귀를 잡는다.

### R5 — 빠른 좌우 흔들기 시 frame=0 reset hitch

`useAnimationFrameCount` 의 `useEffect([state])` 가 state 전환마다 frame 을 0 으로 리셋하므로 사용자가 드래그 중 빠르게 좌우 방향을 흔들면 매번 row 1/2 의 frame 0 (정지 자세) 부터 다시 시작. 보통 사용자가 한 번 정해진 방향으로 드래그하므로 영향 미미. 보고 시 phase 보존 로직 (`useRef<frame>` + state 전환 시에도 frame 유지) 추가.

---

## 검증 / 테스트 전략

### Rust 단위 테스트 (`cargo test --manifest-path src-tauri/Cargo.toml`)

기존 20 개 테스트 회귀 없이 통과해야 한다. 신규:

- **[test]** `serde_json::to_string(&PetAnimationState::RunningRight)` 가 `"\"running_right\""` 인지 검증.
- **[test]** `serde_json::to_string(&PetAnimationState::RunningLeft)` 가 `"\"running_left\""` 인지 검증.
- **[test]** (회귀 가드) `PetAnimationState::Idle` 가 `"\"idle\""` 로 직렬화 — snake_case 가 단일어에서 lowercase 와 동일함을 보장.

### TS 단위 테스트 (`pnpm vitest run`)

기존 57 개 테스트 회귀 없이 통과해야 한다. `pickVisibleSessions` / `sessionVisualState` / `dismissDecision` 변경 없음 → 추가 케이스 불필요.

### 수동 검증 (debug bundle 권장)

`pnpm tauri build --debug` 후 `src-tauri/target/debug/bundle/macos/Pet Companion.app` 실행:

1. 펫 idle 상태 (모든 세션 `in_progress=false`) → row 0 (조용한 idle, 6 프레임) 표시. row 1 우향 달리기가 idle 시 보이지 않음.
2. 펫을 마우스로 우측 60px 이상 드래그 → row 1 (running-right) 8 프레임 애니메이션. 마우스 떼는 순간 effective state 로 즉시 복귀.
3. 펫을 좌측 60px 이상 드래그 → row 2 (running-left) 8 프레임 애니메이션. 자산은 CSS flip 이 아닌 실제 좌향 그림.
4. 드래그 중 방향 반전 (좌→우 또는 우→좌, hysteresis 4px 이상) → row 1 ↔ row 2 즉시 전환.
5. 드래그 종료 → 즉시 `payload.overlay.effectiveState` 로 복귀. 진행 중 세션 카드가 표시 시 row 7 working pose 유지.
6. 캐논 비호환 pet (예: 구버전 `bori`) 선택 시 드래그 → row 1/2 자산이 비어 있으면 transparent / wrong cell 노출. R2 의 follow-up 으로 처리.

### 빌드 검증

- `cargo check --manifest-path src-tauri/Cargo.toml` clean (unused warning 없음 — `Sleeping` 변형이 완전 제거되어야 dead variant warning 안 남는다).
- `pnpm build` clean.

---

## Atomic Tasks

> writing-plans 단계에서 채워진다. design 승인 후 `/plan` 의 단계 3 으로 진행.
