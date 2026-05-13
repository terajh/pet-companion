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
import "./App.css";

const windowLabel = getCurrentWebviewWindow().label;

const INITIAL_PAYLOAD: AppPayload = {
  config: {
    attached: true,
    language: "ko",
    trackedApp: "auto",
    manualSessionApp: null,
    manualSessionId: null,
    petOverrideId: null,
  },
  codexSelectedPetId: null,
  overlay: {
    activeSession: null,
    claudeFrontmost: false,
    codexFrontmost: false,
    currentWindowTitle: null,
    effectiveState: "idle",
    messagePreview: null,
    manualSessionMissing: false,
    manualSessionPinned: false,
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
  },
  pets: [],
};

const STATE_ROWS: Record<
  PetAnimationState,
  { frames: number; fps: number; row: number }
> = {
  idle: { row: 0, frames: 6, fps: 3 },
  running: { row: 7, frames: 6, fps: 7 },
  waiting: { row: 6, frames: 6, fps: 2 },
  waving: { row: 3, frames: 4, fps: 6 },
  jumping: { row: 4, frames: 5, fps: 8 },
  review: { row: 8, frames: 6, fps: 3 },
  failed: { row: 5, frames: 8, fps: 4 },
};

const SPRITE_WIDTH = 96;
const SPRITE_HEIGHT = 104;
const MENU_WIDTH = 248;
const MENU_MAX_HEIGHT = 280;
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
    autoFollow: "Auto-follow active session",
    autoLogin: "Launch automatically at login",
    autoPetFollow: "Follow Codex selected custom pet automatically",
    autoPetMode: "Auto-follow Codex",
    close: "Close",
    codexSelectedPet: "Codex selected pet",
    currentSession: "Current session",
    detached: "Detached overlay",
    effectivePet: "Effective pet",
    fallbackMissingPin:
      "Pinned session was missing, so companion fell back to auto-follow.",
    focusApp: "Focus app",
    language: "Language",
    manualPetOverride: "Manual pet override",
    manualSessionOverride: "Manual session override",
    noActiveSession: "No active session",
    noCustomPet: "No custom pet",
    openPetsFolder: "Open ~/.codex/pets",
    openSettings: "Open Settings",
    permissionBody:
      "Without Accessibility access, the pet stays detached and cannot anchor to the active Claude or Codex window.",
    permissionCta: "Open System Settings",
    permissionTitle: "Accessibility permission required",
    petSource: "Pet source",
    petState: "Pet state",
    pinSection: "Session Pin",
    settingsSubtitle: "Menu bar pet that follows local Claude Desktop and Codex Desktop sessions.",
    startup: "Startup",
    title: "Pet Companion",
    trackClaudeOnly: "Track Claude only",
    trackCodexOnly: "Track Codex only",
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
    autoFollow: "활성 세션 자동 추적",
    autoLogin: "로그인 시 자동 실행",
    autoPetFollow: "Codex에서 선택한 커스텀 펫 자동 추종",
    autoPetMode: "Codex 자동 추종",
    close: "닫기",
    codexSelectedPet: "Codex 선택 펫",
    currentSession: "현재 상태",
    detached: "분리 오버레이",
    effectivePet: "실제 사용 펫",
    fallbackMissingPin:
      "고정한 세션을 찾지 못해서 자동 추적으로 되돌렸습니다.",
    focusApp: "앱 포커스",
    language: "언어",
    manualPetOverride: "수동 펫 선택",
    manualSessionOverride: "수동 세션 고정",
    noActiveSession: "활성 세션 없음",
    noCustomPet: "선택된 커스텀 펫 없음",
    openPetsFolder: "~/.codex/pets 열기",
    openSettings: "설정 열기",
    permissionBody:
      "손쉬운 사용 권한이 없으면 펫은 분리 상태로만 동작하고 Claude 또는 Codex 창에 붙어다니지 못합니다.",
    permissionCta: "시스템 설정 열기",
    permissionTitle: "손쉬운 사용 권한 필요",
    petSource: "펫 소스",
    petState: "펫 상태",
    pinSection: "세션 고정",
    settingsSubtitle: "로컬 Claude Desktop 및 Codex Desktop 세션을 따라다니는 메뉴바 펫입니다.",
    startup: "시작 설정",
    title: "Pet Companion",
    trackClaudeOnly: "Claude만 추적",
    trackCodexOnly: "Codex만 추적",
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
      waiting: "Still working",
      waving: "Completed",
    },
    ko: {
      failed: "실패",
      idle: "대기",
      jumping: "갱신 중",
      review: "검토",
      running: "진행 중",
      waiting: "계속 진행 중",
      waving: "완료됨",
    },
  } as const;

  return labels[language][state];
}

