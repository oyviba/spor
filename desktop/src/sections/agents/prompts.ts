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
    `make sure it still builds and the tests pass, and commit.`
  );
}
