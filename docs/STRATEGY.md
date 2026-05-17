# Pet Companion — Strategy

> 이 문서는 새로 큰 기능을 기획할 때 `/plan` 명령이 컨텍스트로 읽는 전략 문서다.
> 자잘한 버그 수정·UX 조정에는 비워두어도 무방함 — `CLAUDE.md` 구현 메모로 충분.

## 1. 사용자가 얻는 가치

Pet Companion은 Claude Desktop / Codex Desktop을 사용하는 개발자가 **다른 앱에서 작업 중일 때도 AI 에이전트 세션 상태를 놓치지 않도록** 하는 macOS 메뉴바 동반자다.

- **멀티태스킹 인지**: 여러 Claude/Codex 세션을 병행할 때, 어떤 세션이 입력 대기 중인지 / 완료됐는지를 창 전환 없이 파악
- **포커스 전환 최소화**: 오버레이 카드 클릭 한 번으로 해당 앱 세션으로 즉시 이동
- **상태 시각화**: 펫 애니메이션으로 `idle / running / waiting / waving / jumping` 상태를 직관적으로 표시

## 2. 핵심 사용자 시나리오

1. **멀티 세션 병행**: Claude로 문서 작업 + Codex로 코드 작업을 동시에 돌릴 때, 어느 세션이 응답을 기다리는지 오버레이 카드로 파악
2. **장시간 작업 중 자리 이탈**: Codex에게 큰 작업을 시키고 다른 앱을 쓰다가, 작업이 완료되면 (macOS 알림 또는 오버레이 카드로) 돌아올 타이밍을 앎
3. **세션 전환**: 완료된 세션 카드를 클릭해 해당 Claude/Codex 창을 즉시 포커스

## 3. 경쟁 갭 분석 (Codex Pets / Claude Code Buddy 기준, 2026-05-17)

> 참조: https://help.apiyi.com/ko/codex-pets-vs-claude-code-buddy-feature-comparison-ko.html

### Pet Companion의 경쟁 우위

| 기능 | Pet Companion | Codex Pets | Claude Code Buddy |
|------|:---:|:---:|:---:|
| macOS 데스크톱 오버레이 | ✅ | ✅ | ❌ 터미널 |
| Claude + Codex 동시 추적 | ✅ **독보적** | Codex만 | Claude만 |
| 세션 카드 + 멀티 세션 | ✅ | ❌ | ❌ |
| 앱 포커스 클릭 | ✅ | ❌ | ❌ |
| 드래그 / detach | ✅ | ❌ | ❌ |
| 에이전트 상태 시각화 | ✅ 5상태 | ✅ 3상태 | ⚠️ 말풍선만 |
| 커스텀 스프라이트 | ✅ | ✅ | ❌ |

### 기능 갭 (우선순위 순)

| ID | 기능 | 임팩트 | 설명 |
|----|------|--------|------|
| GAP-1 | **macOS 알림** | 🔴 높음 | 세션이 `waiting` / `waving` 전환 시 macOS 알림. 화면 안 봐도 인지 가능 |
| GAP-2 | 펫 컬렉션 / 멀티 슬롯 | 🟡 중간 | 여러 펫 보관 + 전환. 현재는 단일 커스텀 펫 또는 bori fallback |
| GAP-3 | 상태별 행동 가이드 | 🟢 낮음 | 카드에 "지금 돌아가세요" 힌트 텍스트. 단순 UI 변경으로 해결 가능 |
| GAP-4 | 세션 통계 | 🟢 낮음 | 오늘 완료된 turn 수, 총 작업 시간 등. 차별화 가능 |
| GAP-5 | 펫 직접 대화 | ⚪ 미지수 | Buddy 스타일 @pet 대화. Anthropic이 삭제한 이유 미상 — 보류 |
| GAP-6 | 언어 감지 펫 힌트 | 🟢 낮음 | 커스텀 펫이 이미 존재하므로 우선순위 낮음 |

## 4. 성공 지표

- 메뉴바 앱 상시 켜둠 (macOS 로그인 시 자동 시작 포함) — 개인 사용 기준
- `waiting` 상태 알림 수신 후 30초 내 해당 세션으로 전환 빈도 증가
- 세션 카드 dismiss → 재활성화 사이클이 자연스럽게 동작

## 5. 트레이드오프 / 제약

- **macOS only** — Windows/Linux 대응은 비범위
- **Accessibility 권한 의존**: AppleScript 기반 창 포커스는 TCC 권한 필요. fallback(`open -a`) 필수
- **알림 권한 별도 필요**: macOS `UNUserNotificationCenter` 또는 AppleScript `display notification`은 사용자 허가 필요
- **노이즈 방지 우선**: 알림은 상태 전환 시 1회만. 반복/스팸 금지
- **드래그 핫패스 규칙 유지**: 사용자 입력 IPC에서 `state.model.lock().await` 직접 호출 금지 (CLAUDE.md 불변 규칙)

## 6. 로드맵 (구현 순서 제안)

1. **v0.2.0** — GAP-1: macOS 알림 (가장 높은 실용 가치)
2. **v0.2.x** — GAP-3: 상태별 행동 힌트 (카드 UI, 작업 소) 
3. **v0.3.0** — GAP-2: 펫 컬렉션 / 전환 UI
4. **미정** — GAP-4: 세션 통계 대시보드
