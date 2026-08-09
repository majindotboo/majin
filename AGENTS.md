# Majin

Terminal agent harness built with Bevy and Ratatui.

## Architecture

- Read `CONTEXT.md` before changing domain terms.
- Read `docs/architecture.md` before changing ECS ownership, plugin boundaries, cameras, providers, tools, persistence, or the TUI state model.
- Keep `src/main.rs` as terminal bootstrap.
- Expose application composition through `MajinPlugin` in `src/lib.rs`.
- Bevy World owns application state and lifecycle.
- Ratatui projects World state and owns no application state.
- Submit harness actions through typed Bevy `Command` values.
- Treat cameras as conceptual ECS projections. Do not add Bevy spatial camera dependencies.
- Keep test bodies under root `tests/`. Do not add `#[cfg(test)]` modules to implementation files.

## Documentation

- Repository documentation is authoritative for Majin.
- Keep project context in root `CONTEXT.md`.
- Keep architecture and decisions under `docs/`.
- Do not read or write Majin documentation in an external vault unless the user explicitly requests it.

## Rules

- App code lives in `src/`.
- Vendored integration lives in `vendor/bevy_ratatui`.
- Keep root and vendored Bevy versions equal.
- Read input through `bevy_ratatui::event`.
- Draw through `RatatuiContext`.
- Keep terminal-only minimal Bevy setup unless requested.
- Reuse existing APIs. Add no needless dependencies or abstractions.
- Keep vendor patches small. Preserve upstream licenses.
- Keep `Cargo.lock` committed.
- Test behavior, not terminal pixels.
- Comments explain why. Delete dead code. Avoid `#[allow(...)]`.

## Checks

Run one Cargo command at a time:

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run
cargo build
```

Use `cargo test --all-targets --all-features` when `cargo nextest` is unavailable.

For vendor changes:

```sh
cargo fmt --manifest-path vendor/bevy_ratatui/Cargo.toml -- --check
cargo clippy --manifest-path vendor/bevy_ratatui/Cargo.toml --all-targets --all-features -- -D warnings
cargo test --manifest-path vendor/bevy_ratatui/Cargo.toml --all-targets --all-features
```

## TUI

- Run `cargo run --locked` in a real terminal after TUI changes.
- Test affected input and terminal restoration.
