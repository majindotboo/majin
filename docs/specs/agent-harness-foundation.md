# Agent Harness Foundation

## Problem

Majin currently renders a fake transcript from one `TuiView` resource in `src/lib.rs`.
Keyboard and mouse input mutate that resource directly.
No Agent, Session, Turn, provider, tool, camera, async work, branching, or persistence model exists.

The prototype proves terminal rendering and input.
It does not provide the World-owned agent harness defined by `CONTEXT.md` and `docs/architecture.md`.

## Outcome

Majin becomes a one-process ECS agent harness foundation.
A user can start or select a Session, submit a prompt, observe fake provider and tool work, interrupt an active Turn, branch from an earlier Turn, restart Majin, and reopen persisted history.

Ratatui renders World projections through conceptual cameras.
Provider and tool implementations remain fake but use the same capability, command, async work, outcome, and context boundaries required by real implementations.

## Scope

- Add `HarnessPlugin`, `CameraPlugin`, `PersistencePlugin`, and `TuiPlugin` under `MajinPlugin`.
- Add the shared ordered system pipeline.
- Model Agents, Providers, Models, ToolDefinitions, tool exposure, Sessions, Turns, messages, work, outcomes, and timeline facts as ECS entities and components.
- Use separate archetypes for each fact type.
- Add typed Bevy Commands for prompt submission, interruption, Session selection, and branch selection.
- Add consumer-driven TranscriptCamera and ContextCamera projections.
- Add fake asynchronous model and tool executors.
- Add generation-based stale-result rejection and Turn interruption.
- Add branching Sessions with parent-linked Turns and an active head.
- Add scoped DynamicWorld persistence with debounced atomic writes and startup hydration.
- Gate commands behind `HarnessReady`.
- Keep all test bodies under root `tests/`.

## Out of Scope

- Real provider network APIs.
- Real filesystem or shell tools.
- Tool approval policy or sandboxing.
- Save migrations before the first stable format.
- Multi-agent orchestration.
- Camera caching.
- Compaction algorithms beyond representing compaction as a timeline fact.
- Provider request payload capture beyond normalized facts and required opaque replay metadata.

## Current System

- `src/main.rs` is a thin bootstrap that calls `majin::run()`.
- `src/lib.rs` exposes `MajinPlugin` and owns the complete prototype.
- `MajinPlugin` registers `KeyMessage`, `MouseMessage`, `AppExit`, and one `TuiView` resource.
- `handle_input` and `handle_mouse_input` mutate `TuiView` directly.
- `draw` renders `TuiView.transcript` and `TuiView.composer` through `RatatuiContext`.
- `tests/agent_ui.rs` proves prompt submission and mouse scrolling through public World state.
- Bevy uses minimal terminal features.
- `bevy_world_serialization` is not enabled yet.
- Repository rules require root integration tests and forbid test modules under `src/`.

## Implementation Approach

### 1. Composition and scheduling

Keep `src/main.rs` unchanged as terminal bootstrap.
Keep `run()` responsible for terminal-only plugins and the schedule runner.

Move application behavior behind these plugins:

```rust
pub struct MajinPlugin;
pub struct HarnessPlugin;
pub struct CameraPlugin;
pub struct PersistencePlugin;
pub struct TuiPlugin;
```

`MajinPlugin` composes the four capability plugins.
It defines ordered `SystemSet` values:

```rust
pub enum MajinSet {
    Input,
    Orchestrate,
    Dispatch,
    Apply,
    Project,
    Persist,
    Render,
}
```

Order these sets in one chain.
Plugin systems join these sets instead of creating hidden cross-plugin ordering.

### 2. Domain entities

Use public components where integration tests and future clients must inspect World behavior.
Keep executor internals and helper functions private.

Use these durable roots and capability entities:

```rust
Agent { model: Entity }
Provider { provider_id: ProviderId }
Model { provider: Entity, model_id: String }
ToolDefinition { tool_id: ToolId, name: String, description: String }
AgentTool { agent: Entity, tool: Entity, order: u32 }
Session { id: SessionId, active_head: Option<Entity> }
Turn { id: TurnId, session: Entity, parent: Option<Entity>, sequence: u64, generation: u64 }
```

Use separate entities and components for:

- `UserMessage`
- `AssistantMessage`
- `ModelChange`
- `Compaction`
- `Recovery`
- `BranchSelection`
- `ModelRequest`
- `ModelResponse`
- `ToolUse`
- `ToolOutcome`
- `TurnCompleted`
- `TurnFailed`
- `TurnCancelled`
- `TurnInterrupted`

