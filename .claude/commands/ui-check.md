---
description: pet-companion 의 React UI 변경사항을 vite dev 서버(localhost:1420)에 띄워 실시간 시각 검증. /review 가 .tsx/.css 변경을 감지하면 알림으로 권장. 사용자가 명시적으로 호출.
argument-hint: [선택] 점검 시나리오 — 예 "overlay 카드", "settings 슬라이더". 비우면 변경된 컴포넌트 자동 추론
---

# /ui-check — React UI 시각 검증

너는 지금 pet-companion 의 React UI 변경을 **실제 브라우저에 띄워 시각적으로 검증**해야 한다. 입력: `$ARGUMENTS` (없으면 변경 컴포넌트 자동 추론).

## 한계 인지 (반드시 출력 첫 줄에 사용자에게 알림)

> ⚠️ **Tauri IPC 의존 기능은 브라우저 단독에서 동작하지 않습니다.**
> 본 명령은 순수 CSS / 컴포넌트 props / DOM 구조 / 레이아웃 검증에 한정됩니다.
> IPC (`invoke()`) 호출이 필요한 기능 (펫 스프라이트 로딩, 세션 데이터, 드래그 등) 은 `pnpm tauri dev` 로 실제 앱 실행 후 수동 확인이 필요합니다.

## 사전 작업

1. 변경 범위 파악:
   - `git diff` 으로 `.tsx`, `.css` 변경 파일 확인.
   - 변경 파일이 0개면 사용자에게 알리고 종료.
2. dev 서버 상태 확인:
   - `lsof -i :1420 -P -n | head -3` 또는 `curl -s -o /dev/null -w '%{http_code}' http://localhost:1420` 로 확인.
   - 응답 없으면 사용자에게 다음을 안내 후 종료:
     ```
     dev 서버가 떠있지 않습니다. 다른 터미널에서 다음을 실행하세요:
       pnpm tauri dev    (전체 Tauri 앱)
       pnpm dev          (Vite 만 — IPC 없는 순수 UI 검증용, 추천)
     서버가 뜨면 다시 /ui-check 를 호출하세요.
     ```

## 단계 1 — 브라우저 시작 + 페이지 로드

다음 순서로 시도. 첫 번째 성공한 도구를 그대로 사용한다.

### 우선순위 1: `mcp__playwright__browser_navigate`

- `url`: `http://localhost:1420`
- 성공하면 단계 2 진행.

### 우선순위 2: `mcp__Claude_Preview__preview_start` → `mcp__Claude_Preview__preview_screenshot`

- Playwright MCP 가 거부되거나 사용 불가하면 Claude Preview 로 fallback.

### 우선순위 3: 수동 안내

- 둘 다 실패 시 사용자에게 "수동으로 http://localhost:1420 열어 확인하세요" 안내 후 종료.

## 단계 2 — 시각적 점검

다음을 순서대로 수행:

1. **초기 렌더링 스크린샷** (`mcp__playwright__browser_take_screenshot` 또는 `mcp__Claude_Preview__preview_screenshot`).
2. **콘솔 에러 점검** (`mcp__playwright__browser_console_messages`):
   - JS 에러, React 경고(key 누락, hooks 위반 등), 404, Tauri IPC reject 메시지.
   - IPC reject 는 예상된 것 (브라우저에 Tauri runtime 없음) — 무시하고 사용자에게 명시.
3. **DOM 구조 점검** (`mcp__playwright__browser_snapshot`):
   - 변경된 컴포넌트가 실제 렌더링되는가? class/id 가 의도대로 적용?
   - 변경된 CSS 클래스가 DOM 에 붙어 있는가?
4. **인터랙션 시나리오** (변경 사항에 따라 선택):
   - hover, click, drag 등은 IPC 비의존 부분에 한해 시도.
   - 예: 설정창의 체크박스/슬라이더는 `cmd_set_*` 호출이 실패해도 onChange 핸들러는 동작 가능 → 시각적 토글 확인.

## 단계 3 — 리포트

```
## 🎨 /ui-check 리포트

> ⚠️ 본 점검은 IPC 비의존 UI 영역에 한정됨.

### 변경 컴포넌트
- (변경된 .tsx / .css 파일 리스트)

### 초기 렌더링
- 스크린샷: [있음/없음]
- 레이아웃 이슈: (있으면 명시)

### 콘솔
- 🔴 에러: N건 — (요약. IPC 관련은 별도 표시)
- 🟡 경고: N건 — (요약)
- ✅ Tauri IPC reject 는 정상 동작 (브라우저 환경)

### 인터랙션
- 시도한 시나리오: (예: "settings 슬라이더 드래그", "context-menu 열기")
- 결과: (정상 / 실패 / IPC 의존이라 검증 불가)

### 권고
- (있다면 CSS/props 수정 제안)
- IPC 의존 기능: `pnpm tauri dev` 로 수동 검증 필요한 항목 명시.
```

## 주의

- 본 명령은 코드 수정을 하지 않는다. 시각 검증과 리포트만.
- IPC 호출 실패는 브라우저 환경의 정상 동작. 사용자에게 혼란을 주지 않도록 명확히 분류.
- 콘솔 에러 분류 기준:
  - `__TAURI_INTERNALS__` 미정의, `invoke is not defined` → IPC 환경 이슈 (예상됨, 무시).
  - React 경고, 일반 JS 에러, 404 → 실제 이슈 (보고).
- 점검 종료 후 브라우저는 명시적으로 닫지 않는다 (사용자가 추가로 확인할 수 있도록).
