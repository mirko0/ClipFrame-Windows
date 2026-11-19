# ClipFrame — Rust Code Conventions

These rules are enforced by tooling wherever possible (`rustfmt.toml`, `clippy.toml`,
`[workspace.lints]`, CI). Anything not enforced by tooling is checked in review.

---

## 1. Tooling gates (must pass before merge)

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check            # licenses, advisories, duplicate deps
```

Workspace lints (in the root `Cargo.toml`, inherited by every crate via `[lints] workspace = true`):

```toml
[workspace.lints.rust]
unsafe_op_in_unsafe_fn = "deny"
missing_debug_implementations = "warn"
unused_qualifications = "warn"

[workspace.lints.clippy]
pedantic = { level = "warn", priority = -1 }
unwrap_used = "deny"
expect_used = "warn"            # allowed with a message that states the invariant
panic = "deny"
todo = "warn"
dbg_macro = "deny"
print_stdout = "deny"           # use tracing
cognitive_complexity = "warn"
excessive_nesting = "warn"
undocumented_unsafe_blocks = "deny"
module_name_repetitions = "allow"   # pedantic noise
must_use_candidate = "allow"        # pedantic noise
```

Tests may `#[allow(clippy::unwrap_used)]` at module level.

---

## 2. Size limits

| Item | Soft limit | Hard limit | Enforced by |
|---|---|---|---|
| Function / method body | 30 lines | 50 lines | `clippy::too_many_lines` |
| Function parameters | 4 | 5 | `clippy::too_many_arguments` — beyond that, pass a struct |
| Nesting depth | 3 | 4 | `clippy::excessive_nesting` |
| Cognitive complexity | 10 | 15 | `clippy::cognitive_complexity` |
| File / module | 300 lines | 500 lines (excl. tests) | review |
| Struct fields | 8 | 12 — split into sub-structs | review |
| `impl` block | 15 methods | 25 — split by responsibility or trait | review |
| Line width | — | 100 chars | `rustfmt` |

Exceeding a soft limit is allowed with a reason; exceeding a hard limit needs an
`#[allow(...)]` with a comment explaining why.

---

## 3. Structure & architecture

- **Crate boundaries are dependency rules.** `cf-core` must never depend on `windows`,
  `wgpu` or `winit`. If logic needs to be tested, it belongs in `cf-core`.
- **All `unsafe` lives in `cf-platform`** (and in narrowly scoped GPU interop in
  `cf-render`). Every `unsafe` block has a `// SAFETY:` comment stating the invariant.
  Wrap every Win32/WinRT call in a safe function with a Rust-typed signature.
- **One responsibility per module.** Prefer `capture/monitor.rs`, `capture/window.rs` over
  one `capture.rs` with everything in it.
- **Traits at seams.** Platform services (capture, clipboard, OCR, UIA) are used through
  traits, so app logic can be tested with fakes.
- **No global mutable state.** No `static mut`; `OnceLock` only for true process-wide
  singletons (e.g. logging).
- **Visibility**: default to private, then `pub(crate)`; `pub` only for the crate's API.

---

## 4. Naming

- Standard Rust casing: `snake_case` functions/modules, `UpperCamelCase` types,
  `SCREAMING_SNAKE_CASE` constants.
- Conversions follow the API guidelines: `as_` (cheap borrow), `to_` (expensive),
  `into_` (consuming); constructors `new` / `with_*` / `from_*`.
- Getters have no `get_` prefix: `fn width(&self)`.
- Booleans read as questions: `is_pinned`, `has_selection`, `should_copy`.
- **Units in names when ambiguous**: `width_px`, `delay_ms`, `scale_factor`.
  Use newtypes for coordinate spaces: `PhysicalPx`, `LogicalPx`, `CanvasPt`. Never mix
  physical and logical pixels in one function signature.

---

## 5. Error handling

- Library crates: typed errors with `thiserror`, one `Error` enum per crate (or per large
  module). Add context with variants, not strings.
- `cf-app` (binary): `anyhow::Result` with `.context("…")` at boundaries.
- No `unwrap()` outside tests. `expect("…")` only when the message states the invariant
  that makes failure impossible.
- No panics in hotkey, capture or clipboard paths: a failed capture logs and shows a
  toast, it never crashes the tray process.
- Never swallow errors silently. At minimum `tracing::warn!` them.

---

## 6. Performance rules

- **No allocations in per-frame paths.** Reuse buffers (`clear()` instead of new `Vec`),
  keep scene builders alive between frames.
- **Repaint on change only.** Every redraw is triggered by an input or state event.
  No `request_repaint` loops.
- **Heavy work off the UI thread**: encoding, OCR, barcode scanning, UIA queries, disk I/O.
  Use a small dedicated worker pool and channels; no async runtime unless a real need appears.
- **Pixels stay on the GPU.** Read back to the CPU only for save, clipboard, OCR and barcode
  scanning — once per capture, shared between them.
- **Release memory when idle.** Windows, textures and the GPU device are dropped when not
  in use (see PLAN §2 GPU lifecycle).
- **Measure, don't guess.** Performance-sensitive changes include a `criterion` benchmark
  or a `tracing` span measurement in the PR description.
- Prefer `&[T]` / `&str` parameters over owned types; `Cow` where ownership varies.
- Add a dependency only after checking its size and transitive tree (`cargo tree`,
  `cargo bloat`). Disable unused default features.

---

## 7. Testing

| Kind | Location | What |
|---|---|---|
| Unit | `#[cfg(test)] mod tests` at the bottom of the same file | Pure logic: geometry, DPI conversion, undo/redo, step renumbering, filename templates, config migration |
| Property | `proptest` in unit tests | Coordinate transforms round-trip, selection clamping, spotlight mask union |
| Integration | `crates/<crate>/tests/` | Public API of each crate, with fake platform services |
| Golden image | `cf-render/tests/golden/` | Render a document offscreen, compare to a reference PNG with a per-pixel tolerance. Update with `UPDATE_GOLDEN=1`. |
| Benchmarks | `benches/` with `criterion` | Render 4K document, PNG encode, clipboard copy, barcode scan |
| Platform | `#[ignore]` tests in `cf-platform` | Need a real desktop/GPU; run locally with `cargo test -- --ignored` |

Rules:

- Every bug fix comes with a test that fails without the fix.
- Test names describe behaviour: `spotlight_overlapping_holes_do_not_double_brighten`.
- Arrange / Act / Assert, one behaviour per test.
- Tests are deterministic: no sleeps, no real clock (inject a `Clock`), no network.
- `cf-core` targets ≥ 80 % line coverage (`cargo llvm-cov`).

---

## 8. Documentation & comments

- Every `pub` item has a `///` doc comment; `#![warn(missing_docs)]` in library crates.
- Comments explain **why**, not what. Non-obvious Win32 behaviour gets a link to the
  Microsoft docs page.
- `// TODO(#issue): …`: TODOs reference an issue, never a bare TODO.

---

## 9. Logging

- `tracing` everywhere; `error!` for user-visible failures, `warn!` for recovered ones,
  `debug!`/`trace!` for diagnostics.
- Never log image contents, OCR text, clipboard data or file contents (privacy).
- Spans around capture, render, encode and save, for profiling.

---

## 10. Git

- Conventional commits: `feat(capture): …`, `fix(editor): …`, `perf(render): …`,
  `refactor:`, `test:`, `docs:`, `chore:`.
- One logical change per commit; the build and tests pass at every commit.
- Branches: `feat/<short-name>`, `fix/<short-name>`.