function usePayload() {
  const [payload, setPayload] = useState<AppPayload>(INITIAL_PAYLOAD);

  useEffect(() => {
    let mounted = true;
    call<AppPayload>("cmd_get_app_payload")
      .then((next) => {
        if (mounted) {
          setPayload(next);
        }
      })
      .catch(console.error);

    const unlistenPromise = listen<AppPayload>("companion:update", (event) => {
      setPayload(event.payload);
    });

    return () => {
      mounted = false;
      unlistenPromise.then((fn) => fn()).catch(() => {});
    };
  }, []);

  return payload;
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
}: {
  pet: PetDescriptor;
  state: PetAnimationState;
  onLoaded?: () => void;
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
        backgroundSize: `${canonicalW}px ${canonicalH}px`,
        backgroundPosition: `-${frame * SPRITE_WIDTH}px -${spec.row * SPRITE_HEIGHT}px`,
        width: `${SPRITE_WIDTH}px`,
        height: `${SPRITE_HEIGHT}px`,
      }}
      aria-label={`${pet.displayName} ${state}`}
      role="img"
    />
  );
}

const MAX_VISIBLE_CARDS = 6;

function pickVisibleSessions(payload: AppPayload): SessionSummary[] {
  const dismissed = new Set(payload.overlay.dismissedSessionIds);

  // A session gets its own card when:
  //   • it is in_progress (always show, even if previously dismissed), OR
  //   • it is non-archived and non-dismissed (matches CLAUDE.md policy:
  //     "완료/유휴 상태는 기본적으로 카드 표시").
  const candidates = payload.overlay.sessions.filter((session) => {
    if (session.isArchived) return false;
    if (session.inProgress) return true;
    if (dismissed.has(session.sessionId)) return false;
    return true;
  });

  // The candidate filter already excludes dismissed non-in_progress sessions,
  // so this pass is a no-op but kept for safety/readability.
  const filtered = candidates.filter(
    (session) => session.inProgress || !dismissed.has(session.sessionId),
  );

  // Sort: in-progress first, then by recency.
  const sorted = [...filtered].sort((a, b) => {
    if (a.inProgress !== b.inProgress) return a.inProgress ? -1 : 1;
    return b.lastActivityAt - a.lastActivityAt;
  });

  // Deduplicate by sessionId in case the same id slipped through.
  const seen = new Set<string>();
  const unique: SessionSummary[] = [];
  for (const session of sorted) {
    if (seen.has(session.sessionId)) continue;
    seen.add(session.sessionId);
    unique.push(session);
    if (unique.length >= MAX_VISIBLE_CARDS) break;
  }
  return unique;
}

function sessionVisualState(
  session: SessionSummary,
  payload: AppPayload,
): PetAnimationState {
  // For the currently-active session use the backend-computed effective state.
  if (session.sessionId === payload.overlay.activeSession?.sessionId) {
    return payload.overlay.effectiveState;
  }
  if (session.inProgress) {
    const ageMs = Date.now() - session.lastActivityAt;
    return ageMs > 30_000 ? "waiting" : "running";
  }
  return "idle";
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
  onActivateSession: (sessionId: string) => void;
}) {
  const visible = pickVisibleSessions(payload);
  if (visible.length === 0) return null;
  const activeId = payload.overlay.activeSession?.sessionId ?? null;

  return (
    <div className="overlay-card-stack">
      {visible.map((session) => (
        <OverlayCard
          key={session.sessionId}
          isActive={session.sessionId === activeId}
          language={payload.config.language}
          onActivate={() => onActivateSession(session.sessionId)}
          preview={sessionPreview(session, payload)}
          session={session}
          state={sessionVisualState(session, payload)}
          strings={strings}
        />
      ))}
    </div>
  );
}

