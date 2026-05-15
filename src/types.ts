export type PetAnimationState =
  | "idle"
  | "sleeping"
  | "running"
  | "waiting"
  | "waving"
  | "jumping"
  | "review"
  | "failed";

export type SessionAppKind = "claude" | "codex";

export interface SessionSummary {
  appKind: SessionAppKind;
  assistantPreview: string | null;
  completedPreview: string | null;
  completedTurns: number | null;
  cwd: string;
  inProgress: boolean;
  isArchived: boolean;
  lastActivityAt: number;
  sessionId: string;
  title: string;
  userPreview: string | null;
}

export interface PetDescriptor {
  description: string;
  displayName: string;
  id: string;
  source: "custom";
  spriteSheetPath: string;
}

export interface CompanionConfig {
  attached: boolean;
  language: "en" | "ko";
  trackedApp: "auto" | SessionAppKind;
  petOverrideId: string | null;
  petScale: number;
  watchClaude: boolean;
  watchCodex: boolean;
}

export interface OverlaySnapshot {
  activeSession: SessionSummary | null;
  claudeFrontmost: boolean;
  codexFrontmost: boolean;
  currentWindowTitle: string | null;
  effectiveState: PetAnimationState;
  messagePreview: string | null;
  permissionGranted: boolean;
  pet: PetDescriptor;
  sessions: SessionSummary[];
  showCard: boolean;
  stateLabel: string;
  dismissedSessionIds: string[];
  completedRuntimeSessionIds: string[];
  cardsBelow: boolean;
}

export interface AppPayload {
  codexSelectedPetId: string | null;
  config: CompanionConfig;
  overlay: OverlaySnapshot;
  pets: PetDescriptor[];
}
