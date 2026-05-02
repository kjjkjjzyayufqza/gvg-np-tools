# Root Eframe Restructure TODO

Hub links:

- Cross-agent hub: `../../../AGENTS.md`
- Cursor rule: `../../../.cursor/rules/gvg-research-tools.mdc`
- Process log: `process.md`

## Task

Move the Rust application out of the old nested crate directory into the
repository root, reshape the project closer to `emilk/eframe_template`, update
AI-only guidance so it points agents at the root Cargo project, and add a root
`README.md` with build, run, and debug instructions.

## TODO

- [x] Read relevant docs and inspect the current Cargo layout.
- [x] Decide the safest root-level file movement plan.
- [x] Move source/Cargo files to root while leaving generated build artifacts
  behind.
- [x] Update project docs/rules that still refer to the old nested crate path.
- [x] Add `README.md`.
- [x] Run formatting/build verification or record why it could not run.
- [x] Record final status and handoff notes in `process.md`.
- [x] Upgrade root Cargo package to Rust 2024 edition and verify it.
