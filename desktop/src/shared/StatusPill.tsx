export type Tone = "sky" | "emerald" | "red" | "amber" | "violet" | "muted";

/** One status for a row or a page: what it is, in one word or two. */
export interface Status {
  label: string;
  tone: Tone;
  /** Still going: the dot pulses (gently, and not with reduced motion). */
  live: boolean;
}

export function StatusPill({ status }: { status: Status | null }) {
  if (!status) return null;
  return (
    <span className={`pill tone-${status.tone}`}>
      <span className={`dot${status.live ? " pulse" : ""}`} />
      <span className="num">{status.label}</span>
    </span>
  );
}
