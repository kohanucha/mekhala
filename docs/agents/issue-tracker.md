# Issue tracker — GitHub (MCP)

This repo uses GitHub Issues to track work, managed via GitHub MCP tools.

## Tools

Agents should use the following tools to interact with the issue tracker:

- **Creating Issues:** Use `mcp_github_issue_write` with `method: 'create'`.
- **Listing/Reading Issues:** Use `mcp_github_list_issues` or `mcp_github_issue_read`.
- **Triaging/Updating:** Use `mcp_github_issue_write` with `method: 'update'`.
- **Searching:** Use `mcp_github_search_issues`.

## Repository Information

- **Owner:** kohanucha
- **Repo:** mekhala

## Workflows

### `to-issues`

When an agent needs to record a task, bug, or idea, use `mcp_github_issue_write`:

```json
{
  "method": "create",
  "owner": "kohanucha",
  "repo": "mekhala",
  "title": "...",
  "body": "...",
  "labels": ["ready-for-agent"]
}
```

### `triage`

When an agent triages incoming issues, list them using `mcp_github_list_issues` (filtering by labels if necessary). To update an issue's labels:

```json
{
  "method": "update",
  "owner": "kohanucha",
  "repo": "mekhala",
  "issue_number": <number>,
  "labels": ["ready-for-agent"]
}
```
