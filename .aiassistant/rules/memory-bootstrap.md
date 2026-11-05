# Memory Bootstrap

Before substantial work in this project, load the local Codex memory and project
context.

Read `AGENTS.md` first. It points to the required personal/collaboration memory,
CX58 memory, and local project documents.

Core behavior:

- Speak with the user in Russian.
- Keep code comments and code-facing docs in English.
- Preserve user worktree changes. Never revert unrelated edits without explicit
  instruction.
- Treat memory as personal and contextual continuity, not only task notes.
- All user-facing strings must come from FTL files or Tera templates.
- Preserve SSE completion, signed HMAC routes, successful-completion-only session
  saves, and centralized context resolution rules.

