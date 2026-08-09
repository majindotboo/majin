# Scoped World persistence

Majin persists selected domain entities and registered components through a scoped Bevy DynamicWorld snapshot. Bevy remaps Entity relationships during hydration, while selective stable IDs support correlation and external contracts. Runtime capabilities, tasks, TUI state, and projections remain transient.
