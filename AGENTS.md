# AGENTS.md

This file is AI-only operating guidance for coding agents working in this
repository. It complements human-facing docs and should be treated as the
cross-agent hub for Cursor, Claude, Codex, Copilot, OpenAI-based agents, and
other coding agents that understand repository instructions.

## Project Identity

- This repository is a research and tooling project for
  **Mobile Suit Gundam: Gundam vs. Gundam Next Plus**
  (`ガンダムVS.ガンダムNEXT PLUS`).
- The project investigates game asset formats and modding workflows, especially
  `Z_DATA.BIN`, AFS containers, PZZ archives, PMF2 models, GIM textures, DAE/FBX
  exchange, PPSSPP behavior, and IDA/Ghidra reverse-engineering findings.
- Prefer preserving original binary structure over speculative rebuilds. When
  format knowledge is incomplete, document uncertainty and keep changes narrow.

## Mandatory Task Startup Protocol

Before doing any task, every AI agent must:

1. Read this file first.
2. Read the linked Cursor rule when running inside Cursor:
   `.cursor/rules/gvg-research-tools.mdc`.
3. Search `docs/` for Markdown files relevant to the user's request, then read
   the most relevant analysis notes before touching code or assets.
4. Create or update a topic-specific session folder:
   `docs/agent-sessions/<topic>/`.
5. Maintain both files in that folder:
   - `todo.md` for current tasks, status, and next actions.
   - `process.md` for context gathered, decisions, commands, test results,
     failures, and handoff notes.

If a task already has a suitable session folder, reuse it instead of creating a
duplicate. Choose short kebab-case topic names such as `pmf2-mesh-import`,
`gim-replace`, `gvg-gui-save-planner`, or `gvg-ai-rules`.

## Required Documentation Sources

Use `docs/` as the first source of project truth. Important starting points:

- `docs/MOD_WORKFLOW.md` for the full PMF2 modding workflow.
- `docs/RUST_DAE_PMF2_COMMANDS.md` for Rust converter command examples.
- `docs/PMF2_TODO.md` for known PMF2 import/rendering status.
- `docs/PMF2_M00_RENDER_ANALYSIS.md` for m00 runtime draw-mask findings.
- `docs/PMF2_SPECIAL_SECTIONS_ANALYSIS.md` for special section caveats.
- `docs/GIM_REPLACE_NOTES.md` for GIM texture replacement notes.
- `docs/PPSSPP_OPERATION_ANALYSIS.md` for emulator/runtime behavior.
- `docs/superpowers/specs/` for approved design specifications.

When adding new research findings, write them under `docs/` and link them from
the active session `process.md`.

## Rule And Skill Link Map

`AGENTS.md` is the hub. Every project rule or skill should link back here, and
this section should link to every project-owned rule/skill entry point.

Current project rule entry points:

- Cursor project rule: `.cursor/rules/gvg-research-tools.mdc`
- Cross-agent hub: `AGENTS.md`

Current session records:

- `docs/agent-sessions/gvg-ai-rules/todo.md`
- `docs/agent-sessions/gvg-ai-rules/process.md`

Future project-owned skills should live in one of these conventional locations
when added:

- `.agents/skills/<skill-name>/SKILL.md`
- `.claude/skills/<skill-name>/SKILL.md`
- `.github/skills/<skill-name>/SKILL.md`

Each future `SKILL.md` must:

- Use a specific trigger-oriented description.
- Link back to this `AGENTS.md`.
- Link to any related Cursor rule or session process file.
- Keep bulky reference material in linked files rather than inline.

## Development Conduct

- Prefer existing Rust modules in `src/` over new duplicate
  parsers or ad hoc binary logic.
- For PMF2 edits, preserve template bytes unless there is a documented reason to
  rebuild a section. Be especially careful with matrix noise, GE command order,
  section renderability, PZZ tails, and AFS alignment.
- Treat raw game assets as research artifacts. Do not delete, overwrite, or
  regenerate them unless the user explicitly asks and the process is documented
  in the active session log.
- Keep generated outputs under ignored/output folders when possible. Avoid
  committing build artifacts such as `target/`.
- Do not use Cursor's internal search/grep tools for broad codebase searching in
  this workspace; the project rule reports they can hang. Prefer shell-based
  commands such as `git grep`, `git ls-files`, and PowerShell file commands.

## Verification And Handoff

- Run the narrowest reliable verification command for the change. For Rust work,
  use root-level Cargo commands such as `cargo test` or a focused package/test
  target when available.
- Record commands and outcomes in the active `process.md`, including failures
  and skipped checks.
- Before ending a task, make sure `todo.md` says what is done, what remains, and
  where the next agent should start.
