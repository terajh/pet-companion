# Claude Pet Companion

macOS menu bar companion that renders a Codex-compatible desktop pet and maps it to local Claude Desktop session activity.

## Stack

- Tauri 2
- React 19
- TypeScript
- Vite
- Rust

## What it does

- Reads custom pets directly from `~/.codex/pets`
- Follows the currently selected Codex custom pet from `~/.codex/.codex-global-state.json`
- Falls back to `bori` when Codex has no active custom pet
- Tracks local Claude Desktop sessions from:
  - `~/Library/Application Support/Claude/claude-code-sessions`
- Anchors to the active Claude window when permission is available
- Falls back to detached overlay mode when Accessibility is not granted
- Hides the session card while Claude is focused
- Replays a completion animation when `completedTurns` increases

## Development

```bash
pnpm install
pnpm tauri dev
```

## Build

```bash
pnpm build
pnpm tauri build --debug
```

Debug app bundle:

`src-tauri/target/debug/bundle/macos/Claude Pet Companion.app`

## Project layout

```text
claude-pet-companion/
├── src/
│   ├── App.tsx
│   ├── App.css
│   ├── main.tsx
│   └── types.ts
└── src-tauri/
    ├── Cargo.toml
    ├── Info.plist
    ├── tauri.conf.json
    └── src/
        ├── lib.rs
        └── main.rs
```
