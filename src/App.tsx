import { enable, disable, isEnabled } from "@tauri-apps/plugin-autostart";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { useEffect, useRef, useState } from "react";
import type {
  AppPayload,
  PetAnimationState,
  PetDescriptor,
  SessionAppKind,
  SessionSummary,
} from "./types";
import {
  dismissDecisionForVisualState,
  pickVisibleSessions,
  sessionVisualState,
} from "./state";
import "./App.css";

const windowLabel = getCurrentWebviewWindow().label;

const INITIAL_PAYLOAD: AppPayload = {
  config: {
    attached: true,
    language: "ko",
    trackedApp: "auto",
    petOverrideId: null,
    petScale: 1.0,
    watchClaude: true,
    watchCodex: true,
    petHidden: false,
  },
  codexSelectedPetId: null,
  overlay: {
    activeSession: null,
    claudeFrontmost: false,
    codexFrontmost: false,
    currentWindowTitle: null,
    effectiveState: "idle",
    messagePreview: null,
    permissionGranted: false,
    pet: {
      description: "",
      displayName: "Bori",
      id: "bori",
      spriteSheetPath: "",
      source: "custom",
    },
    sessions: [],
    showCard: false,
    stateLabel: "Idle",
    dismissedSessionIds: [],
    completedRuntimeSessionIds: [],
    cardsBelow: false,
  },
  pets: [],
};

const STATE_ROWS: Record<
  PetAnimationState,
  { frames: number; fps: number; row: number }
> = {
  idle: { row: 0, frames: 6, fps: 3 },
  sleeping: { row: 1, frames: 6, fps: 2 },
  running: { row: 7, frames: 6, fps: 7 },
  waiting: { row: 6, frames: 6, fps: 2 },
  waving: { row: 3, frames: 4, fps: 6 },
  jumping: { row: 4, frames: 5, fps: 8 },
  review: { row: 8, frames: 6, fps: 3 },
  failed: { row: 5, frames: 8, fps: 4 },
};

const SPRITE_WIDTH = 96;
const SPRITE_HEIGHT = 104;
const MENU_WIDTH = 140;
const MENU_MAX_HEIGHT = 60;
const MENU_MARGIN = 12;

type ContextMenuState = {
  x: number;
  y: number;
} | null;

const MESSAGES = {
  en: {
    collapseCards: "Hide cards",
    expandCards: (n: number) => `Show ${n} session card${n === 1 ? "" : "s"}`,
    activeWindow: "Resolved session",
    anchorMode: "Anchor mode",
    attachToClaude: "Reattach to Claude",
    autoTrackApp: "Auto-track focused app",
    badgeClaude: "Claude",
    badgeCodex: "Codex",
    autoLogin: "Launch automatically at login",
    autoPetMode: "Auto-follow Codex",
    close: "Close",
    currentSession: "Current session",
    detached: "Detached overlay",
    effectivePet: "Effective pet",
    focusApp: "Focus app",
    hidePet: "Hide Pet",
    language: "Language",
    manualPetOverride: "Manual pet override",
    noActiveSession: "No active session",
    noCustomPet: "No custom pet",
    openPetsFolder: "Open ~/.codex/pets",
    openSettings: "Open Settings",
    petScale: "Pet size",
    permissionBody:
      "Without Accessibility access, the pet stays detached and cannot anchor to the active Claude or Codex window.",
    permissionCta: "Open System Settings",
    permissionTitle: "Accessibility permission required",
    petSource: "Pet source",
    petState: "Pet state",
    settingsSubtitle: "Menu bar pet that follows local Claude Desktop and Codex Desktop sessions.",
    startup: "Startup",
    title: "Pet Companion",
    trackClaudeOnly: "Track Claude only",
    trackCodexOnly: "Track Codex only",
    watchClaude: "Watch Claude sessions",
    watchCodex: "Watch Codex sessions",
    watchSection: "Session watch",
    autostartError: "Couldn't update login items. Allow Pet Companion under System Settings → General → Login Items.",
    windowAttached: "Attached to Claude",
  },
  ko: {
    collapseCards: "카드 숨기기",
    expandCards: (n: number) => `${n}개 세션 표시`,
    activeWindow: "현재 세션",
    anchorMode: "앵커 모드",
    attachToClaude: "Claude에 다시 붙이기",
    autoTrackApp: "포커스된 앱 자동 추적",
    badgeClaude: "Claude",
    badgeCodex: "Codex",
    autoLogin: "로그인 시 자동 실행",
    autoPetMode: "Codex 자동 추종",
    close: "닫기",
    currentSession: "현재 상태",
    detached: "분리 오버레이",
    effectivePet: "실제 사용 펫",
    focusApp: "앱 포커스",
    hidePet: "펫 숨기기",
    language: "언어",
    manualPetOverride: "수동 펫 선택",
    noActiveSession: "활성 세션 없음",
    noCustomPet: "선택된 커스텀 펫 없음",
    openPetsFolder: "~/.codex/pets 열기",
    openSettings: "설정 열기",
    petScale: "캐릭터 크기",
    permissionBody:
      "손쉬운 사용 권한이 없으면 펫은 분리 상태로만 동작하고 Claude 또는 Codex 창에 붙어다니지 못합니다.",
    permissionCta: "시스템 설정 열기",
    permissionTitle: "손쉬운 사용 권한 필요",
    petSource: "펫 소스",
    petState: "펫 상태",
    settingsSubtitle: "로컬 Claude Desktop 및 Codex Desktop 세션을 따라다니는 메뉴바 펫입니다.",
    startup: "시작 설정",
    title: "Pet Companion",
    trackClaudeOnly: "Claude만 추적",
    trackCodexOnly: "Codex만 추적",
    watchClaude: "Claude 세션 감시",
    watchCodex: "Codex 세션 감시",
    watchSection: "세션 감시",
    autostartError: "로그인 항목 업데이트에 실패했습니다. 시스템 설정 → 일반 → 로그인 항목에서 Pet Companion을 허용하세요.",
    windowAttached: "Claude 창에 부착",
  },
} as const;

