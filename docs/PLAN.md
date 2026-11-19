# ClipFrame — Architecture & Feature Plan

A fast, low-footprint screenshot tool for Windows 10/11, written in Rust.
Screenshots first (v1); video recording later, on the same capture pipeline.

---

## 1. Guiding principles

1. **Near-zero cost at idle.** When nothing is on screen, ClipFrame is a tray icon and a
   hotkey listener: target **< 10 MB private memory, 0 % CPU**, no GPU device held.
2. **GPU for pixels, CPU for decisions.** Compositing, blur, pixelate, spotlight and
   annotation rendering run on the GPU. The CPU handles input, geometry and encoding.
3. **Event-driven, never polling.** Windows repaint only when state changes. No timers
   ticking while idle.
4. **One renderer.** The on-screen preview and the exported file come from the same GPU
   pipeline, so what you see is exactly what gets saved.
5. **Non-destructive editing.** The original capture is never modified; annotations are a
   vector document flattened only on save/copy.

### Performance budgets (measured in CI benchmarks + manual checks)

| Metric | Target |
|---|---|
| Idle RAM (tray only) | < 10 MB private bytes (M0 measured: 5.6 MB private, 14.8 MB working set incl. shared system DLLs) |
| Idle CPU | 0 % (no wakeups) |
| Hotkey → frozen overlay visible | < 150 ms cold GPU, < 60 ms warm |
| Editor interaction | 60 fps+, repaint on change only |
| Copy-to-clipboard after edit | < 100 ms after debounce for a 4K image |
| RAM with editor open (single 4K capture) | < 120 MB |

---

## 2. Tech stack

| Layer | Choice | Notes |
|---|---|---|
| Language | Rust (edition 2024), Cargo workspace | |
| Windowing / event loop | `winit` (used directly, **not** eframe) | One event loop for the whole process lifetime; windows created and destroyed on demand. eframe can't be torn down and restarted, which blocks the "drop the GPU when idle" goal. |
| GPU | `wgpu` (DX12 backend, Vulkan fallback) | One shared device for overlay + editor. |
| 2D vector rendering | `vello` on the shared wgpu device | GPU compute renderer for shapes, arrows, strokes, text. Fallback if it proves immature: `vello_hybrid` / `tiny-skia`. |
| Text layout | `parley` (+ `fontique` system fonts) | Feeds glyph runs to vello. |
| Effects | Custom WGSL shaders | Pixelate, blur, spotlight mask, highlighter (multiply), magnifier loupe. |
| UI chrome (toolbars, settings, onboarding) | `egui` via `egui-winit` + `egui-wgpu` | Same device; canvas is a vello-rendered texture shown inside egui. |
| Screen capture | `windows-capture` (Windows.Graphics.Capture), DXGI Duplication fallback | Same API later produces video frames. |
| Win32 / WinRT | `windows` crate (windows-rs) | DPI, UI Automation, clipboard, OCR, toasts, registry. |
| Global hotkeys | `global-hotkey` | Integrates with the winit loop. Low-level keyboard hook only as fallback. |
| Tray | `tray-icon` | |
| OCR | `Windows.Media.Ocr` (built into Windows) | No extra model files or RAM; respects installed OS languages. |
| QR / barcodes | `rxing` (Rust port of ZXing) | QR, Data Matrix, Aztec, PDF417, EAN/UPC, Code 128, … |
| Image encoding | `png` (fast preset for clipboard, better preset for files), `image` for JPEG/WebP | Always on a background thread. |
| Config | `serde` + `serde_json`, `%APPDATA%\ClipFrame\config.json` | Portable mode: an empty `portable` file next to the exe keeps everything in `<exe dir>\data`. |
| Logging / profiling | `tracing` + `tracing-appender` (rolling file); `tracy` feature flag for profiling builds | |
| Errors | `thiserror` in library crates, `anyhow` only in the binary | |
| Installer + updates | **Velopack** (Rust SDK) | Single-click branded setup, delta auto-updates, no admin rights needed. Onboarding happens **in-app on first launch** (see §7). |

### GPU lifecycle

