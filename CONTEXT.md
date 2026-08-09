# Majin Context

## Product

Majin is a terminal agent harness.
Majin owns the TUI, agent lifecycle, provider requests, tool execution, cameras, and persistence in one process.

## Vocabulary

- **World**: The Bevy `World`. It is the application source of truth.
- **Agent**: An entity that selects a Model and an ordered set of exposed tools.
- **Provider**: A capability entity that owns transport and native request conversion.
- **Model**: A capability entity that references a Provider and defines a model ID, limits, and capabilities.
- **ToolDefinition**: A capability entity that owns a tool name, description, input schema, and executor.
- **Session**: A persistent conversation tree with one active branch head.
- **Turn**: One user-triggered agent cycle inside a Session.
- **Work entity**: A ModelRequest or ToolUse entity that represents active asynchronous work.
- **Outcome entity**: A persistent result, failure, cancellation, or interruption fact.
- **Timeline fact**: A persistent entity that affects history or context. Examples include messages, model changes, compaction, recovery, and branch selection.
- **Camera**: A conceptual Bevy entity that selects and projects World facts for one consumer.
- **TranscriptCamera**: A camera that projects a Session branch for terminal display.
- **ContextCamera**: A camera that projects a Session branch into provider-neutral context.
- **ContextDocument**: Ordered provider-neutral camera output. A Provider converts it into a native request.
- **Projection**: A temporary read of World facts. Projections are computed by consumers and are not persisted.
- **Branch head**: The selected leaf Turn in a Session tree.
- **Harness command**: A typed Bevy `Command` that validates and mutates harness state.