type Messages = (typeof MESSAGES)["en"] | (typeof MESSAGES)["ko"];

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  return invoke<T>(command, args);
}

function clampMenuPosition(x: number, y: number): { x: number; y: number } {
  const maxX = Math.max(MENU_MARGIN, window.innerWidth - MENU_WIDTH - MENU_MARGIN);
  const maxY = Math.max(MENU_MARGIN, window.innerHeight - MENU_MAX_HEIGHT - MENU_MARGIN);

  return {
    x: Math.min(Math.max(MENU_MARGIN, x), maxX),
    y: Math.min(Math.max(MENU_MARGIN, y), maxY),
  };
}

function appBadge(kind: SessionAppKind, strings: Messages): string {
  return kind === "codex" ? strings.badgeCodex : strings.badgeClaude;
}

function stateLabel(state: PetAnimationState, language: "en" | "ko"): string {
  const labels = {
    en: {
      failed: "Failed",
      idle: "Idle",
      jumping: "Updating",
      review: "Review",
      running: "In progress",
      sleeping: "Sleeping",
      waiting: "Still working",
      waving: "Completed",
    },
    ko: {
      failed: "실패",
      idle: "대기",
      jumping: "갱신 중",
      review: "검토",
      running: "진행 중",
      sleeping: "자는 중",
      waiting: "계속 진행 중",
      waving: "완료됨",
    },
  } as const;

  return labels[language][state];
}

// How long the frontend trusts its optimistic patch over an incoming
// `companion:update` whose config still reflects pre-spawn state.  After this
// TTL we accept whatever the backend says, on the assumption that the spawn
// either succeeded slowly or failed and the user has moved on.  Used for
// both pet-override and pet-scale reconciliation.
const PENDING_INTENT_TTL_MS = 5000;
// Pet-scale comparisons use this epsilon — slider step is 0.1 so anything
// within 1e-3 is effectively the same value.  Avoids floating-point near-miss
// mismatches that would defeat the "backend caught up" branch.
const PET_SCALE_EPSILON = 1e-3;