- No GPU device at idle.
- On hotkey, the capture starts **in parallel** with device creation, so the cold-start
  cost is hidden behind the capture itself.
- After the last window closes, the device is kept warm for a configurable period
  (default 5 min), then dropped. Setting: *"Keep GPU ready"* = never / 5 min / always.

### Crate layout

```
clipframe/
├─ crates/
│  ├─ cf-core/      # pure logic: geometry, document model, config, undo/redo — no Win32, fully unit-testable
│  ├─ cf-platform/  # all unsafe/Win32/WinRT: capture, DPI, UIA, clipboard, OCR, toasts, hotkeys
│  ├─ cf-render/    # wgpu device mgmt, vello scene building, WGSL effects, offscreen export
│  ├─ cf-detect/    # QR/barcode (rxing), OCR result parsing, sensitive-data detection (emails, IPs, tokens)
│  └─ cf-app/       # binary: event loop, windows (overlay, editor, settings, onboarding), tray
├─ docs/
└─ installer/       # Velopack config, branding assets
```

Dependency direction: `cf-app → cf-render, cf-platform, cf-detect → cf-core`. `cf-core`
depends on nothing platform-specific.

---

## 3. Capture (v1)

- **Frozen screen.** On hotkey, every monitor is captured first, then the overlay shows
  over the frozen frames. Open menus and tooltips survive.
- **Multi-monitor.** One borderless, topmost, skip-taskbar overlay window per monitor.
  Per-Monitor DPI Awareness v2 in the manifest. Selections can span monitors; the result
  is stitched in physical pixels. Must be tested with mixed scaling (100 % + 150 %).
