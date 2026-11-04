// A unified diff (from `git diff`) as files, hunks and numbered lines.

export interface DiffLine {
  kind: "add" | "del" | "ctx" | "hunk";
  text: string;
  oldNo: number | null;
  newNo: number | null;
}

export interface DiffFile {
  path: string;
  oldPath: string | null;
  status: "added" | "deleted" | "renamed" | "modified" | "binary";
  additions: number;
  deletions: number;
  lines: DiffLine[];
}

export function parsePatch(patch: string): DiffFile[] {
  const files: DiffFile[] = [];
  let file: DiffFile | null = null;
  let oldNo = 0;
  let newNo = 0;
  for (const raw of patch.split("\n")) {
    if (raw.startsWith("diff --git ")) {
      const m = /^diff --git a\/(.+) b\/(.+)$/.exec(raw);
      file = {
        path: m ? m[2] : raw.slice(11),
        oldPath: null,
        status: "modified",
        additions: 0,
        deletions: 0,
        lines: [],
      };
      files.push(file);
      continue;
    }
    if (!file) continue;
    if (raw.startsWith("@@")) {
      const m = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@(.*)$/.exec(raw);
      oldNo = m ? Number(m[1]) : 0;
      newNo = m ? Number(m[2]) : 0;
      file.lines.push({ kind: "hunk", text: m ? m[3].trim() : raw, oldNo: null, newNo: null });
    } else if (file.lines.length === 0) {
      // Header lines before the first hunk.
      if (raw.startsWith("new file")) file.status = "added";
      else if (raw.startsWith("deleted file")) file.status = "deleted";
      else if (raw.startsWith("rename from ")) {
        file.status = "renamed";
        file.oldPath = raw.slice(12);
      } else if (raw.startsWith("Binary files")) file.status = "binary";
    } else if (raw.startsWith("+")) {
      file.additions++;
      file.lines.push({ kind: "add", text: raw.slice(1), oldNo: null, newNo: newNo++ });
    } else if (raw.startsWith("-")) {
      file.deletions++;
      file.lines.push({ kind: "del", text: raw.slice(1), oldNo: oldNo++, newNo: null });
    } else if (raw.startsWith(" ")) {
      file.lines.push({ kind: "ctx", text: raw.slice(1), oldNo: oldNo++, newNo: newNo++ });
    }
  }
  return files;
}
