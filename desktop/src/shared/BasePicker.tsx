// Where an agent task starts. By default origin's copy of the default
// branch (`origin/main`), freshly fetched, so a task never starts from a
// local branch that's behind; a local branch only when it's picked here.

import type { RepoInfo } from "../types";
import { Branch } from "./Icons";

/** The base a new task starts from unless another is picked. */
export const defaultStart = (repo: RepoInfo) =>
  repo.default_start ?? repo.default_base ?? repo.branches[0] ?? "main";

export function BasePicker({ repo, value, onChange, title }: { repo: RepoInfo; value: string; onChange: (base: string) => void; title: string }) {
  const remote = repo.default_start && repo.default_start !== repo.default_base ? repo.default_start : null;
  return (
    <label className="chip-select-wrap" title={title}>
      <Branch size={12} />
      <select className="chip-select bare" value={value} onChange={(e) => onChange(e.target.value)}>
        {remote ? (
          <>
            <option value={remote}>{remote} (latest)</option>
            <optgroup label="Local branches">
              {repo.branches.map((b) => (
                <option key={b} value={b}>
                  {b} (local)
                </option>
              ))}
            </optgroup>
          </>
        ) : (
          repo.branches.map((b) => <option key={b}>{b}</option>)
        )}
      </select>
    </label>
  );
}
