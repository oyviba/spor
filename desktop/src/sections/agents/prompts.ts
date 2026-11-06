// What spor asks an agent to do, beyond the task itself.

/** What to ask the agent when its branch conflicts with `base`. Once it's a
 * pull request the branch has been pushed, and pushes aren't forced, so it
 * merges the base in rather than rebasing. The remote's copy of the base is
 * the one GitHub merges into, and the background fetch keeps it current. */
export function catchUpPrompt(base: string, files: string[], published: boolean): string {
  const where = files.length ? ` in ${files.join(", ")}` : "";
  const how = published
    ? `Merge origin/${base} into this branch (git merge origin/${base}; don't rebase, the branch is already pushed)`
    : `Rebase onto ${base} (git rebase ${base})`;
  return (
    `${base} has moved on and this branch now conflicts with it${where}. ` +
    `${how}, resolve the conflicts keeping both sides' intent, ` +
    `keep everything ${base} gained (don't write files back from an older copy), ` +
    `make sure it still builds and the tests pass, and commit.`
  );
}

/** What to ask the agent when landing its branch would take back work that
 * reached `base` after it started (`TaskDetail.undoes`). */
export function restorePrompt(base: string, undoes: { path: string; lines: number }[]): string {
  const files = undoes.map((u) => `${u.path} (${u.lines} ${u.lines === 1 ? "line" : "lines"})`).join(", ");
  return (
    `Landing this branch would undo work that reached ${base} after you started, in ${files}: ` +
    `lines ${base} has that this branch removes, probably by writing a file back from an older copy. ` +
    `Compare with ${base} (git diff ${base} -- <file>), put back what ${base} added unless this task ` +
    `really means to remove it, keep your own changes on top, make sure it still builds and the tests pass, and commit.`
  );
}