Each fact references its owning Turn or Session with remappable `Entity` fields.
Use selective stable ID newtypes for Sessions, Turns, model requests, and tool calls.
Use a persisted monotonic allocator resource for new stable IDs and creation sequence values.

A normal Bevy query order is never canonical.
Every projection that needs creation order performs an explicit stable sort by sequence and stable ID.

### 3. Commands

Implement typed Bevy `Command` values:

```rust
SubmitPrompt { session: Entity, text: String }
InterruptTurn { turn: Entity }
SelectSession { session: Entity }
SelectBranch { session: Entity, head: Entity }
```

Commands validate target entities and `HarnessReady` before mutation.
Invalid commands emit typed command-result messages.
TUI input translates terminal events into these commands.
TUI systems do not directly mutate harness domain entities.

`SubmitPrompt` creates a UserMessage and a child Turn from the active branch head.
It updates the Session active head and creates the first ModelRequest.

`InterruptTurn` increments Turn generation, signals child tasks, removes transient task components, and creates a TurnCancelled outcome.
Late results with an old generation are rejected.

### 4. Cameras and projections

Use conceptual camera entities with type-specific components:

```rust
TranscriptCamera { session: Entity, head: Entity }
TerminalTranscriptViewport { scroll_from_bottom: usize }
ContextCamera { agent: Entity, session: Entity, head: Entity, budget: usize }
```

Do not add Bevy spatial camera features.
Do not persist terminal cameras.
Persist ContextCamera only when it is Agent domain state.

Transcript projection walks the selected Turn parent chain, gathers visible facts, sorts facts within each Turn, and returns temporary transcript rows.
TUI render calls this projection directly.
Projection output carries semantic roles and content.
Consumer adapters add labels, styles, and viewport behavior.

Context projection walks the selected branch and returns an ordered provider-neutral `ContextDocument`.
Provider adapters convert the document into native requests.
Projection output is not inserted into World and is not cached.

### 5. Capabilities and fake async work

Register Provider, Model, ToolDefinition, and AgentTool capability entities during startup.
Create one selected Agent for the first UI.

Use fake executors that run through Bevy async task infrastructure.
Transient task components hold task handles.
Durable ModelRequest and ToolUse entities hold correlation identity, normalized input, Turn, and generation.

Result messages create normalized ModelResponse, AssistantMessage, ToolOutcome, and Turn outcome entities.
Provider-native request bodies are generated at dispatch time and are not canonical history.
Preserve provider metadata and opaque replay artifacts in normalized results.

A fake prompt must exercise both paths:

1. ModelRequest returns an AssistantMessage that requests one fake tool.
2. ToolUse returns a ToolOutcome.
3. A second ModelRequest returns the final AssistantMessage and completes the Turn.

### 6. Branching

A Session is a Turn tree.
Each Turn stores one optional parent Turn.
Session.active_head selects the current leaf.

Selecting an earlier Turn changes the active head and records BranchSelection.
Submitting from that head creates a sibling branch without copying prior Turns.
Transcript and context cameras walk parent links from the selected head.

### 7. Persistence and hydration

Enable Bevy `bevy_world_serialization` and its serialization support.
Do not enable Bevy spatial camera or window rendering features.

Add a persistent marker or equivalent DynamicWorldBuilder filter for domain entities.
Register every persisted component and resource for reflection and serialization.
Derive or implement entity remapping for persisted `Entity` fields.

Persist:

- Agent domain state and selected Model reference.
- Sessions, Turns, branch heads, normalized facts, work identity, outcomes, and stable ID allocator.
- Provider and tool identity only when required to reconnect persisted references.
- Agent ContextCamera state when it affects future provider context.

Do not persist:

- Capability executors.
- Active task handles.
- TUI views, terminal cameras, focus, composer, or scroll.
- Ratatui output.
- Derived projections.
- Runtime-only resources.

Use one debounced writer.
Any change to persistent state marks persistence dirty.
Write to a temporary file and atomically replace the active snapshot.

Startup order:

1. Register capabilities and reflected types.
2. Load the scoped DynamicWorld when present.
3. Remap entity relationships.
4. Convert orphaned in-flight work into TurnInterrupted outcomes.
5. Rebuild transient links and selected UI state.
6. Set `HarnessReady`.

The TUI shows a loading state before `HarnessReady`.
Harness commands reject or defer execution before readiness.

The development save format can break without migration.
A load error preserves the source file and produces a visible recovery failure.

### 8. TUI migration

Replace the transcript Vec in `TuiView` with a TranscriptCamera entity reference.
Keep composer and focus as view state.
Keep `ActiveSession` as a singleton resource.