- **HDR.** HDR monitors are captured as FP16 and tone-mapped to SDR on the GPU.
- **Modes** (default configurable; switch live with keys):
  | Key | Mode |
  |---|---|
  | `F` | Free select (default) |
  | `R` | Rectangle |
  | `W` | Window — z-ordered window rects snapshotted via `EnumWindows` at capture time, so hover is instant |
  | `E` | Element — UI Automation hit-test on hover (background thread, debounced, cached). Works in Chromium/Edge/Firefox (DOM elements) and native apps. Scroll wheel walks to the parent element. |
  | `C` | Browser content area only (the browser's document element) |
  | `M` | Current monitor |
  | `A` | All monitors |
- **Selection tools.** Magnifier loupe with pixel coordinates; live size readout;
  color picker (copies hex); resize handles after selecting; arrow keys nudge (Shift = 10 px);
  aspect-ratio lock (Shift while dragging).
- **Repeat last region** (its own hotkey).
- **Delay timer** (3 / 5 / 10 s).
- **Include cursor** toggle.
- **Window capture options.** With/without shadow, keep rounded corners, transparent background.

---

## 4. Editor

### Document model

```
Document
├─ base: captured bitmap (immutable, GPU texture)
├─ crop / canvas padding
├─ spotlight: SpotlightLayer   (one global layer, see below)
└─ items: Vec<Annotation>      (z-ordered, each selectable / movable / restylable)
```

All edits go through command objects → unlimited undo/redo.

### Tools

- Pen / freehand: color, opacity, width, line style (solid, dashed, dotted), smoothing
- Line, arrow (several head styles, curved arrows)
- Rectangle / ellipse: outline, filled, or both; corner radius
- Highlighter (multiply blend, so text stays readable)
- Text with an optional background pill; font, size, color
- **Step counter**: auto-increments; renumbers when a step is deleted or reordered
- **Pixelate / redact**: solid fill or heavy pixelation, flattened on export so it
  can't be reversed. Gaussian blur exists as a style but is labelled "not for sensitive data".
- **Spotlight** (see below)
- Magnify inset (zoomed copy of a region placed elsewhere)
- Crop, add padding/border/background
- Eyedropper, color presets, saved tool styles

### Spotlight: one global dim layer

- A single `SpotlightLayer { dim_opacity, feather, holes: Vec<Shape> }`.
- The dim is one full-canvas quad; spotlights are **holes** in its mask.
- The mask is the **union** of all holes (max-alpha in the shader), so overlapping or
  added spotlights never darken the image further and never double-brighten.
- Dim intensity and edge feather are global settings on the layer; each hole can be a
  rectangle or ellipse and is moved/resized like any other item.
- Removing the last hole removes the dim.

### Clipboard behaviour

- Setting: **auto-copy on capture** (on/off).
- Setting: **update clipboard on edit** (on/off). Re-renders and re-copies after a
  300 ms debounce on a background thread.
- Clipboard formats: PNG + `CF_DIBV5`, so transparency survives in apps that support it.
- Optional "copy file path instead of image".

---

## 5. Smart features (v1)

- **OCR → copy text.** Whole image or a selected region; keeps line breaks.
- **QR / barcode → copy data.** Scanned automatically after capture (background thread).
  When something is found, a chip appears: *Copy* / *Open link* (only for `http(s)` URLs,
  with the URL shown before opening).
- **Auto-redact suggestions.** OCR + regex detect emails, IP addresses, phone numbers and
  token-looking strings; one click adds redact boxes.

---

## 6. Workflow

- **After-capture action**: open editor / copy only / save silently (configurable).
- **Save location**: folder picker, filename template (`{date}`, `{time}`, `{app}`,
  `{title}`, `{n}`), optional date subfolders, format (PNG/JPEG/WebP) and quality.
- **Toast notification** with *Open* / *Edit* / *Show in folder* actions.
- **Pin to screen**: floating always-on-top image, click-through toggle, opacity.
- **Drag & drop out of the editor** as a file into other apps.
- **Capture history** panel (thumbnails stored on disk, never kept in RAM).
- Tray app, start on login, single instance.
- PrintScreen takeover: detect Windows' "Use Print screen to open screen capture"
  setting (`HKCU\Control Panel\Keyboard\PrintScreenKeyForSnippingEnabled`) and offer to
  disable it when the user binds PrintScreen.

---

## 7. Installer & onboarding

- **Velopack** builds a branded single-click `Setup.exe` (custom splash, no admin
  rights, per-user install) and handles **delta auto-updates**.
- **Onboarding runs in-app on first launch** rather than inside the installer, so the
  same screens can be reopened later from Settings → "Run setup again".
  1. Welcome
  2. Pick capture hotkey (+ PrintScreen takeover offer)
  3. Save folder & filename template
  4. Clipboard toggles (auto-copy, update on edit)
  5. Default capture mode
  6. Start with Windows
  7. Short interactive tutorial: take your first capture
- Later: winget manifest, optional MSI for enterprise.

---

## 8. Milestones

| # | Milestone | Content |
|---|---|---|
| M0 | Skeleton | Workspace, lints, CI, tray, hotkey, config, logging |
| M1 | Capture | Frozen multi-monitor overlay, free/rect/monitor modes, loupe, save + clipboard |
| M2 | Window & element modes | EnumWindows snapshot, UIA element hit-testing, browser content area |
| M3 | Editor core | Document model, undo/redo, pen/line/arrow/shapes/text, vello rendering, export |
| M4 | Effects | Pixelate/redact, blur, highlighter, global spotlight layer, magnify inset, crop |
| M5 | Smart | OCR copy, QR/barcode chips, auto-redact suggestions |
| M6 | Workflow | Toasts, pin to screen, history, drag-out, filename templates |
| M7 | Ship | Velopack installer, onboarding, auto-update, performance pass vs budgets |
| Later | Video | WGC frames → Media Foundation hardware H.264/HEVC, region/window recording, GIF export |

---

## 9. Risks / open questions

- **vello maturity.** If text quality or a needed feature falls short, swap the renderer
  behind the `cf-render` trait (fallback: `vello_hybrid` or `tiny-skia` + upload).
- **UIA latency** in heavy pages: hit-tests can take tens of ms. Mitigate with a background
  thread, debounce and a per-capture element cache.
- **Cold GPU start** on slow machines: measure; the *Keep GPU ready* setting covers it.
- **Zero-copy capture → wgpu** (shared D3D handles) is an optimisation for later. v1 does
  one CPU readback per monitor, which OCR/QR/saving need anyway.
