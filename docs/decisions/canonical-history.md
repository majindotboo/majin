# Canonical normalized history

Majin persists normalized messages, tool facts, and context-shaping timeline facts as canonical history. Provider adapters rebuild native requests and preserve opaque provider artifacts needed for replay. This keeps Sessions portable across providers without making raw transport payloads the domain model.