function ContextMenu({
  menu,
  onClose,
  onFocusSession,
  onOpenSettings,
  onReattach,
  onSelectTrackedApp,
  onSelectSession,
  payload,
  strings,
}: {
  menu: ContextMenuState;
  onClose: () => void;
  onFocusSession: () => void;
  onOpenSettings: () => void;
  onReattach: () => void;
  onSelectTrackedApp: (app: "auto" | SessionAppKind) => void;
  onSelectSession: (app: SessionAppKind | null, sessionId: string | null) => void;
  payload: AppPayload;
  strings: Messages;
}) {
  if (!menu) {
    return null;
  }

  const pinnedSessionId = payload.config.manualSessionId;
  const pinnedSessionApp = payload.config.manualSessionApp;
  const claudeSessions = payload.overlay.sessions.filter((session) => session.appKind === "claude");
  const codexSessions = payload.overlay.sessions.filter((session) => session.appKind === "codex");

  return (
    <>
      <div className="menu-backdrop" onClick={onClose} />
      <div className="context-menu" style={{ left: menu.x, top: menu.y }}>
        <button className="context-menu__item" onClick={onFocusSession} type="button">
          {payload.overlay.activeSession
            ? `${appBadge(payload.overlay.activeSession.appKind, strings)} ${strings.focusApp}`
            : strings.focusApp}
        </button>
        <button className="context-menu__item" onClick={onReattach} type="button">
          {strings.attachToClaude}
        </button>
        <button className="context-menu__item" onClick={onOpenSettings} type="button">
          {strings.openSettings}
        </button>
        <div className="context-menu__separator" />
        <div className="context-menu__section-label">{strings.pinSection}</div>
        <button
          className={`context-menu__item ${
            pinnedSessionId === null && payload.config.trackedApp === "auto" ? "is-selected" : ""
          }`}
          onClick={() => {
            onSelectSession(null, null);
            onSelectTrackedApp("auto");
          }}
          type="button"
        >
          {strings.autoTrackApp}
        </button>
        <button
          className={`context-menu__item ${
            pinnedSessionId === null && payload.config.trackedApp === "claude" ? "is-selected" : ""
          }`}
          onClick={() => {
            onSelectSession(null, null);
            onSelectTrackedApp("claude");
          }}
          type="button"
        >
          {strings.trackClaudeOnly}
        </button>
        <button
          className={`context-menu__item ${
            pinnedSessionId === null && payload.config.trackedApp === "codex" ? "is-selected" : ""
          }`}
          onClick={() => {
            onSelectSession(null, null);
            onSelectTrackedApp("codex");
          }}
          type="button"
        >
          {strings.trackCodexOnly}
        </button>
        {claudeSessions.length > 0 ? (
          <>
            <div className="context-menu__separator" />
            <div className="context-menu__section-label">{strings.badgeClaude}</div>
          </>
        ) : null}
        {claudeSessions.slice(0, 6).map((session) => (
          <button
            className={`context-menu__item ${
              pinnedSessionId === session.sessionId && pinnedSessionApp === "claude"
                ? "is-selected"
                : ""
            }`}
            key={`claude-${session.sessionId}`}
            onClick={() => onSelectSession("claude", session.sessionId)}
            type="button"
            title={session.title}
          >
            {session.title}
          </button>
        ))}
        {codexSessions.length > 0 ? (
          <>
            <div className="context-menu__separator" />
            <div className="context-menu__section-label">{strings.badgeCodex}</div>
          </>
        ) : null}
        {codexSessions.slice(0, 6).map((session) => (
          <button
            className={`context-menu__item ${
              pinnedSessionId === session.sessionId && pinnedSessionApp === "codex"
                ? "is-selected"
                : ""
            }`}
            key={`codex-${session.sessionId}`}
            onClick={() => onSelectSession("codex", session.sessionId)}
            type="button"
            title={session.title}
          >
            {session.title}
          </button>
        ))}
      </div>
    </>
  );
}

