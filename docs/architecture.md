# Architecture

This document defines the target architecture.
The current TUI is intentionally minimal, but it follows the production ownership and persistence boundaries below.

## Composition

Majin stays one Cargo package until a second consumer requires a separate crate.

- `src/main.rs` starts the terminal application.
- `src/lib.rs` exposes `MajinPlugin`.
- `MajinPlugin` composes `HarnessPlugin`, `CameraPlugin`, `PersistencePlugin`, and `TuiPlugin`.
- Small data modules do not become plugins unless they own systems or lifecycle.

## Ownership

The Bevy World is the source of truth.
Entities and components own domain state.
Resources hold true singletons.
Ratatui stores no application state.

A `TuiView` entity owns composer and focus state.
Camera entities own selection and viewport state through type-specific components.
An `ActiveSession` resource identifies the selected Session.

## Domain model

Majin supports multiple Agent entities and selects one Agent initially.
An Agent references one Model entity and explicit tool exposure entities.
A Model references one Provider entity.
A ToolDefinition is reusable across Agents.

A Session is persistent and branching.
Each Turn references its parent Turn.
The Session stores its active branch head.
New work extends the active head.
Retry creates a sibling branch.
A monotonic sequence records creation order.
Queries sort by sequence when order matters.
Bevy query iteration order is never an ordering contract.

Messages, model requests, model responses, tool uses, tool outcomes, failures, model changes, compaction, recovery, and branch selection use separate archetypes.
Transcript is a projection of these facts.
Transcript is not an entity type.

## Commands and schedule

TUI systems submit typed Bevy `Command` values.
Commands such as `SubmitPrompt` and `InterruptTurn` own validation and World mutation.
TUI systems do not directly spawn or mutate harness domain entities.

`MajinPlugin` defines this ordered pipeline:

1. `Input`
2. `Orchestrate`
3. `Dispatch`
4. `Apply`
5. `Project`
6. `Persist`
7. `Render`

Capability plugins place systems into these sets.

## Asynchronous work

ModelRequest and ToolUse entities own durable request identity and payload state.
Transient task components own active async handles.
Typed result messages carry Session, Turn, work, and generation identity.
Apply systems reject stale results.

Interrupt targets the active Turn.
Interrupt signals child work, increments generation, removes transient handles, and persists cancellation outcomes.
Late outcomes cannot mutate the Session.

A saved in-flight operation is not restored as running.
Hydration converts orphaned work into an interrupted or resumable outcome.

## Cameras

Majin cameras are conceptual.
They do not use Bevy spatial camera types.

Camera instances are entities with type-specific components such as TranscriptCamera and ContextCamera.
CameraPlugin registers projection systems and pure helpers for each camera type.
There are no CameraDefinition entities.

Projection is consumer-driven.
The TUI projects a TranscriptCamera during render.
A provider request projects a ContextCamera when it builds a ModelRequest.
Camera output is temporary.
Camera output is not cached until measured cost requires caching.

A ContextCamera returns a provider-neutral ContextDocument.
The selected Provider converts that document into its native request format.

## History and providers

Normalized messages and tool facts are canonical history.
Provider-native request bodies are rebuilt by Provider adapters.
Canonical facts preserve provider, model, API, response ID, usage, stop reason, tool call identity, and opaque provider artifacts required for replay.
Raw transport payload capture is optional debug data.
Credentials and configured secrets are never persistent history.

Model changes, compaction, recovery, and branch selection are first-class timeline facts.
Cameras decide which facts affect terminal display and provider context.

## Persistence

Persistence uses an explicit, versioned JSONL event log.
Each Session owns one append-only file under `~/.majin/sessions/session-<id>.jsonl`.
There is no central index and no per-turn file; the sessions directory is discovered directly from the filesystem.

Records contain domain IDs and typed references, never Bevy Entity values or Rust module paths.
The Bevy World is rebuilt by replaying a session log.
Capability definitions, transient tasks, TUI views, terminal cameras, projections, and runtime resources stay outside it.
The persistent ContextCamera budget is recorded as session state because it affects future provider context.

Each record carries a schema version and per-session ordinal.
Writes append and sync complete records.
An incomplete final JSONL record is truncated during recovery; a malformed complete record is preserved and blocks replay.
The default storage path is `~/.majin/sessions`; callers can override it through `PersistenceConfig`.
Majin uses a `HarnessReady` resource to gate commands until capability registration and hydration finish.
The TUI renders a loading state before that gate opens.

Schema migrations are explicit and versioned.

## Failures

Expected provider, tool, cancellation, and recovery failures become outcome entities.
Invalid harness commands emit typed result events.
Only corrupted invariants or terminal failure stop the application.

Tool approval policy is deferred.
Do not encode current development behavior as a permanent authorization contract.

## Tests

All test bodies live under root `tests/`.
Tests exercise public behavior through MajinPlugin and World state.
Implementation files contain no `#[cfg(test)]` modules.
Tests assert behavior and state transitions.
Tests do not assert terminal pixels.

## Decisions

- [The World and conceptual cameras](decisions/world-and-cameras.md)
- [Canonical normalized history](decisions/canonical-history.md)
- [Scoped World persistence](decisions/scoped-world-persistence.md)
