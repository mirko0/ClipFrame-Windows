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
| Hotkey → frozen overlay visible | < 150 ms (measured, 2 × 1080p, RX 580: ~250 ms new helper, ~100–130 ms warm helper) |
| Editor interaction | 60 fps+, repaint on change only |
| Copy-to-clipboard after edit | < 100 ms after debounce for a 4K image |
| RAM with editor open (single 4K capture) | < 120 MB |

---

## 2. Tech stack

| Layer | Choice | Notes |
|---|---|---|
| Language | Rust (edition 2024), Cargo workspace | |
| Windowing / event loop | `winit` (used directly) | One event loop for the whole process lifetime; windows created and destroyed on demand. |
| GPU | **Direct3D 11 + Direct2D** via windows-rs (`cf-render`) | Decided in M1 after measuring: ~65–110 ms / ~7 MB to create vs ~230–410 ms / ~150 MB for wgpu on Direct3D 12 (which also never released below ~85 MB). Flip-model swap chain per window. |
| 2D vector rendering | Direct2D | Paths, strokes with dash styles, rounded shapes, gradients, anti-aliasing: everything the annotation tools need. |
| Text | DirectWrite | System fonts, ClearType-quality layout. |
| Effects | Direct2D built-in effects | Gaussian blur, scale (pixelate), blend modes (highlighter = multiply), masks (spotlight), all on the GPU. |
| UI chrome (toolbars, settings, onboarding) | Toolbars drawn with Direct2D; settings/onboarding window **to decide in M3** (native Win32 controls vs WebView2) | egui was dropped together with wgpu. |
| Screen capture | GDI `BitBlt` in M1 (fast, no start-up cost); Windows.Graphics.Capture / DXGI Duplication later for HDR and video | Same `capture_rect` signature either way. |
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

- No GPU device until the first capture.
- On hotkey, the screen is captured on a worker thread **in parallel** with device creation.
- After the last window closes, the device is kept for a configurable period (default
  5 min), then dropped. Setting: *"Keep GPU ready"* = never / 5 min / always.
- **Why a helper process:** once a swap chain has been presented, the GPU driver keeps
  ~40–60 MB inside the process even after the device is dropped (AMD RX 580), and recreating
  the device repeatedly creeps upward. So all GPU work runs in a **capture helper process**
  (`clipframe.exe --capture-helper <request>`), never in the tray:
  - The tray finds a running helper through a hidden message-only window
    (`ClipFrame.CaptureHelper`) and posts it the request; if none runs, it starts one. Either
    way it first passes on foreground rights (`AllowSetForegroundWindow`) so the overlay gets
    keyboard focus.
  - The helper re-reads settings on every request, stays alive for the *Keep GPU ready*
    period, then exits (after waiting for unfinished saves). "Repeat last region" survives
    the exit through `%LOCALAPPDATA%\ClipFrame\state.json`.
  - Measured (2 × 1080p, RX 580): tray **5.9 MB** throughout; helper ~59–63 MB while alive;
    hotkey → overlay **~250 ms** with a new helper, **~100–130 ms** with a warm one.

### Crate layout

