---
name: sprint-scope-analyst
description: Explains scope change in a Jira sprint - what was added or removed after the sprint started, by whom, how many story points moved and the resulting scope creep. Use it when someone asks about sprint scope creep, unplanned work, or changes to a sprint's commitment.
allowed-tools: get-sprint-scope
---

# Sprint scope analyst

Use this skill when a person asks how a sprint's scope changed after it started: scope creep,
work added mid-sprint, work pulled out, who changed the sprint, or how big the commitment was.

## Calling `get-sprint-scope`

Call `get-sprint-scope` with one input:

- `sprintId` (string, required): the numeric id of the Jira sprint, for example `"41"`. It is the
  number in a sprint URL or in the board's sprint picker, not the sprint's name. If the person only
  gives a name, ask them for the sprint id (or the sprint link) before calling.

Call it once per sprint. The answer is computed for the person asking, so do not reuse one
person's result for someone else.

## Reading the result

A successful result is a JSON object:

- `sprintName`: the sprint's name.
- `committed`: story points of the issues that were in the sprint when it started.
- `added`: story points of issues in the sprint now that were not in it at the start.
- `removed`: story points of issues that were in the sprint after it started and are not in it now.
- `creepPercent`: `100 × added / committed`, one decimal; `null` when nothing was committed.
- `changes`: each change the person can see, oldest first: `issueKey`, `kind` (`added` or
  `removed`), `points` (the issue's current estimate), `at` (UTC time of the change) and `by`
  (who made it).
- `hiddenChanges`: how many changes are on issues this person cannot browse. Say that they exist
  and how many, never guess what they are.

The totals are team totals and include hidden issues; the list of changes does not. Report the
numbers exactly as returned, with `%` after `creepPercent`. When `creepPercent` is `null`, say
that creep cannot be computed because the sprint started with no committed points.

## Errors

If the result has an `error` field instead, nothing was computed. Tell the person the message in
plain words. When it says the sprint was not found or the id is missing, ask for the correct
sprint id; when it says the sprint has not started, explain there is no scope change yet. Do not
retry with a guessed id.
