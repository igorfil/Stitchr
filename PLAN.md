# Stitchr — Development Plan

## Phase 0: Foundation (scaffold — done)
Cargo workspace, empty `core`/`cli` crates, repo live.

## Phase 1: Core Engine (Rust, portable)
- Image I/O (PNG/JPEG decode via `image` crate)
- Layout algorithms: `grid`, `stack` (horizontal/vertical), `freeform`
- Rules engine: `max-size`, `spacing`, alignment
- Composition + export (PNG/JPEG output)
- Unit tests per layout algorithm (deterministic pixel-position assertions)

## Phase 2: CLI (Rust, `clap`)
- Arg spec matching example: `stitchr img1.png img2.png --layout grid --max-size 2000 --spacing 10`
- Subcommands only if needed later (e.g. `stitchr ai-layout ...`); keep flat for v1
- CLI is a thin pass-through to `core` — no logic duplicated
- Extensive `--help` output: every flag documented with type, default, allowed values, and examples inline (`clap`'s `long_about`/`after_help`). Structured/predictable text so an AI agent can parse `--help` and self-discover usage without external docs.
- `--help-json` / `--schema` flag exposing the full CLI spec as structured JSON (flags, types, enums, defaults) — most reliable for AI tool-use/function-calling integration, vs. parsing free-text help.

## Phase 3: AI Layer (trait-based, pluggable)
- Define `SmartLayoutProvider` / `SmartCropProvider` traits in `core`
- macOS impl: Vision/CoreImage via small Swift shim, exposed back to Rust
- v1 AI features: auto-suggest layout based on image aspect ratios; smart crop/trim borders
- Cloud fallback (optional flag) for heavier layout reasoning later

## Phase 4: macOS App (SwiftUI + UniFFI)
- UniFFI-generate Swift bindings from `core`
- UI: drag-drop canvas, layout picker, live preview, spacing/max-size controls, export panel
- App and CLI both call the same `core` — guarantees feature parity by construction

## Phase 5: Polish & Distribution
- App icon, notarization, DMG or direct `.app` distribution
- `brew install stitchr` for CLI (Homebrew tap)
- Basic onboarding / empty-state UI

## Phase 6 (later): Linux / Omarchy
- Reuse `core` + `cli` unchanged
- New front-end (GTK4 or egui)
- Linux-native AI provider (ONNX/candle or cloud) implementing the same trait

---

**Milestone order**: Phase 1 → 2 (get CLI fully working first — validates the core) → 4 (app) → 3 (AI, can start simple/heuristic before real ML) → 5 → 6.