function usePayload() {
  const [payload, setPayload] = useState<AppPayload>(INITIAL_PAYLOAD);
  // v0.1.34: pet change revert flash fix.  Race condition:
  //   1. User picks pet B.  `cmd_set_pet_override` synchronously emits
  //      `companion:pet_override` with B's id → frontend optimistically
  //      patches overlay.pet to B's descriptor.
  //   2. The 750ms refresh tick had already started rebuilding the payload
  //      BEFORE step 1 (or it grabs the model lock immediately after) and
  //      reads stale `model.config.pet_override_id = A` → emits
  //      `companion:update` with overlay.pet = A's descriptor.
  //   3. The spawn from cmd_set_pet_override hasn't yet acquired the lock
  //      to write pet_override_id = B.
  // Visible symptom: pet sprite flashes back to A briefly, then returns to
  // B once the spawn's refresh_and_emit lands.
  //
  // Fix: track the most recent override intent here.  When companion:update
  // arrives whose petOverrideId doesn't match our pending intent (within
  // TTL), patch overlay.pet from the incoming pets list using the intended
  // id so the flash never happens.
  const pendingOverrideRef = useRef<{ petId: string | null; ts: number } | null>(null);
  // v0.1.35: identical race for pet_scale.  User drops slider to 0.5; the
  // 750ms tick mid-rebuild emits a `companion:update` carrying the stale
  // 1.0; spawn hasn't written 0.5 yet → overlay --pet-scale snaps back to
  // 1.0 before the spawn's eventual update lands.  Reconcile here just
  // like the override case.
  const pendingScaleRef = useRef<{ scale: number; ts: number } | null>(null);

  useEffect(() => {
    let mounted = true;
    call<AppPayload>("cmd_get_app_payload")
      .then((next) => {
        if (mounted) {
          setPayload(next);
        }
      })
      .catch(console.error);

    const unlistenUpdate = listen<AppPayload>("companion:update", (event) => {
      const incoming = event.payload;
      let patched = incoming;

      // ── Pet override reconciliation ──
      const pendingOverride = pendingOverrideRef.current;
      if (pendingOverride) {
        const age = Date.now() - pendingOverride.ts;
        if (incoming.config.petOverrideId === pendingOverride.petId) {
          pendingOverrideRef.current = null;
        } else if (age > PENDING_INTENT_TTL_MS) {
          pendingOverrideRef.current = null;
        } else {
          // Stale snapshot — patch overlay.pet from the intended id.
          const lookupId = pendingOverride.petId ?? incoming.codexSelectedPetId;
          const resolved = lookupId
            ? incoming.pets.find((pet) => pet.id === lookupId)
            : undefined;
          if (resolved) {
            patched = {
              ...patched,
              config: { ...patched.config, petOverrideId: pendingOverride.petId },
              overlay: { ...patched.overlay, pet: resolved },
            };
          }
        }
      }

      // ── Pet scale reconciliation ──
      const pendingScale = pendingScaleRef.current;
      if (pendingScale) {
        const age = Date.now() - pendingScale.ts;
        if (Math.abs(incoming.config.petScale - pendingScale.scale) < PET_SCALE_EPSILON) {
          pendingScaleRef.current = null;
        } else if (age > PENDING_INTENT_TTL_MS) {
          pendingScaleRef.current = null;
        } else {
          patched = {
            ...patched,
            config: { ...patched.config, petScale: pendingScale.scale },
          };
        }
      }

      setPayload(patched);
    });

    // v0.1.32: lightweight pet-scale-only event.  The backend emits this
    // synchronously inside `cmd_set_pet_scale` BEFORE acquiring the model
    // lock, so the slider feels instantaneous even when the 750ms refresh
    // tick is mid-rebuild.  We merge into the local payload and let the
    // eventual `companion:update` overwrite if values diverge.
    const unlistenScale = listen<number>("companion:pet_scale", (event) => {
      const nextScale = event.payload;
      pendingScaleRef.current = { scale: nextScale, ts: Date.now() };
      setPayload((prev) =>
        prev.config.petScale === nextScale
          ? prev
          : { ...prev, config: { ...prev.config, petScale: nextScale } },
      );
    });

    // v0.1.33: same pattern as `companion:pet_scale` but for the manual
    // pet override.  Payload is the new override id (string | null).
    // Frontend resolves the descriptor from its local `pets` list and
    // patches `overlay.pet` so the sprite swaps instantly.  If the id
    // can't be resolved (e.g. auto-mode with no codex pet) we still
    // update `config.petOverrideId` and let the eventual
    // `companion:update` fix `overlay.pet`.
    const unlistenOverride = listen<string | null>(
      "companion:pet_override",
      (event) => {
        const nextId = event.payload;
        pendingOverrideRef.current = { petId: nextId, ts: Date.now() };
        setPayload((prev) => {
          const lookupId = nextId ?? prev.codexSelectedPetId;
          const resolved = lookupId
            ? prev.pets.find((pet) => pet.id === lookupId)
            : undefined;
          const nextConfig = { ...prev.config, petOverrideId: nextId };
          if (!resolved) {
            return { ...prev, config: nextConfig };
          }
          return {
            ...prev,
            config: nextConfig,
            overlay: { ...prev.overlay, pet: resolved },
          };
        });
      },
    );

    return () => {
      mounted = false;
      unlistenUpdate.then((fn) => fn()).catch(() => {});
      unlistenScale.then((fn) => fn()).catch(() => {});
      unlistenOverride.then((fn) => fn()).catch(() => {});
    };
  }, []);

  return payload;
}

/**
 * Subscribes to `companion:facing` events from the Rust drag loop.
 * Returns the most recent drag state: `isDragging` true while a drag is
 * active, `facingLeft` true when the cursor is moving left.  When idle
 * the values are `{ isDragging: false, facingLeft: false }`.
 *
 * The hook is intentionally separate from `usePayload` because (a) it
 * has a different lifecycle (only matters during drag), and (b) updating
 * it doesn't need to re-emit the whole payload through React.
 */
function usePetFacing(): { isDragging: boolean; facingLeft: boolean } {
  const [state, setState] = useState({ isDragging: false, facingLeft: false });

  useEffect(() => {
    if (windowLabel !== "overlay") return;
    const unlisten = listen<{ dragging: boolean; facingLeft: boolean }>(
      "companion:facing",
      (event) => {
        const { dragging, facingLeft } = event.payload;
        setState((prev) => {
          if (prev.isDragging === dragging && prev.facingLeft === facingLeft) {
            return prev;
          }
          return { isDragging: dragging, facingLeft };
        });
      },
    );
    return () => {
      unlisten.then((fn) => fn()).catch(() => {});
    };
  }, []);

  return state;
}

type SpriteSheet = { src: string; naturalWidth: number; naturalHeight: number };

function useSpriteSheet(spriteSheetPath: string): SpriteSheet {
  const [sheet, setSheet] = useState<SpriteSheet>({ src: "", naturalWidth: 0, naturalHeight: 0 });

  useEffect(() => {
    let cancelled = false;

    if (!spriteSheetPath) {
      setSheet({ src: "", naturalWidth: 0, naturalHeight: 0 });
      return;
    }

    call<string>("cmd_read_pet_sprite_data_url", { path: spriteSheetPath })
      .then((dataUrl) => {
        if (cancelled) return;
        const img = new Image();
        img.onload = () => {
          if (!cancelled) {
            setSheet({ src: dataUrl, naturalWidth: img.naturalWidth, naturalHeight: img.naturalHeight });
          }
        };
        img.onerror = () => {
          if (!cancelled) {
            console.error("[PetSprite-FATAL] Failed to load spritesheet image from data URL", spriteSheetPath);
            setSheet({ src: "", naturalWidth: 0, naturalHeight: 0 });
          }
        };
        img.src = dataUrl;
      })
      .catch((error) => {
        console.error("[PetSprite-FATAL] cmd_read_pet_sprite_data_url failed for path:", spriteSheetPath, error);
        if (!cancelled) {
          setSheet({ src: "", naturalWidth: 0, naturalHeight: 0 });
        }
      });

    return () => {
      cancelled = true;
    };
  }, [spriteSheetPath]);

  return sheet;
}

