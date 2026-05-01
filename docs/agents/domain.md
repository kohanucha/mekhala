# Domain documentation

Engineering skills (`improve-codebase-architecture`, `diagnose`, `tdd`) use these files to understand the project's domain and past decisions.

## Layout

This repo uses a **single-context** layout:

- `CONTEXT.md` (root) — The "living" definition of the project's domain language, core concepts, and high-level architecture.
- `docs/adr/` — Architectural Decision Records.

## Consumer Rules

- **Read first.** Before suggesting changes, read `CONTEXT.md` to ensure terminology matches.
- **Consult ADRs.** When investigating "why" something is the way it is, check `docs/adr/` before assuming it's a bug.
- **Update.** If a plan changes the domain model or architecture, the agent should update `CONTEXT.md` or propose a new ADR as part of the PR.
