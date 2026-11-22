// A new GitHub issue (#148): what the New issue dialog starts from, either
// blank or prefilled from something an agent's task turned up (a debrief's
// risk or open question, a transcript message, a failed check), with a
// link back to the task.

/** What the dialog opens with. */
export interface IssueDraft {
  title: string;
  body: string;
  labels: string[];
  /** Logins, or `@me`; assigned to you, it's a Planned work item. */
  assignees: string[];
}

export const blankIssue = (): IssueDraft => ({ title: "", body: "", labels: [], assignees: ["@me"] });

/** Where a filed issue came from. */
export interface IssueSource {
  /** The agent task, its id as its commits carry it (`Spor-Task: <id>`). */
  task?: { id: string; branch: string };
  /** "a risk in the debrief", "an open question in the debrief", … */
  what: string;
  text: string;
  /** In place of one made from the text's first line. */
  title?: string;
  /** A page to link to (a run on GitHub). */
  url?: string;
  /** Labels it starts with. */
  labels?: string[];
}

/** The longest a title made from the text gets. */
const TITLE = 80;

/** A title from the text's first line, cut at a word if it's long. */
export function titleOf(text: string): string {
  const line = (text.split("\n").find((l) => l.trim()) ?? "")
    .trim()
    // Markdown that means nothing in a title.
    .replace(/^#+\s*|^[-*]\s+|^>\s*/g, "")
    .replace(/\*\*|__|`/g, "")
    .trim();
  if (line.length <= TITLE) return line;
  const cut = line.slice(0, TITLE);
  const at = cut.lastIndexOf(" ");
  return `${(at > TITLE / 2 ? cut.slice(0, at) : cut).replace(/[\s,.;:]+$/, "")}…`;
}

/** The text quoted, with where it came from. */
export function issueFrom(src: IssueSource): IssueDraft {
  const quoted = src.text
    .trim()
    .split("\n")
    .map((l) => (l ? `> ${l}` : ">"))
    .join("\n");
  const t = src.task;
  const from = t ? `${src.what} of agent task \`${t.id}\` (branch \`${t.branch}\`)` : src.what;
  const link = src.url ? `: ${src.url}` : ".";
  const trailer = t ? `\n\nSpor-Task: ${t.id}` : "";
  const body = `${quoted}\n\n---\nFiled from ${from}${link}${trailer}`;
  return { title: src.title?.trim() || titleOf(src.text), body, labels: src.labels ?? [], assignees: ["@me"] };
}

/** "bug, ui ,, docs" → ["bug", "ui", "docs"]. */
export function splitNames(text: string): string[] {
  return [...new Set(text.split(",").map((s) => s.trim()).filter(Boolean))];
}
