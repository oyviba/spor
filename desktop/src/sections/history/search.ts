// What the History search box means, apart from the screen it's on.

/** The words of a query to mark in the results: none for `path:` and
 * `code:` queries, where the words aren't in the commit lines. */
export function searchTerms(query: string): string[] {
  const q = query.trim();
  if (!q || /^(path|code):/.test(q)) return [];
  return [...new Set(q.toLowerCase().split(/\s+/))];
}

/** `text` cut into pieces, `match` for the parts that are one of `terms`
 * (ignoring case). */
export function highlight(text: string, terms: string[]): { text: string; match: boolean }[] {
  if (terms.length === 0) return [{ text, match: false }];
  const pattern = new RegExp(
    `(${terms
      .map((t) => t.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"))
      .sort((a, b) => b.length - a.length)
      .join("|")})`,
    "gi",
  );
  return text
    .split(pattern)
    .map((part, i) => ({ text: part, match: i % 2 === 1 }))
    .filter((part) => part.text !== "");
}

/** Where to go from `at` (an index in the rows, -1 for nowhere): the
 * next (or, with `step` -1, previous) row that's a hit, wrapping round. */
export function stepHit(hitRows: number[], at: number, step: 1 | -1): number | null {
  if (hitRows.length === 0) return null;
  if (step === 1) return hitRows.find((i) => i > at) ?? hitRows[0];
  return [...hitRows].reverse().find((i) => i < at) ?? hitRows[hitRows.length - 1];
}