function useAnimationFrameCount(state: PetAnimationState) {
  const [frame, setFrame] = useState(0);

  useEffect(() => {
    const spec = STATE_ROWS[state];
    setFrame(0);
    const interval = window.setInterval(() => {
      setFrame((current) => (current + 1) % spec.frames);
    }, Math.max(80, Math.round(1000 / spec.fps)));

    return () => window.clearInterval(interval);
  }, [state]);

  return frame;
}

function PetSprite({
  pet,
  state,
  onLoaded,
  flipHorizontal = false,
}: {
  pet: PetDescriptor;
  state: PetAnimationState;
  onLoaded?: () => void;
  flipHorizontal?: boolean;
}) {
  const frame = useAnimationFrameCount(state);
  const spec = STATE_ROWS[state];
  const sheet = useSpriteSheet(pet.spriteSheetPath);

  if (!pet.spriteSheetPath) {
    console.error(
      "[PetSprite] pet.spriteSheetPath is empty for pet:",
      pet.id,
      pet.displayName,
    );
  }

  const spriteLoaded = sheet.src !== "";

  // Notify parent when sprite becomes available so the shell can gain is-loaded.
  const prevLoadedRef = useRef(false);
  useEffect(() => {
    if (spriteLoaded && !prevLoadedRef.current) {
      prevLoadedRef.current = true;
      onLoaded?.();
    }
    if (!spriteLoaded) {
      prevLoadedRef.current = false;
    }
  }, [spriteLoaded, onLoaded]);

  // Canonical projection: always address the sheet as if it were exactly
  // 8 columns × 9 rows of SPRITE_WIDTH × SPRITE_HEIGHT logical pixels.
  // background-size pins the sheet to those logical dimensions; the browser
  // handles retina sharpness via device pixel ratio automatically.
  const canonicalW = SPRITE_WIDTH * 8;   // 768
  const canonicalH = SPRITE_HEIGHT * 9;  // 936

  if (!spriteLoaded) {
    // Render a small visible placeholder so the user knows where the pet is
    // while the spritesheet data-URL is being fetched.
    return (
      <div
        className="pet-sprite pet-sprite--placeholder"
        style={{ width: `${SPRITE_WIDTH}px`, height: `${SPRITE_HEIGHT}px` }}
        aria-label={`${pet.displayName} loading`}
        role="img"
      >
        🐾
      </div>
    );
  }

  return (
    <div
      className="pet-sprite"
      style={{
        backgroundImage: `url("${sheet.src}")`,
        backgroundSize: `calc(${canonicalW}px * var(--pet-scale, 1)) calc(${canonicalH}px * var(--pet-scale, 1))`,
        backgroundPosition: `calc(${-frame * SPRITE_WIDTH}px * var(--pet-scale, 1)) calc(${-spec.row * SPRITE_HEIGHT}px * var(--pet-scale, 1))`,
        width: `calc(${SPRITE_WIDTH}px * var(--pet-scale, 1))`,
        height: `calc(${SPRITE_HEIGHT}px * var(--pet-scale, 1))`,
        // Horizontal mirror for left-facing during drag.  The sprite sheet
        // only ships a single right-facing running row (row 7), so we flip
        // the entire element with CSS instead of swapping rows.
        transform: flipHorizontal ? "scaleX(-1)" : undefined,
      }}
      aria-label={`${pet.displayName} ${state}`}
      role="img"
    />
  );
}

function normalizeText(text: string): string {
  return text.trim().toLowerCase().replace(/\s+/g, " ");
}

function sessionPreview(
  session: SessionSummary,
  payload: AppPayload,
): string | null {
  let candidate: string | null;

  if (session.sessionId === payload.overlay.activeSession?.sessionId) {
    candidate = payload.overlay.messagePreview;
  } else if (session.inProgress) {
    candidate = session.userPreview ?? null;
  } else {
    candidate = session.assistantPreview ?? session.completedPreview ?? null;
  }

  if (candidate !== null && normalizeText(candidate) === normalizeText(session.title)) {
    return null;
  }

  return candidate;
}

/**
 * Derive a short project label from a session's `cwd`.
 * Uses the trailing path segment (basename) so the user can tell at a glance
 * which project the session was opened against.
 *
 * Examples:
 *   "/Users/carter.p/Dev/claude/works/claude-pet-companion" -> "claude-pet-companion"
 *   "/Users/carter.p" -> "carter.p"
 *   ""                -> null
 */
function projectLabel(cwd: string | null | undefined): string | null {
  if (!cwd) return null;
  const segments = cwd.split("/").filter((part) => part.length > 0);
  if (segments.length === 0) return null;
  return segments[segments.length - 1] ?? null;
}

