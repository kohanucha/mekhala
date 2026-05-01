# Issue tracker — GitHub

This repo uses GitHub Issues to track work.

## Commands

### `to-issues`

When an agent needs to record a task, bug, or idea, it uses the `gh` CLI:

```bash
gh issue create --title "..." --body "..." --label "ready-for-agent"
```

### `triage`

When an agent triages incoming issues, it reads them via:

```bash
gh issue list --label "needs-triage"
```

Then it applies labels to move them through the state machine:

```bash
gh issue edit <number> --add-label "ready-for-agent" --remove-label "needs-triage"
```
