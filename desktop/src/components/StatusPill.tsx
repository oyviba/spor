import type { Status } from "../status";

export function StatusPill({ status }: { status: Status | null }) {
  if (!status) return null;
  return (
    <span className={`pill tone-${status.tone}`}>
      <span className={`dot${status.live ? " pulse" : ""}`} />
      <span className="num">{status.label}</span>
    </span>
  );
}
