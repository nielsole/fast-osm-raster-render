# AGENTS.md

## Commit Gate: Germany Startup Time

For every commit in this repository:

1. Measure Germany load/startup time using:
`cargo run --release -- ../go-gl-osm/germany-prepared.osm.pbf --load-stats-only`
2. Record the emitted `LOAD_STATS` line in the commit message body, PR description, or commit notes.
3. If startup time regresses, include a short explanation and next optimization step.

Goal: systematically reduce startup/load time over successive commits.