function OverlayCard({
  onActivate,
  session,
  state,
  preview,
  strings,
  language,
  isActive,
}: {
  onActivate: () => void;
  session: SessionSummary;
  state: PetAnimationState;
  preview: string | null;
  strings: Messages;
  language: "en" | "ko";
  isActive: boolean;
}) {
  const status = stateLabel(state, language);
  const badge = appBadge(session.appKind, strings);
  const project = projectLabel(session.cwd);

  return (
    <button
      className={`overlay-card overlay-card--compact${isActive ? " is-active" : ""}`}
      onClick={onActivate}
      type="button"
    >
      <span className="overlay-card__row">
        <span className={`overlay-card__badge is-${session.appKind}`}>{badge}</span>
        <span className={`overlay-card__status is-${state}`}>
          <span className="overlay-card__status-dot" />
          <span>{status}</span>
        </span>
      </span>
      {project ? (
        <span className="overlay-card__project" title={session.cwd}>
          {project}
        </span>
      ) : null}
      <span className="overlay-card__title" title={session.title}>
        {session.title}
      </span>
      {preview ? (
        <span className="overlay-card__preview" title={preview}>
          {preview}
        </span>
      ) : null}
    </button>
  );
}

function OverlayCardStack({
  payload,
  strings,
  onActivateSession,
}: {
  payload: AppPayload;
  strings: Messages;
  onActivateSession: (
    sessionId: string,
    appKind: SessionAppKind,
    visualState: PetAnimationState,
  ) => void;
}) {
  const visible = pickVisibleSessions(payload);
  if (visible.length === 0) return null;
  const activeId = payload.overlay.activeSession?.sessionId ?? null;
  const stackClass = payload.overlay.cardsBelow
    ? "overlay-card-stack overlay-card-stack--below"
    : "overlay-card-stack";

  return (
    <div className={stackClass}>
      {visible.map((session) => {
        const visualState = sessionVisualState(session, payload);
        return (
          <OverlayCard
            key={session.sessionId}
            isActive={session.sessionId === activeId}
            language={payload.config.language}
            onActivate={() =>
              onActivateSession(session.sessionId, session.appKind, visualState)
            }
            preview={sessionPreview(session, payload)}
            session={session}
            state={visualState}
            strings={strings}
          />
        );
      })}
    </div>
  );
}

function ContextMenu({
  menu,
  onClose,
  onHidePet,
  onOpenSettings,
  strings,
}: {
  menu: ContextMenuState;
  onClose: () => void;
  onHidePet: () => void;
  onOpenSettings: () => void;
  strings: Messages;
}) {
  if (!menu) {
    return null;
  }

  return (
    <>
      <div className="menu-backdrop" onClick={onClose} />
      <div className="context-menu" style={{ left: menu.x, top: menu.y }}>
        <button className="context-menu__item" onClick={onHidePet} type="button">
          {strings.hidePet}
        </button>
        <button className="context-menu__item" onClick={onOpenSettings} type="button">
          {strings.openSettings}
        </button>
      </div>
    </>
  );
}

/**
 * Returns true when the element at (x, y) belongs to an interactive region.
 * We use closest() so any child of the hit containers also matches correctly.
 */
function isInteractivePoint(x: number, y: number): boolean {
  const el = document.elementFromPoint(x, y);
  if (!el) return false;
  return Boolean(
    el.closest(".pet-shell, .overlay-card-stack, .context-menu, .menu-backdrop"),
  );
}

/**
 * Dynamic hit-test hook for the overlay window.
 *
 * Strategy (option C from spec):
 *   • When ignore=false (normal): native mousemove fires → we check hit and
 *     call setIgnoreCursorEvents(true) when cursor is over a transparent gap.
 *   • When ignore=true (pass-through): OS swallows mousemove, so we poll
 *     Rust cmd_cursor_position_in_overlay every 50 ms and re-enable hit-test
 *     when cursor re-enters an interactive element.
 *   • While context menu is open we force ignore=false unconditionally so
 *     backdrop and menu items always receive clicks.
 *
 * Coordinate guarantee:
 *   cmd_cursor_position_in_overlay converts:
 *     physical_cursor − physical_window_origin) / scale_factor  →  logical px
 *   This matches clientX/clientY used by elementFromPoint().
 */
function useOverlayHitTest(menuOpen: boolean, draggingRef: React.MutableRefObject<boolean>): void {
  useEffect(() => {
    if (windowLabel !== "overlay") return;

    const win = getCurrentWebviewWindow();
    let ignoring = false;

    const applyIgnore = (next: boolean): void => {
      if (next === ignoring) return;
      ignoring = next;
      win.setIgnoreCursorEvents(next).catch(() => {});
    };

    // Start fully transparent so the overlay never blocks anything at init.
    applyIgnore(true);

    // --- native mousemove path (ignore=false) ---
    const onMove = (e: MouseEvent): void => {
      if (menuOpen || draggingRef.current) {
        // During an active drag, IPC latency causes the window to lag a few
        // ms behind the cursor.  In those frames the cursor's window-local
        // clientY can fall outside the pet shell (or even go negative), at
        // which point isInteractivePoint() returns false and we would flip
        // to pass-through — losing pointer capture mid-drag and freezing the
        // pet.  Force interactive while dragging.
        applyIgnore(false);
        return;
      }
      applyIgnore(!isInteractivePoint(e.clientX, e.clientY));
    };

    window.addEventListener("mousemove", onMove, { passive: true });

    // --- polling path (ignore=true) ---
    // When pass-through is active the OS never delivers mousemove, so we
    // periodically ask Rust for the current cursor position in window-local
    // logical coordinates and re-enable hit-test if the cursor is back over
    // an interactive element.
    const pollId = window.setInterval(async () => {
      if (!ignoring) return; // native mousemove path handles it
      if (menuOpen || draggingRef.current) {
        applyIgnore(false);
        return;
      }
      try {
        const pos = await call<[number, number] | null>(
          "cmd_cursor_position_in_overlay",
        );
        if (pos) {
          const [x, y] = pos;
          if (isInteractivePoint(x, y)) {
            applyIgnore(false);
            // Native mousemove takes over from here once ignore=false.
          }
        }
      } catch {
        // Non-fatal: next tick will retry.
      }
    }, 50);

    return () => {
      window.removeEventListener("mousemove", onMove);
      window.clearInterval(pollId);
      applyIgnore(false);
    };
  }, [menuOpen, draggingRef]);
}