function OverlayApp() {
  const payload = usePayload();
  const [menu, setMenu] = useState<ContextMenuState>(null);
  const [spriteLoaded, setSpriteLoaded] = useState(false);
  const [collapsed, setCollapsed] = useState(false);
  const pointerStartRef = useRef<{ x: number; y: number } | null>(null);
  const dragStartedRef = useRef(false);
  const strings = MESSAGES[payload.config.language];

  // Reset loaded state when the pet changes so the placeholder shows briefly.
  const prevPetIdRef = useRef(payload.overlay.pet.id);
  useEffect(() => {
    if (payload.overlay.pet.id !== prevPetIdRef.current) {
      prevPetIdRef.current = payload.overlay.pet.id;
      setSpriteLoaded(false);
    }
  }, [payload.overlay.pet.id]);

  const handleActivateSession = async (sessionId?: string) => {
    try {
      if (sessionId) {
        await call("cmd_focus_session_by_id", { sessionId });
      } else {
        await call("cmd_focus_active_session");
      }
    } catch (error) {
      console.error(error);
    }
  };

  // Drag is handled natively via `data-tauri-drag-region` on the pet shell —
  // calling `start_dragging` from a pointermove handler doesn't work because
  // by the time the IPC round-trips back to Rust the originating mousedown
  // event is no longer in the macOS event queue.  We still detect a "real"
  // drag vs a simple click in JS so we only fire the pet reaction on click.
  const handlePointerDown = (event: React.PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) {
      return;
    }
    pointerStartRef.current = { x: event.clientX, y: event.clientY };
    dragStartedRef.current = false;
  };

  const handlePointerMove = (event: React.PointerEvent<HTMLDivElement>) => {
    if (!pointerStartRef.current || dragStartedRef.current) {
      return;
    }
    const dx = event.clientX - pointerStartRef.current.x;
    const dy = event.clientY - pointerStartRef.current.y;
    if (Math.hypot(dx, dy) >= 6) {
      dragStartedRef.current = true;
    }
  };

  const handlePointerUp = async () => {
    if (!pointerStartRef.current) {
      return;
    }
    const wasDrag = dragStartedRef.current;
    pointerStartRef.current = null;
    dragStartedRef.current = false;

    if (!wasDrag) {
      try {
        await call("cmd_pet_reaction");
      } catch (error) {
        console.error(error);
      }
    } else {
      // The window moved; mark overlay as detached on the backend side.
      try {
        await call("cmd_mark_detached");
      } catch (error) {
        console.error(error);
      }
    }
  };

  const visibleCardCount = pickVisibleSessions(payload).length;

  return (
    <div
      className="overlay-root"
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
        className={`pet-shell is-${payload.overlay.effectiveState}${spriteLoaded ? " is-loaded" : ""}`}
        data-tauri-drag-region
        onDoubleClick={() => call("cmd_reattach_overlay").catch(console.error)}
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={handlePointerUp}
      >
        <PetSprite
          onLoaded={() => setSpriteLoaded(true)}
          pet={payload.overlay.pet}
          state={payload.overlay.effectiveState}
        />
        <button
          className="pet-shell__collapse-btn"
          data-tauri-drag-region="false"
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
        onFocusSession={() => {
          setMenu(null);
          handleActivateSession().catch(console.error);
        }}
        onOpenSettings={() => {
          setMenu(null);
          call("cmd_show_settings").catch(console.error);
        }}
        onReattach={() => {
          setMenu(null);
          call("cmd_reattach_overlay").catch(console.error);
        }}
        onSelectTrackedApp={(app) => {
          setMenu(null);
          call("cmd_set_tracked_app", { app }).catch(console.error);
        }}
        onSelectSession={(app, sessionId) => {
          setMenu(null);
          call("cmd_set_manual_session", { appKind: app, sessionId }).catch(console.error);
        }}
        payload={payload}
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
  const strings = MESSAGES[payload.config.language];

  useEffect(() => {
    isEnabled().then(setAutostartEnabled).catch(() => setAutostartEnabled(false));
  }, []);

  const pinnedSessionId = payload.config.manualSessionId;

  const handleAutostartChange = async (checked: boolean) => {
    try {
      if (checked) {
        await enable();
      } else {
        await disable();
      }
      setAutostartEnabled(checked);
    } catch (error) {
      console.error(error);
    }
  };

  return (
    <div className="settings-root">
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
            onChange={(event) =>
              call("cmd_set_language", { language: event.target.value }).catch(console.error)
            }
            value={payload.config.language}
          >
            <option value="ko">한국어</option>
            <option value="en">English</option>
          </select>
        </label>
        <label className="toggle-row">
          <input
            checked={payload.config.petOverrideId === null}
            onChange={(event) => {
              if (event.target.checked) {
                call("cmd_set_pet_override", { petId: null }).catch(console.error);
              } else {
                call("cmd_set_pet_override", { petId: payload.overlay.pet.id }).catch(console.error);
              }
            }}
            type="checkbox"
          />
          <span>{strings.autoPetFollow}</span>
        </label>
        <div className="kv-grid compact">
          <div className="kv">
            <span className="kv__label">{strings.codexSelectedPet}</span>
            <span className="kv__value">{payload.codexSelectedPetId ?? strings.noCustomPet}</span>
          </div>
          <div className="kv">
            <span className="kv__label">{strings.effectivePet}</span>
            <span className="kv__value">{payload.overlay.pet.displayName}</span>
          </div>
        </div>
        <label className="stacked-field">
          <span>{strings.manualPetOverride}</span>
          <select
            disabled={payload.config.petOverrideId === null}
            onChange={(event) =>
              call("cmd_set_pet_override", { petId: event.target.value || null }).catch(
                console.error,
              )
            }
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
        <button
          className="secondary-button"
          onClick={() => call("cmd_open_pets_folder").catch(console.error)}
          type="button"
        >
          {strings.openPetsFolder}
        </button>
      </SettingsSection>

      <SettingsSection title={strings.pinSection}>
        <label className="stacked-field">
          <span>{strings.manualSessionOverride}</span>
          <select
            onChange={(event) =>
              call("cmd_set_manual_session", {
                sessionId: event.target.value.length > 0 ? event.target.value : null,
              }).catch(console.error)
            }
            value={pinnedSessionId ?? ""}
          >
            <option value="">{strings.autoFollow}</option>
            {payload.overlay.sessions.map((session) => (
              <option key={session.sessionId} value={session.sessionId}>
                {session.title}
              </option>
            ))}
          </select>
        </label>
        {payload.overlay.manualSessionMissing ? (
          <p className="notice">{strings.fallbackMissingPin}</p>
        ) : null}
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
      </SettingsSection>
    </div>
  );
}

export default function App() {
  return windowLabel === "settings" ? <SettingsApp /> : <OverlayApp />;
}