Message submission queues `SubmitPrompt`.
Mouse and keyboard scrolling mutate only the terminal viewport component on the TranscriptCamera entity.
Session and branch selection queue typed commands.
Ratatui renders temporary transcript rows returned by the camera projection.

## Acceptance Criteria

- [ ] `src/main.rs` remains terminal bootstrap and contains no application state.
- [ ] `MajinPlugin` composes Harness, Camera, Persistence, and TUI plugins.
- [ ] The shared system sets run in the documented order.
- [ ] A fresh World registers one Provider, one Model, one Agent, one fake ToolDefinition, and explicit AgentTool exposure.
- [ ] Submitting non-empty composer text through terminal input queues a typed command and creates a Session Turn plus UserMessage.
- [ ] Empty composer submission creates no Turn or message.
- [ ] Fake provider and tool work execute through transient async task components and durable work entities.
- [ ] The fake flow renders user text, tool activity, tool outcome, and final assistant text.
- [ ] Interrupting the active Turn produces a cancellation outcome and late results cannot mutate the Session.
- [ ] Transcript rendering is computed from World facts through a TranscriptCamera.
- [ ] Provider context is computed from the selected branch through a ContextCamera and provider-neutral ContextDocument.
- [ ] Context projection order is deterministic and does not depend on Bevy query iteration order.
- [ ] Selecting an earlier Turn and submitting creates a sibling branch without copying history.
- [ ] Restarting Majin restores Sessions, branches, normalized facts, outcomes, stable IDs, and Agent context camera state.
- [ ] Restarting during in-flight work produces an interrupted outcome instead of restoring work as running.
- [ ] TUI composer, focus, terminal scroll, task handles, and projections are not restored.
- [ ] Commands cannot mutate domain state before `HarnessReady`.
- [ ] Persistence failure leaves the prior snapshot intact and creates a visible failure outcome.
- [ ] No Bevy spatial camera or graphical rendering feature is enabled.
- [ ] No test body or `#[cfg(test)]` module exists under `src/`.

## Verification

- Root integration tests construct `App`, add `MajinPlugin`, submit terminal messages, run updates, and query public World components.
- Command tests cover valid targets, invalid targets, empty prompts, interruption, stale generations, branch selection, and readiness gating.
- Camera tests build branched Sessions in World and assert deterministic transcript and context documents.
- Async tests use deterministic fake executors and prove model-tool-model sequencing.
- Persistence tests use a temporary directory, round-trip a scoped snapshot, assert entity remapping, and simulate interrupted work.
- Failure tests corrupt or block snapshot replacement and assert prior-file preservation plus visible recovery failure.
- Run one Cargo command at a time:
  - `cargo fmt --check`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo nextest run`
  - `cargo build`
- Run `cargo run --locked` in a real terminal after TUI changes.
- Verify keyboard input, mouse wheel scrolling, Session selection, interruption, and terminal restoration.

## Risks and Constraints

- Scoped DynamicWorld persistence requires complete reflection registration and entity remapping.
- Entity links outside the serialized scope become invalid.
- Debounced persistence can lose the final unflushed changes during process termination.
- Branch projection must detect malformed parent cycles and missing parents.
- Provider opaque replay artifacts can become incompatible after a model change.
- Tool authorization is intentionally unresolved and must not become an implicit permanent allow policy.
- The internal save format is disposable during development.

## Delivery Plan

### Phase 1: Composition seam

Goal: establish plugins, modules, schedule sets, and test seams without changing visible TUI behavior.

Verification: existing composer and mouse tests pass through `MajinPlugin`.

Completion: prototype behavior runs through the new plugin boundaries.

### Phase 2: World-owned Session flow

Goal: replace transcript storage with Session, Turn, fact, command, and TranscriptCamera entities.

Verification: prompt submission, empty submission, ordering, and branch tests pass.

Completion: transcript is only a projection of World facts.

### Phase 3: Harness execution flow

Goal: add capability entities, ContextCamera, fake async provider work, fake tool work, outcomes, and interruption.

Verification: deterministic model-tool-model and stale-result tests pass.

Completion: one complete fake Turn exercises the real orchestration boundaries.

### Phase 4: Persistence

Goal: add scoped DynamicWorld snapshots, debounced atomic writes, hydration, readiness gating, and interrupted recovery.

Verification: round-trip, remapping, recovery, and failure-preservation tests pass.

Completion: Sessions survive restart without restoring transient state.

### Phase 5: TUI completion

Goal: expose Session and branch navigation, loading, active work, interruption, failures, and restored history in the terminal UI.

Verification: root integration tests pass and real-terminal smoke tests cover all affected input and restoration.

Completion: every acceptance criterion is observable through World state or TUI behavior.

## Implementation Notes

Pending implementation.