function OverlayApp() {
  const payload = usePayload();
  // v0.1.34: drag-direction animation.  Rust drag loop emits `companion:facing`
  // when the cursor crosses a hysteresis threshold; we use the state here to
  // (a) override the pet's animation to "running" while dragging and (b)
  // mirror the sprite horizontally via CSS when the cursor is moving left.
  const facing = usePetFacing();
  const [menu, setMenu] = useState<ContextMenuState>(null);
  const [spriteLoaded, setSpriteLoaded] = useState(false);
  const [collapsed, setCollapsed] = useState(false);
  // Drag state.  WKWebView's `event.screenY` / `event.clientY` flip their
  // origin between monitors (verified v0.1.26 diagnostics: screenY oscillated
  // ±1080 across the upper-monitor boundary), so the frontend cannot compute
  // a reliable target window position.  Instead the frontend captures only
  // the cursor offset within the window on pointerdown, then asks Rust to
  // take over via `cmd_begin_drag`: Rust polls `window.cursor_position()`
  // (single global coord space) every 16 ms and writes the new logical
  // position itself.  pointermove sends no per-frame IPC; pointerup calls
  // `cmd_finalize_drag_position` which stops the polling loop.
  const dragStateRef = useRef<{
    // Cursor offset within the overlay window at pointerdown (in logical
    // CSS pixels).  Frozen for the duration of the drag and passed once to
    // Rust via `cmd_begin_drag` so the pet stays anchored under the cursor.
    grabOffsetX: number;
    grabOffsetY: number;
    // Initial cursor screen position (for click-vs-drag threshold only —
    // not used for positioning).
    startScreenX: number;
    startScreenY: number;
    started: boolean;
  } | null>(null);
  // Mirror of dragStateRef.current?.started, read by useOverlayHitTest so it
  // can suppress pass-through transitions while a drag is active.
  const draggingRef = useRef<boolean>(false);
  const strings = MESSAGES[payload.config.language];

  // Dynamic hit-test: transparent areas pass clicks through; interactive
  // elements (.pet-shell, cards, context menu) receive events normally.
  // When the context menu is open we force ignore=false so backdrop/items work.
  useOverlayHitTest(menu !== null, draggingRef);

  // Reset loaded state when the pet changes so the placeholder shows briefly.
  const prevPetIdRef = useRef(payload.overlay.pet.id);
  useEffect(() => {
    if (payload.overlay.pet.id !== prevPetIdRef.current) {
      prevPetIdRef.current = payload.overlay.pet.id;
      setSpriteLoaded(false);
    }
  }, [payload.overlay.pet.id]);

  const handleActivateSession = async (
    sessionId?: string,
    appKind?: SessionAppKind,
    visualState?: PetAnimationState,
  ) => {
    try {
      if (sessionId && appKind) {
        // Dismiss-on-click policy (v0.1.30): decided by VISUAL state, not by
        // the backend's in_progress flag.  See `dismissDecisionForVisualState`
        // for the full rationale and edge cases (Claude B-T1-R1 quirk, etc.).
        const dismiss = dismissDecisionForVisualState(visualState ?? "idle");
        await call("cmd_focus_session_by_id", {
          input: { sessionId, appKind, dismiss },
        });
      } else {
        await call("cmd_focus_active_session");
      }
    } catch (error) {
      console.error(error);
    }
  };

  const DRAG_THRESHOLD = 6;

  const handlePointerDown = (event: React.PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) {
      return;
    }
    event.currentTarget.setPointerCapture(event.pointerId);
    dragStateRef.current = {
      grabOffsetX: event.clientX,
      grabOffsetY: event.clientY,
      startScreenX: event.screenX,
      startScreenY: event.screenY,
      started: false,
    };
  };

  const handlePointerMove = (event: React.PointerEvent<HTMLDivElement>) => {
    const state = dragStateRef.current;
    if (!state) {
      return;
    }
    if (!state.started) {
      const dx = event.screenX - state.startScreenX;
      const dy = event.screenY - state.startScreenY;
      if (Math.hypot(dx, dy) < DRAG_THRESHOLD) {
        return;
      }
      state.started = true;
      draggingRef.current = true;
      // Hand the drag off to Rust: it polls `window.cursor_position()`
      // (single global coord space) at 16 ms intervals and writes the new
      // window position itself, bypassing WKWebView's per-monitor screenY
      // origin flip that caused the v0.1.25 oscillation.  Also flips
      // `detached = true` synchronously so the 750 ms sync tick stops
      // re-anchoring to the tracked Claude/Codex window.
      call("cmd_begin_drag", {
        input: {
          grabOffsetX: state.grabOffsetX,
          grabOffsetY: state.grabOffsetY,
        },
      }).catch((err) => console.error("cmd_begin_drag failed:", err));
    }
    // Per-frame moves are intentionally NOT forwarded to Rust — the Rust
    // cursor-polling loop handles position updates itself.
  };

  const handlePointerUp = async (event: React.PointerEvent<HTMLDivElement>) => {
    const state = dragStateRef.current;
    if (!state) {
      return;
    }
    try {
      event.currentTarget.releasePointerCapture(event.pointerId);
    } catch {
      /* ignore — capture may have been lost */
    }
    const wasDrag = state.started;
    dragStateRef.current = null;
    draggingRef.current = false;

    if (!wasDrag) {
      try {
        await call("cmd_pet_reaction");
      } catch (error) {
        console.error(error);
      }
    } else {
      // Stops the Rust cursor-polling loop, snapshots the final position
      // (from `window.outer_position()`), persists it, and refreshes the
      // payload so the frontend recomputes `cardsBelow` for the new pet
      // location.
      try {
        await call("cmd_finalize_drag_position");
      } catch (error) {
        console.error("cmd_finalize_drag_position failed:", error);
      }
    }
  };

  const visibleCardCount = pickVisibleSessions(payload).length;

  return (
    <div
      className="overlay-root"
      style={{ "--pet-scale": String(payload.config.petScale) } as React.CSSProperties}
      onContextMenu={(event) => {
        event.preventDefault();
        setMenu(clampMenuPosition(event.clientX, event.clientY));
      }}
    >
      {!collapsed ? (
        <OverlayCardStack
          onActivateSession={handleActivateSession}
          payload={payload}
          strings={strings}
        />
      ) : null}
      <div
        className={`pet-shell is-${facing.isDragging ? "running" : payload.overlay.effectiveState}${spriteLoaded ? " is-loaded" : ""}`}
        onDoubleClick={() => call("cmd_reattach_overlay").catch(console.error)}
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={handlePointerUp}
      >
        <PetSprite
          flipHorizontal={facing.isDragging && facing.facingLeft}
          onLoaded={() => setSpriteLoaded(true)}
          pet={payload.overlay.pet}
          state={facing.isDragging ? "running" : payload.overlay.effectiveState}
        />
        <button
          className="pet-shell__collapse-btn"
          onPointerDown={(e) => e.stopPropagation()}
          onClick={(e) => {
            e.stopPropagation();
            setCollapsed((prev) => !prev);
          }}
          title={
            collapsed
              ? strings.expandCards(visibleCardCount)
              : strings.collapseCards
          }
          type="button"
        >
          {collapsed ? String(visibleCardCount) : "▾"}
        </button>
      </div>
      <ContextMenu
        menu={menu}
        onClose={() => setMenu(null)}
        onHidePet={() => {
          setMenu(null);
          call("cmd_set_pet_hidden", { input: { hidden: true } }).catch(console.error);
        }}
        onOpenSettings={() => {
          setMenu(null);
          call("cmd_show_settings").catch(console.error);
        }}
        strings={strings}
      />
    </div>
  );
}