```
clipframe/
├─ crates/
│  ├─ cf-core/      # pure logic: geometry, document model, config, undo/redo — no Win32, fully unit-testable
│  ├─ cf-platform/  # all unsafe/Win32/WinRT: capture, DPI, UIA, clipboard, OCR, toasts, hotkeys
│  ├─ cf-render/    # D3D11/Direct2D/DirectWrite behind safe types: Gpu, WindowSurface, GpuImage, Canvas
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
  | `E` | Element — UI Automation hit-test on hover (background thread; only the newest query is answered). Works in Chromium/Edge/Firefox (DOM elements) and native apps. Scroll wheel walks to the parent element and back. |
  | `C` | Browser content area only (the browser's document element) |
  | `M` | Current monitor |
  | `A` | All monitors |
- **Snapping.** Free select snaps to the window under the pointer on a plain click (no
  drag). The overlay covers the screen, so `ElementFromPoint` would only find the overlay:
  instead the UIA tree of the window under the pointer (from the snapshot) is descended,
  keeping the smallest child containing the point (control view, cached bounding rects,
  1 s UIA timeouts so a hung app can't stall the picker).
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

### Implementation (M3)

- `cf_core::annotate`: `Document` (annotations + `Edit` undo/redo, step numbers derived from
  order), `EditorState` (tool, style, selection, gestures, text drafts) — all unit-tested.
- The editor window lives in the capture helper and reuses its Direct2D device. The same
  annotation renderer draws on screen (with a view transform) and for export
  (`cf_render::render_to_image`, read back from a CPU-readable bitmap), so the copied/saved
  image matches the screen exactly.
- The toolbar is drawn with Direct2D as well — no UI toolkit in the editor.
- After an interactive capture the image is delivered (clipboard/save) immediately, then the
  editor opens (`editor.open_after_capture`, default on). Edits re-copy after 300 ms
  (`clipboard.update_on_edit`); Ctrl+C copies, Ctrl+S saves a new file (then overwrites it).
- Keys: V select · P pen · L line · A arrow · R rect · E ellipse · T text · N step ·
  1–8 colour · [ ] thickness · D dash · F fill · O opacity · Ctrl+Z/Y · Delete · Esc
  (M4 tools below).
- Not yet: resize handles on annotations, curved arrows, corner radius, zoom/pan beyond
  fit-to-window, eyedropper, saved styles (later milestones).

### Implementation (M4)

- New items: `Highlight`, `Blur`, `Redact` (pixelate; a solid box when the style is filled),
  `Spotlight` (a hole), `Magnify` (source rect + inset at 2×, placed beside the source and
  kept inside the image; moving it moves only the inset). The crop is a document property
  (`Edit::Crop`, undoable); annotations keep full-image coordinates and the crop only limits
  the view and the export. With the crop tool active the whole image is shown with the
  outside dimmed; Delete removes the crop.
- Rendering layers, bottom to top: image → region effects → the one spotlight dim layer →
  everything else (magnify insets stay bright).
  - Blur: Direct2D Gaussian blur effect over the captured image, clipped to the region.
  - Pixelate: Scale effect down (high-quality cubic) then up (nearest neighbour), so blocks
    align with the image origin. If an effect fails to render, the region is filled solid,
    so a redaction can never show the original pixels.
  - Spotlight: one geometry = image rect *excluding* the winding-mode union of all holes,
    filled once — overlapping holes never double-brighten. Dim strength is
    `editor.spotlight_dim` (percent, default 55).
  - Highlighter: `D2D1_PRIMITIVE_BLEND_MIN` — each channel becomes the darker of ink and
    background, so black text stays black and overlapping strokes don't stack. It keeps its
    own style (yellow by default).
- Keys: H highlighter · X pixelate/redact · B blur · S spotlight · M magnify · C crop.
  Thickness is one cycling toolbar button (`[` `]` still step it); it also sets blur
  strength, pixel size and highlighter width.
- Fixed in M4: a one-point pen/highlighter stroke (the first mouse-down) handed Direct2D an
  empty `AddLines` slice and crashed the helper; single points now draw a dot.
- Not yet: elliptical spotlights and edge feather, padding/border/background, a straight
  highlighter (Shift), a connector line for magnify insets.

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
| M0 ✅ | Skeleton | Workspace, lints, CI, tray, hotkey, config, logging |
| M1 ✅ | Capture | Frozen multi-monitor overlay, free/rect/monitor modes, loupe, save + clipboard |
| M2 ✅ | Window & element modes | EnumWindows snapshot, UIA element hit-testing, browser content area |
| M3 ✅ | Editor core | Document model, undo/redo, pen/line/arrow/shapes/text, Direct2D rendering, export |
| M4 ✅ | Effects | Pixelate/redact, blur, highlighter, global spotlight layer, magnify inset, crop |
| M5 | Smart | OCR copy, QR/barcode chips, auto-redact suggestions |
| M6 | Workflow | Toasts, pin to screen, history, drag-out, filename templates |
| M7 | Ship | Velopack installer, onboarding, auto-update, performance pass vs budgets |
| Later | Video | WGC frames → Media Foundation hardware H.264/HEVC, region/window recording, GIF export |

---

## 9. Risks / open questions

- **Helper hand-off race:** a request posted in the instant between the helper deciding to
  exit and destroying its message window is lost (the user presses the hotkey again).
  Rare (sub-millisecond window); fix if it ever shows up in practice.
- **Settings/onboarding UI toolkit** is open (native Win32 vs WebView2); decide in M3.
- **UIA latency** in heavy pages: hit-tests can take tens of ms. They run on a background
  thread and stale queries are dropped; a per-capture element cache is still open.
- **Chromium accessibility** switches on at the first UIA query, so the very first hover
  in a browser may only find the window; the document lookup retries once.
- **HDR monitors**: GDI capture returns the SDR-mapped image; switch to
  Windows.Graphics.Capture with tone-mapping when HDR support is added.
- **Controlled Folder Access** (Windows ransomware protection) can block saving into
  Pictures; ClipFrame falls back to `%LOCALAPPDATA%\ClipFrame\captures` and logs how to
  allow it. Onboarding (M7) should detect this and explain it.
