# Session event-log persistence

Majin persists explicit domain events as one versioned JSONL append-only log per Session under `~/.majin/sessions`. Session files are discovered directly from the directory; there is no central index and no file per Turn.

The log contains stable domain IDs and typed references. Bevy Entity values, ECS component names, capability executors, task handles, TUI state, and projections remain runtime-only. Startup replays the log into a fresh Bevy World, recovers incomplete final records, and converts persisted in-flight work into interruption outcomes.
