# Journeys

Five things people come to spor to do, step by step (#75). They're the
yardstick for #74: a change that makes spor more cohesive makes one of these
shorter, and one that adds a section to visit makes one longer. The words are
[concepts.md](concepts.md)'s.

Each journey is written twice:

- **Before**: how it went before work items (#74), when the rail was Changes,
  History, Issues, Pull requests, Actions and Agents, each on its own
- **Target**: how it should go, with a **budget**: how many times you change
  section (the rail's current item) from where you start

The end-to-end suite that counted section changes against these budgets has
been removed; the counts below are as of its last run. A **gap** is a step that
needs work that hasn't landed yet.

| Journey | Before | Target | Now | Gaps |
| --- | --- | --- | --- | --- |
| 1. Morning triage | 4+ per repository | 1 | 1 | |
| 2. Issue to landed pull request, with an agent | 5 | 1 | 1 | |
| 3. Review a teammate's pull request | 2 | 1 | 1 | the failing step's log on the page |
| 4. Your own branch | 5 | 1 | 1 so far | opening the pull request from spor, and on from there |
| 5. Three agents at once | 0, without seeing collisions | 0 | 0 so far | collisions between tasks, and landing in order |

## 1. Morning triage

Find what's waiting on you across every repository (agent questions, tasks
ready for review, review requests, red CI on your branches) and deal with the
first one.

**Before** (4 or more changes per repository): open each repository in turn.
In each, Agents for questions and finished runs; Pull requests, *Review*, for
review requests; Actions for failed runs, reading branch names to find yours.
Then back to whichever needed you.

**Target** (budget 1):

1. **Inbox** lists what waits on you in every repository of the workspace,
   grouped: *Waiting on an answer*, *Failed runs*, *Ready for review*, *Review
   requested*
2. Open the first: it opens where it's dealt with (a failed run opens in
   **Actions**, with the work card naming its branch) → *1 change*
3. Deal with it: *Re-run failed*

## 2. From an issue to a landed pull request, with an agent

Pick an issue, start an agent on it, answer its question, review its changes
with line comments, send feedback, publish, watch checks, land, clean up.

**Before** (5 changes): Issues, *Start agent task* → Agents to watch it, answer
it, review the diff and comment → *Open pull request* → Pull requests to see it
→ Actions for its checks → Pull requests to merge → Agents to clean up the task.

**Target** (budget 1):

1. **Issues**: pick one assigned to you, *Start with agent*
2. Its task opens on the work page in **Work** → *1 change*
3. *Agent*: the agent asks before running the tests; *Allow*
4. *Changes*: comment on a line; send it, and the agent's next run takes it
5. *Publish*: the branch is pushed and its pull request opens; the task's page
   gains *Discussion* and *Checks*
6. *Checks*: the pull request's checks, on the same page
7. *Land #n*: squash and merge on GitHub, and the task cleans up (its folder
   and branch go; ⌘Z brings them back)

## 3. Review a teammate's pull request

Read the conversation and the diff, see why a check failed, leave line
comments, approve.

**Before** (2 changes): Pull requests, *Review*: read it, Files for the diff and
line comments, Checks → Actions for the failing job's log → back to Pull
requests to approve.

**Target** (budget 1):

1. **Inbox**: the review request opens on the work page in **Pull requests** →
   *1 change*
2. *Discussion*: the description and reviews
3. *Changes*: the diff; comment on a line
4. *Checks*: the failing check, with its failing step's log folded open, as
   Actions shows it — **gap**: the tab lists the checks and links each job to
   Actions, but doesn't show the log itself (#80)
5. *Approve*: the line comments go in the review's body

## 4. Your own branch

Branch from main, stage parts of files, commit, publish, open the pull request,
fix a failed check, land it.

**Before** (5 changes): History (or the palette) for a new branch → Changes to
stage lines and commit, *Publish* → the browser for GitHub's new pull request
page → Pull requests to find it → Actions for the failure → Changes to fix and
push → Pull requests to merge.

**Target** (budget 1):

1. **Work**: *New… → Branch…* from main; it's checked out
2. **History** → *Uncommitted changes* → *1 change*: the branch's work header (*In progress*); pick
   lines in a diff, *Stage lines*, commit with ⌘↩
3. *Publish*: the branch is pushed
4. Its pull request opens from here, through `gh` — **gap**: your own branch's
   *Publish* only pushes, and *Open a pull request* goes to GitHub's page
   (#82's follow-up: Publish on your own branch should open it, as a task's does)
5. A failed check shows in the work card and the header; fix, commit, *Push*
6. *Land #n* from the work card

## 5. Three agents at once

Start three tasks, see which collide, land them in order.

**Before** (0 changes, but no collisions): Agents, three tasks; each one's card
says whether it conflicts with its base, not with the others. Landing one and
then finding the next conflicts is how you'd learn.

**Target** (budget 0):

1. **Work**: *New… → Agent task*, ⌘↩ three times: each starts and the form stays
   for the next
2. All three are listed, working, then finished
3. Work says which of them change the same files (here, all three change
   greet.py) — **gap**: nothing compares tasks with each other yet, only each
   with its base
4. Land them in an order Work suggests, each rebased on the last, asking the
   agent to resolve what collides
