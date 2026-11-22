import type { RefObject } from "react";
import { Button } from "../../shared/Button";
import { Branch, ChevronDown, Search, X } from "../../shared/Icons";

interface Props {
  query: string;
  onQuery: (q: string) => void;
  inputRef: RefObject<HTMLInputElement | null>;
  /** The hits there are, or null while there's no search. */
  hits: { total: number; loaded: number; at: number | null } | null;
  searching: boolean;
  /** Show only the hits, as a list. */
  matchesOnly: boolean;
  onMatchesOnly: (on: boolean) => void;
  /** ↓ (+1) or ↑ (-1) to the next hit. */
  onStep: (step: 1 | -1) => void;
  /** The commits the search found past what the graph has loaded. */
  onLoadMore: (() => void) | null;
  /** The default branch: one click to its tip, whatever the search. */
  defaultBranch: string | null;
  onDefault: () => void;
}

/** The search above the graph: what's typed, how many it found and which
 * one you're on, ↑/↓ between them, and the way back to the default branch. */
export function GraphSearch(p: Props) {
  const { hits } = p;
  return (
    <div className="graph-search">
      <div className="graph-search-box">
        <Search size={13} />
        <input
          ref={p.inputRef}
          value={p.query}
          spellCheck={false}
          placeholder="Search commits: words, a branch or tag, path:file, code:text"
          title={
            "Every word has to match the message, author, hash, or a branch or tag name.\n" +
            "path:src/main.rs — commits that touched a file\ncode:needle — commits that added or removed some text\n" +
            "Enter and ↓ go to the next match, Shift+Enter and ↑ the one before. ⌘F or / to search."
          }
          onChange={(e) => p.onQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              p.onStep(e.shiftKey ? -1 : 1);
            } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
              e.preventDefault();
              p.onStep(e.key === "ArrowDown" ? 1 : -1);
            } else if (e.key === "Escape") {
              if (p.query) p.onQuery("");
              else e.currentTarget.blur();
            }
          }}
        />
        {p.searching && <span className="muted small-text">Searching…</span>}
        {hits && !p.searching && (
          <span className="num muted small-text" aria-live="polite">
            {hits.loaded === 0
              ? "No matches"
              : hits.at != null
                ? `${hits.at + 1} of ${hits.loaded.toLocaleString()}`
                : `${hits.loaded.toLocaleString()} ${hits.loaded === 1 ? "match" : "matches"}`}
          </span>
        )}
        {hits && (
          <>
            <Button className="icon-button tiny" title="Previous match (Shift+Enter)" onClick={() => p.onStep(-1)} disabled={hits.loaded === 0}>
              <ChevronDown size={12} style={{ transform: "rotate(180deg)" }} />
            </Button>
            <Button className="icon-button tiny" title="Next match (Enter)" onClick={() => p.onStep(1)} disabled={hits.loaded === 0}>
              <ChevronDown size={12} />
            </Button>
          </>
        )}
        {p.query && (
          <Button className="icon-button tiny" title="Clear the search (Esc)" onClick={() => p.onQuery("")}>
            <X size={12} />
          </Button>
        )}
      </div>
      {hits && (
        <button
          className="chip-toggle"
          aria-pressed={p.matchesOnly}
          onClick={() => p.onMatchesOnly(!p.matchesOnly)}
          title={p.matchesOnly ? "Showing only the matches — show them in the graph instead" : "Showing the whole graph — show only the matches"}
        >
          Only matches
        </button>
      )}
      {hits && p.onLoadMore && hits.total > hits.loaded && (
        <Button className="link small-text" onClick={p.onLoadMore} title="The rest are further back than the graph has loaded">
          {(hits.total - hits.loaded).toLocaleString()} older — load more
        </Button>
      )}
      <span className="flex-1" />
      {p.defaultBranch && (
        <Button className="ghost small" onClick={p.onDefault} title={`Go to ${p.defaultBranch}, the default branch`}>
          <Branch size={12} /> Go to {p.defaultBranch}
        </Button>
      )}
    </div>
  );
}
