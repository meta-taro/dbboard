# `.claude/issues/`

Lightweight task tracker used during early development before tasks
graduate to GitHub Issues.

## Convention

- One file per task: `NNNN-kebab-slug.md`.
- Number monotonically. Do not reuse numbers.
- Status header at the top: `open`, `in-progress`, `blocked`, `done`.
- When a task ships, set status to `done` and link the commit / PR.
- When a task graduates to GitHub Issues, set status to `done` and link
  the GitHub issue URL.

## Template

```markdown
# NNNN: <title>

- **Status**: open
- **Phase**: 1
- **Opened**: YYYY-MM-DD

## Context

Why this task exists.

## Acceptance

- [ ] Concrete, testable criterion
- [ ] Another one

## Notes

Anything else: links, sketches, gotchas.
```
