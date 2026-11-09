# Concepts

The words spor uses for the work you do in it, one meaning each (#76). Buttons,
toasts, palette commands and undo labels use these words, and `spor::work`
uses them in code. A new feature that seems to need a new word should first
check whether one of these says it.

## Work item

A **work item** is a branch together with whatever goes with it:

- the **issue** it came from
- a **task**: its own folder beside the repository (`<repo>.spor/<name>`),
  usually with an **agent** working in it
- its **pull request**
- its **checks**: the pull request's, or the Actions runs on the branch

Any of these can be missing. Your own branch in the main checkout is a work
item with no task. A teammate's pull request is one with no local branch. An
issue assigned to you with no branch yet is a *Planned* work item.

A work item is identified by its branch. A task, a local branch, a pull
request whose head is that branch, and the runs on it are the same item,
wherever you click them from.

**Task** always means a work item with its own folder, whoever works in it.
It isn't another word for "agent": a task can have no agent ("none" in
settings), and an agent can work without a task (a cloud agent's branch).

## Stages

Every work item is at one stage:

| Stage | Means |
| --- | --- |
| **Planned** | An issue assigned to you, no branch yet |
| **In progress** | A branch with work on it, or a draft pull request |
| **In review** | A pull request is open, or a task finished and you haven't looked yet |
| **Landed** | In its base: the task was landed, or its pull request merged |
| **Closed** | Its pull request was closed without landing |

## Waiting on

Until it's landed or closed, a work item says who has to do something next:

- **You**, and why: the agent asked something, a run finished that you
  haven't seen, your review is requested, checks failed, a reviewer asked for
  changes, or it's approved and green and only needs landing
- **An agent**: it's running
- **A teammate**: the pull request's author, or the reviewer you're waiting for
- **CI**: checks are running

## Verbs

One verb for each thing you do to a work item:

| Verb | Does |
| --- | --- |
| **Start** | Make the branch from a base, and with an agent, its task folder and the agent's first run |
| **Publish** | Push the branch for the first time; on a task, push and open its pull request in one go |
| **Push** | Send new commits to a branch that's already published |
| **Land** | Put the work into its base. Locally (*Land on main*: one squashed commit, or a merge commit), or by merging its pull request on GitHub (*Land #12*: squash, merge commit or rebase) |
| **Hand to an agent** | Give one of your branches its own folder and an agent. From History's uncommitted changes, your uncommitted work goes along and your checkout switches to the base (⌘Z brings it all back). *Start an agent on this* on a branch or pull request does the same |
| **Take over** | Open a task's folder to work in it yourself |
| **Discard** | Remove a task's folder, and its branch unless the branch was yours before (⌘Z brings them back) |
| **Clean up** | After a pull request was merged or closed on GitHub: remove the task's folder and branch, and mark it landed or closed |
| **Send to agent** | Give the agent a follow-up: review comments, failing checks, your own words |

**Merge** is kept for `git merge` of one branch into another, in History
and in the operation banner ("Merge stopped on conflicts"). Landing a pull
request is *Land*, even though GitHub calls it merging: GitHub's own words
appear only in the hints that say what happens there ("Squash and merge on
GitHub").

## Where things live

- **Inbox**: what's waiting on you, across every repository
- **Work**: every work item in the repository, and the work page for each:
  the same page for a task, a pull request and your own branch, with the
  stage track in its header and the tabs that have something in them
  (Agent, Discussion, Changes, Checks)
- **History**: your checkout and the commit graph. Its *Uncommitted
  changes* row shows the checked-out branch's work header and card, with
  *Hand to an agent*, beside the files to stage and commit
- **Issues**, **Pull requests** and **Actions**: browsers for what's on
  GitHub. Anything in them that belongs to a work item shows the same work
  card (issue → branch · folder → pull request → checks), with the same
  next step
