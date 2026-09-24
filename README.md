# Stitchr

Combine multiple screenshots into one image — native macOS app with full CLI parity and AI-assisted layout.

## Example

```
stitchr img1.png img2.png --layout grid --max-size 2000 --spacing 10
```

## Architecture

- **`core/`** — Rust crate (`stitchr-core`): image loading, layout algorithms (grid/stack/freeform), spacing/max-size rules, export. Portable, no UI or platform dependencies.
- **`cli/`** — Rust binary (`stitchr-cli`): thin `clap`-based wrapper over `core`. Same binary runs on macOS and Linux.
- **`app/`** *(not yet scaffolded)* — SwiftUI macOS app, binds to `stitchr-core` via [UniFFI](https://mozilla.github.io/uniffi-rs/). Drag-drop canvas, live preview, export panel — calls the same engine as the CLI.
- **AI**: defined as a `SmartLayoutProvider` / `SmartCropProvider` trait in `core`. macOS implementation uses Vision/CoreImage (via a small Swift shim); future Linux (Omarchy) implementation can use a local ONNX/candle model or a cloud API — swappable without touching the core engine or CLI.

## Cross-platform plan

1. macOS v1: Rust core + CLI + SwiftUI app (UniFFI bindings).
2. Linux (Omarchy) later: same core + CLI, new front-end (GTK4 or egui) — no core rewrite.
3. Platform-specific AI providers implement the shared trait; core stays pure/portable.

## Status

Early scaffold — no functionality implemented yet.
