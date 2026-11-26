// Grouping the sidebar's branches by the part of their name before the
// first "/": chore/a and chore/b under "chore", feat/x under "feat".

export type BranchEntry<T> = { item: T; label: string };

/** A branch on its own, or the branches sharing a prefix. */
export type BranchGroup<T> =
  | { kind: "branch"; entry: BranchEntry<T> }
  | { kind: "group"; prefix: string; entries: BranchEntry<T>[] };

/**
 * Group `items` by the first "/"-separated segment of `name(item)`. A remote
 * branch's name starts with its remote (origin/feat/x); with `skip` 1 that
 * groups under "origin/feat" rather than every branch under "origin".
 * Each group sits where its first branch did, so the list keeps its order,
 * and its branches are labelled with the rest of their name.
 */
export function groupBranches<T>(items: T[], name: (item: T) => string, skip = 0): BranchGroup<T>[] {
  const out: BranchGroup<T>[] = [];
  const groups = new Map<string, BranchEntry<T>[]>();
  for (const item of items) {
    const parts = name(item).split("/");
    if (parts.length < skip + 2 || !parts[skip]) {
      out.push({ kind: "branch", entry: { item, label: name(item) } });
      continue;
    }
    const prefix = parts.slice(0, skip + 1).join("/");
    const entry = { item, label: parts.slice(skip + 1).join("/") };
    const group = groups.get(prefix);
    if (group) {
      group.push(entry);
    } else {
      const entries = [entry];
      groups.set(prefix, entries);
      out.push({ kind: "group", prefix, entries });
    }
  }
  return out;
}
