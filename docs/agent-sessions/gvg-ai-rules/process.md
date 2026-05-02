# GVG AI Rules Session Process

Hub links:

- Cross-agent hub: `../../../AGENTS.md`
- Cursor rule: `../../../.cursor/rules/gvg-research-tools.mdc`
- TODO: `todo.md`

## User Request

The user requested English AI-only development rules for the current GVG
research tools project. The rules must be readable by Cursor and cross-agent
tools such as Claude, OpenAI/Codex, and similar agents. Every AI task must first
review relevant Markdown research notes under `docs/`, then create task-specific
`todo.md` and `process.md` files for handoff. The user selected:

- Root `AGENTS.md` plus `.cursor/rules/gvg-research-tools.mdc`.
- Topic session logs under `docs/agent-sessions/<topic>/`.
- Mutual linking between rules, skills, and related AI guidance.
- Direct implementation after public GitHub/web research, without another
  confirmation gate.

## Project Context Read

Files read or inspected:

- `.gitignore`
- `.cursor/plans/gvg_modding_tool_9b58cfe1.plan.md`
- `docs/MOD_WORKFLOW.md`
- `docs/PMF2_TODO.md`
- `docs/RUST_DAE_PMF2_COMMANDS.md`
- `docs/superpowers/specs/2026-04-29-gvg-modding-tool-v2-design.md`

Important project facts captured in the new guidance:

- This is a research and tooling project for **Mobile Suit Gundam: Gundam vs.
  Gundam Next Plus** (`ガンダムVS.ガンダムNEXT PLUS`).
- Core research areas include `Z_DATA.BIN`, AFS, PZZ, PMF2, GIM, DAE/FBX,
  PPSSPP runtime behavior, and reverse-engineering notes.
- `docs/` is the first source of truth for future AI agents.
- Current Rust tooling lives in the root Cargo project (`Cargo.toml`, `src/`, `tests/`).
- Existing docs emphasize preserving PMF2 template bytes, being careful with
  matrix noise, GE commands, PZZ tails, AFS layout, and runtime draw masks.

## Public Guidance Researched

Searches performed:

- Web search: `GitHub AGENTS.md agent instructions best practices repository rules AI coding agent`
- Web search: `Cursor rules .cursor/rules mdc best practices alwaysApply globs AGENTS.md`
- Web search: `AI coding agent skills rules repository documentation best practices GitHub`
- GitHub CLI search: `gh search repos "agents.md"`
- GitHub CLI search: `gh search repos "agent-skills"`

Relevant public references found:

- `agentsmd/agents.md`: `AGENTS.md` as a simple open format and "README for
  agents."
- GitHub Copilot docs: repository agent instructions should include project
  layout, exact commands, validation, and boundaries.
- Cursor rules docs: `.cursor/rules/*.mdc` supports frontmatter, `alwaysApply`,
  descriptions, globs, and version-controlled project rules.
- Agent skills guidance from GitHub/Claude ecosystem: skills should use
  trigger-oriented descriptions, focused `SKILL.md` files, progressive
  disclosure, linked references, and explicit verification steps.

Design choices based on research:

- Use `AGENTS.md` as the hub because it is broadly recognized by multiple agent
  ecosystems.
- Use a short always-applied Cursor `.mdc` rule to ensure the startup protocol is
  active in Cursor.
- Keep detailed project context in `AGENTS.md`; keep the Cursor rule concise and
  linked.
- Require all future project-owned rules and skills to link back to `AGENTS.md`
  and be listed in its link map.

## Files Created

- `AGENTS.md`
- `.cursor/rules/gvg-research-tools.mdc`
- `docs/agent-sessions/gvg-ai-rules/todo.md`
- `docs/agent-sessions/gvg-ai-rules/process.md`

## Verification

- Read back all four created files and checked that reciprocal links are present.
- Ran `ReadLints` on the created Markdown/MDC files: no linter errors found.
- Confirmed created file existence and line counts:
  - `AGENTS.md`: 108 lines
  - `.cursor/rules/gvg-research-tools.mdc`: 57 lines
  - `docs/agent-sessions/gvg-ai-rules/todo.md`: 31 lines before final TODO update
  - `docs/agent-sessions/gvg-ai-rules/process.md`: 91 lines before this section
- Checked git status for touched paths. New files are untracked. Existing
  `.cursor/rules/cursor-rules.mdc` remains deleted from earlier workspace state
  and was not restored.

## Notes For Next Agent

- Do not assume `.cursor/rules/cursor-rules.mdc` still exists; it appeared as
  deleted in git status before this session.
- Keep future rule/skill additions concise and linked from `AGENTS.md`.
- If adding Claude/Codex/OpenAI-specific files, avoid copying the whole hub;
  point those files back to `AGENTS.md` and add tool-specific deltas only.
