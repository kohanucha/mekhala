# Issue tracker: GitHub (via GitHub MCP)

Issues and specs for this repo live as GitHub issues in `kohanucha/mekhala`.
Use the GitHub MCP tools for every operation. Never use the `gh` CLI or the
REST API with a token.

## Conventions

- **Create an issue**: `issue_write` with `method: "create"`, `title`, `body`.
- **Read an issue**: `issue_read` with `method: "get"`; comments with
  `method: "get_comments"`.
- **List issues**: `list_issues` with `state` and `labels` filters, and
  `fields` to drop bodies when only titles are needed. `search_issues` for
  text queries.
- **Make an issue a sub-issue of a parent**: create it with
  `parent_issue_number`, or attach an existing one with `sub_issue_write`
  (`method: "add"`, `sub_issue_id` = the child's database id from
  `issue_read` `get`, not its `#number`).
- **Comment on an issue**: `add_issue_comment`.
- **Apply / remove labels**: `issue_write` with `method: "update"` and the
  complete `labels` list; it replaces the set, so read the current labels
  first (`issue_read` `get_labels`).
- **Close**: `add_issue_comment` with the reason, then `issue_write`
  `method: "update"`, `state: "closed"`, `state_reason: "completed"` or
  `"not_planned"`.

## Pull requests as a triage surface

**PRs as a request surface: no.** _(Set to `yes` if this repo treats
external PRs as feature requests; `/triage` reads this flag.)_

When set to `yes`: read a PR with `pull_request_read` (`get`, `get_diff`,
`get_comments`), list with `list_pull_requests`, and comment, label or close
through `add_issue_comment` and `issue_write` with the PR's number. GitHub
shares one number space across issues and PRs: try `pull_request_read`
first and fall back to `issue_read`.

## When a skill says "publish to the issue tracker"

Create a GitHub issue.

## When a skill says "fetch the relevant ticket"

Read it as in **Read an issue** above.

## Wayfinding operations

Used by `/wayfinder`. The **map** is a single issue with **child** issues as
tickets.

- **Map**: one issue labelled `wayfinder:map`, holding the Notes /
  Decisions-so-far / Fog body.
- **Child ticket**: a sub-issue of the map (see above), labelled
  `wayfinder:<type>` (`research`/`prototype`/`grilling`/`task`). Once
  claimed, it is assigned to the driving dev.
- **Blocking**: the MCP tools cannot set native issue dependencies, so put
  `Blocked by: #<n>, #<n>` at the top of the child body. A ticket is
  unblocked when every issue on that line is closed.
- **Frontier query**: list the map's open children (`issue_read`
  `get_sub_issues`), drop any with an open blocker or an assignee; first in
  map order wins.
- **Claim**: `issue_write` `update` with `assignees`, the session's first
  write.
- **Resolve**: `add_issue_comment` with the answer, close the issue, then
  append a context pointer (gist + link) to the map's Decisions-so-far.
