// CI status, the same everywhere: PR checks, Actions runs, jobs and steps.

import type { CiStatus } from "../types";
import type { Status } from "./StatusPill";
import { CircleCheck, CircleDashed, CircleSlash, CircleX } from "./Icons";

export function ciStatus(status: CiStatus, elapsed?: string): Status {
  switch (status) {
    case "running":
      return { label: elapsed ? `Running ${elapsed}` : "Running", tone: "sky", live: true };
    case "queued":
      return { label: "Queued", tone: "amber", live: false };
    case "success":
      return { label: "Passed", tone: "emerald", live: false };
    case "failure":
      return { label: "Failed", tone: "red", live: false };
    case "cancelled":
      return { label: "Cancelled", tone: "muted", live: false };
    default:
      return { label: "Skipped", tone: "muted", live: false };
  }
}

export function CiIcon({ status, size = 14 }: { status: CiStatus; size?: number }) {
  switch (status) {
    case "success":
      return <CircleCheck size={size} className="tone-emerald" />;
    case "failure":
      return <CircleX size={size} className="tone-red" />;
    case "running":
      return <CircleDashed size={size} className="tone-sky spin" />;
    case "queued":
      return <CircleDashed size={size} className="tone-amber" />;
    default:
      return <CircleSlash size={size} className="muted-icon" />;
  }
}

/** 42s, 3m, 1h 05m: compact elapsed time between two unix times. */
export function elapsed(from: number, to: number): string {
  const s = Math.max(0, Math.floor(to - from));
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ${String(s % 60).padStart(2, "0")}s`;
  return `${Math.floor(m / 60)}h ${String(m % 60).padStart(2, "0")}m`;
}
