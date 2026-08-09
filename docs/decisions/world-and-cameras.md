# The World and conceptual cameras

Majin uses the Bevy World as its source of truth. Conceptual camera entities project World facts on demand for terminal display and provider context. This keeps Ratatui stateless and avoids duplicated projection state without adding Bevy spatial camera dependencies.