function SettingsSection({
  children,
  title,
}: {
  children: React.ReactNode;
  title: string;
}) {
  return (
    <section className="settings-section">
      <h2>{title}</h2>
      {children}
    </section>
  );
}

function SettingsApp() {
  const payload = usePayload();
  const [autostartEnabled, setAutostartEnabled] = useState(false);
  const [autostartError, setAutostartError] = useState<string | null>(null);
  // Slider drag-vs-payload-echo: the IPC round-trip for cmd_set_pet_scale
  // (lock + persist + emit + React rerender) is slow enough that a 60Hz drag
  // sees the controlled `value` lagging by a frame or two, which makes the
  // thumb feel sticky.  We track a local value that updates synchronously on
  // every change event and resync from `payload.config.petScale` whenever
  // the backend echoes a different scale (initial load, programmatic set).
  const [localScale, setLocalScale] = useState(payload.config.petScale);
  // Watch-toggle drag-vs-payload-echo: identical reasoning to localScale above.
  // `cmd_set_watch_apps` triggers a full `refresh_and_emit` which is even
  // slower than `cmd_set_pet_scale` (whole-payload rebuild including
  // session scan).  Without a local mirror, clicking the checkbox shows the
  // toggle revert visually until the backend echo arrives, which the user
  // perceives as "the checkbox doesn't work."
  const [localWatchClaude, setLocalWatchClaude] = useState(payload.config.watchClaude);
  const [localWatchCodex, setLocalWatchCodex] = useState(payload.config.watchCodex);
  // Single ref tracking the latest desired pair.  Each toggle handler updates
  // this ref synchronously before firing the IPC, so the call sees the
  // freshest values even if React has not yet rerendered after the previous
  // setState. Without this, two rapid clicks (Claude then Codex) could ship
  // the IPC with a stale Claude value because the Codex closure was captured
  // from the pre-Claude-click render.
  const watchStateRef = useRef({
    claude: payload.config.watchClaude,
    codex: payload.config.watchCodex,
  });
  const strings = MESSAGES[payload.config.language];

  useEffect(() => {
    isEnabled().then(setAutostartEnabled).catch(() => setAutostartEnabled(false));
  }, []);

  useEffect(() => {
    setLocalScale(payload.config.petScale);
  }, [payload.config.petScale]);

  useEffect(() => {
    setLocalWatchClaude(payload.config.watchClaude);
    watchStateRef.current.claude = payload.config.watchClaude;
  }, [payload.config.watchClaude]);

  useEffect(() => {
    setLocalWatchCodex(payload.config.watchCodex);
    watchStateRef.current.codex = payload.config.watchCodex;
  }, [payload.config.watchCodex]);

  const handleAutostartChange = async (checked: boolean) => {
    try {
      if (checked) {
        await enable();
      } else {
        await disable();
      }
      setAutostartEnabled(checked);
      setAutostartError(null);
    } catch (error) {
      console.error(error);
      setAutostartError(strings.autostartError);
    }
  };

  const handleWatchToggle = (key: "claude" | "codex", next: boolean) => {
    // Update ref synchronously so the IPC payload always reflects the latest
    // user intent even if two clicks land in the same React batch.
    watchStateRef.current = { ...watchStateRef.current, [key]: next };
    if (key === "claude") setLocalWatchClaude(next);
    else setLocalWatchCodex(next);
    const { claude, codex } = watchStateRef.current;
    call("cmd_set_watch_apps", {
      input: { watchClaude: claude, watchCodex: codex },
    }).catch(console.error);
  };

  return (
    <div className="settings-root" style={{ pointerEvents: "auto", userSelect: "text" }}>
      <header className="settings-header">
        <div>
          <h1>{strings.title}</h1>
          <p>{strings.settingsSubtitle}</p>
        </div>
        <button
          className="secondary-button"
          onClick={() => getCurrentWebviewWindow().hide().catch(console.error)}
          type="button"
        >
          {strings.close}
        </button>
      </header>

      {!payload.overlay.permissionGranted ? (
        <div className="permission-banner">
          <div>
            <strong>{strings.permissionTitle}</strong>
            <p>{strings.permissionBody}</p>
          </div>
          <button
            className="primary-button"
            onClick={() => call("cmd_open_accessibility_settings").catch(console.error)}
            type="button"
          >
            {strings.permissionCta}
          </button>
        </div>
      ) : null}

      <SettingsSection title={strings.currentSession}>
        <div className="kv-grid">
          <div className="kv">
            <span className="kv__label">{strings.activeWindow}</span>
            <span className="kv__value">
              {payload.overlay.activeSession?.title ?? strings.noActiveSession}
            </span>
          </div>
          <div className="kv">
            <span className="kv__label">{strings.petState}</span>
            <span className="kv__value">
              {stateLabel(payload.overlay.effectiveState, payload.config.language)}
            </span>
          </div>
          <div className="kv">
            <span className="kv__label">{strings.anchorMode}</span>
            <span className="kv__value">
              {payload.config.attached && payload.overlay.permissionGranted
                ? strings.windowAttached
                : strings.detached}
            </span>
          </div>
        </div>
      </SettingsSection>

      <SettingsSection title={strings.petSource}>
        <label className="stacked-field">
          <span>{strings.language}</span>
          <select
            onChange={(event) => {
              call("cmd_set_language", { input: { language: event.target.value } }).catch(
                console.error,
              );
            }}
            value={payload.config.language}
          >
            <option value="ko">한국어</option>
            <option value="en">English</option>
          </select>
        </label>
        <div className="kv-grid compact">
          <div className="kv">
            <span className="kv__label">{strings.effectivePet}</span>
            <span className="kv__value">{payload.overlay.pet.displayName}</span>
          </div>
        </div>
        <label className="stacked-field">
          <span>{strings.manualPetOverride}</span>
          <select
            onChange={(event) => {
              call("cmd_set_pet_override", {
                input: { petId: event.target.value || null },
              }).catch(console.error);
            }}
            value={payload.config.petOverrideId ?? ""}
          >
            <option value="">{strings.autoPetMode}</option>
            {payload.pets.map((pet) => (
              <option key={pet.id} value={pet.id}>
                {pet.displayName}
              </option>
            ))}
          </select>
        </label>
        <label className="stacked-field">
          <span>{strings.petScale} ({Math.round(localScale * 100)}%)</span>
          <input
            type="range"
            min="0.5"
            max="2.0"
            step="0.1"
            value={localScale}
            onChange={(e) => {
              const next = Number(e.target.value);
              setLocalScale(next);
              call("cmd_set_pet_scale", { input: { scale: next } }).catch(console.error);
            }}
          />
        </label>
        <button
          className="secondary-button"
          onClick={() => call("cmd_open_pets_folder").catch(console.error)}
          type="button"
        >
          {strings.openPetsFolder}
        </button>
      </SettingsSection>

      <SettingsSection title={strings.watchSection}>
        <label className="toggle-row">
          <input
            checked={localWatchClaude}
            onChange={(event) => handleWatchToggle("claude", event.target.checked)}
            type="checkbox"
          />
          <span>{strings.watchClaude}</span>
        </label>
        <label className="toggle-row">
          <input
            checked={localWatchCodex}
            onChange={(event) => handleWatchToggle("codex", event.target.checked)}
            type="checkbox"
          />
          <span>{strings.watchCodex}</span>
        </label>
      </SettingsSection>

      <SettingsSection title={strings.startup}>
        <label className="toggle-row">
          <input
            checked={autostartEnabled}
            onChange={(event) => handleAutostartChange(event.target.checked)}
            type="checkbox"
          />
          <span>{strings.autoLogin}</span>
        </label>
        {autostartError ? <p className="notice">{autostartError}</p> : null}
      </SettingsSection>
    </div>
  );
}

export default function App() {
  return windowLabel === "settings" ? <SettingsApp /> : <OverlayApp />;
}
