# Issue tracker: GitHub

Issues and specs live in GitHub Issues. Use the `gh` CLI.
Resolve the repository from `git remote -v`.

## Operations

- Create: `gh issue create --title "..." --body-file <path>`
- Read: `gh issue view <number> --comments`; fetch labels when triaging.
- List: `gh issue list --state open --json number,title,body,labels,comments`
- Comment: `gh issue comment <number> --body-file <path>`
- Add or remove labels: `gh issue edit <number> --add-label "..."` or `--remove-label "..."`
- Close: `gh issue close <number> --comment "..."`

For multiline bodies, write the exact text to a temporary file and use `--body-file`.
“Publish to the issue tracker” means create an issue.
“Fetch the relevant ticket” means read the issue and its comments.

## Pull requests as a triage surface

**PRs as a request surface: no.**

## Wayfinding

Use a `wayfinder:map` issue for Notes, Decisions-so-far, and Fog.
Link child tickets as sub-issues; if unavailable, use a task list
in the map and `Part of #<map>` in each child.
Label children `wayfinder:<type>`: research, prototype, grilling, or task.

Use native issue dependencies for blockers when available;
otherwise record `Blocked by: #<number>` in the child.
A ticket is unblocked when all blockers are closed.
Choose the first open, unblocked, unassigned child in map order.
Claim it with `gh issue edit <number> --add-assignee @me`.
On resolution, comment with the result, close the ticket, and add
a summary and link to the map's Decisions-so-far.
