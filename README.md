# ClipFrame for Windows

A fast, lightweight screenshot tool for Windows 10 and 11, written in Rust and drawn with the
GPU. It waits in the tray using about 6 MB of memory and no CPU, and is ready the moment you
press its shortcut.

## Features

- **Capture anything**: free selection that snaps to windows on a click, rectangles, whole
  windows, single UI elements (down to a DOM element in a browser), a browser's page area,
  a monitor or all monitors — on a frozen screen, so menus and tooltips survive.
- **Annotate** in the built-in editor: pen, highlighter, lines and arrows, rectangles and
  ellipses, text, numbered steps, colours, thickness, dashed/dotted lines, opacity.
- **Hide and highlight**: pixelate or black out, blur, one shared spotlight layer, magnified
  insets, crop. One click hides emails, IP addresses, phone/card numbers and keys found in
  the image.
- **Smart**: copy the text in a capture (Windows OCR), read QR codes and barcodes.
- **Workflow**: copy to the clipboard (kept up to date while you edit), save with
  `{date} {time} {app} {title}` file name templates, notifications, recent captures in the
  tray, pin captures on screen, drag them into other apps.
- **Updates itself** from this repository's releases.

## Install

Download `ClipFrameApp-win-Setup.exe` from [Releases](../../releases) and run it — no admin
rights needed. Windows may show a SmartScreen warning until the installer is code-signed.

Default shortcut: **Alt+Shift+S**. Everything else is in *Settings…* in the tray menu.

## Build

Requires Rust 1.85+ (MSVC toolchain) on Windows.

```powershell
cargo run -p cf-app            # debug build, with a console for logs
cargo build --release -p cf-app
```

Before committing: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`
and `cargo test --workspace`. See [docs/PLAN.md](docs/PLAN.md) for the architecture and
[docs/CONVENTIONS.md](docs/CONVENTIONS.md) for code conventions.

The installer is built with [Velopack](https://velopack.io): install the .NET 8 SDK and
`dotnet tool install -g vpk` (same version as the `velopack` crate), then run
`installer\package.ps1`.

## License

[GPL-3.0-or-later](LICENSE).
