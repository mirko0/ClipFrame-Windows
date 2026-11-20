# ClipFrame

Lightweight, GPU-accelerated screenshot tool for Windows, written in Rust.

- Architecture, stack, features and milestones: [docs/PLAN.md](docs/PLAN.md)
- Code conventions (size limits, errors, unsafe, tests, perf rules): [docs/CONVENTIONS.md](docs/CONVENTIONS.md). Follow it for every change.

Key constraints:
- Idle footprint is a top priority: < 10 MB RAM, 0 % CPU, no GPU device held while idle.
- All `unsafe` lives in `cf-platform` (Win32/WinRT) and `cf-render` (Direct3D 11 / Direct2D); `cf-core` stays platform-free and fully unit-tested.
- Before finishing a change, run `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`.
