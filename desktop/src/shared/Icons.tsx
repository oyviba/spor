// Small stroke icons (24-unit grid), drawn after Lucide (ISC, see
// LICENSE-LUCIDE beside this file), so the app has no icon dependency.

import type { SVGProps } from "react";

type P = SVGProps<SVGSVGElement> & { size?: number };

function Svg({ size = 14, children, ...rest }: P & { children: React.ReactNode }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
      {...rest}
    >
      {children}
    </svg>
  );
}

export const Plus = (p: P) => <Svg {...p}><path d="M12 5v14M5 12h14" /></Svg>;
export const Chevron = (p: P) => <Svg {...p}><path d="m9 18 6-6-6-6" /></Svg>;
export const ChevronDown = (p: P) => <Svg {...p}><path d="m6 9 6 6 6-6" /></Svg>;
export const Check = (p: P) => <Svg {...p}><path d="M20 6 9 17l-5-5" /></Svg>;
export const X = (p: P) => <Svg {...p}><path d="M18 6 6 18M6 6l12 12" /></Svg>;
export const ArrowUp = (p: P) => <Svg {...p}><path d="m5 12 7-7 7 7M12 19V5" /></Svg>;
export const Square = (p: P) => <Svg {...p}><rect x="6" y="6" width="12" height="12" rx="2" fill="currentColor" stroke="none" /></Svg>;
export const Branch = (p: P) => (
  <Svg {...p}>
    <circle cx="6" cy="6" r="2.5" />
    <circle cx="6" cy="18" r="2.5" />
    <circle cx="18" cy="8" r="2.5" />
    <path d="M6 8.5v7M18 10.5c0 4-6 3-11 6" />
  </Svg>
);
export const Folder = (p: P) => (
  <Svg {...p}><path d="M4 20h16a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.7-.9l-.8-1.2A2 2 0 0 0 7.9 3H4a2 2 0 0 0-2 2v13c0 1.1.9 2 2 2Z" /></Svg>
);
export const Copy = (p: P) => (
  <Svg {...p}><rect x="8" y="8" width="13" height="13" rx="2" /><path d="M4 16V5a2 2 0 0 1 2-2h11" /></Svg>
);
export const Alert = (p: P) => (
  <Svg {...p}><circle cx="12" cy="12" r="9" /><path d="M12 8v4M12 16h.01" /></Svg>
);
export const Terminal = (p: P) => <Svg {...p}><path d="m4 17 6-6-6-6M12 19h8" /></Svg>;
export const Pencil = (p: P) => <Svg {...p}><path d="M12 20h9M16.4 3.6a2.1 2.1 0 1 1 3 3L7 19l-4 1 1-4Z" /></Svg>;
export const FileIcon = (p: P) => (
  <Svg {...p}><path d="M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9Z" /><path d="M14 3v6h6" /></Svg>
);
export const Search = (p: P) => <Svg {...p}><circle cx="11" cy="11" r="7" /><path d="m20 20-3.5-3.5" /></Svg>;
export const Globe = (p: P) => (
  <Svg {...p}><circle cx="12" cy="12" r="9" /><path d="M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18" /></Svg>
);
export const ListIcon = (p: P) => <Svg {...p}><path d="M8 6h13M8 12h13M8 18h13M3 6h.01M3 12h.01M3 18h.01" /></Svg>;
export const Bot = (p: P) => (
  <Svg {...p}><rect x="4" y="8" width="16" height="12" rx="3" /><path d="M12 4v4M9 13v1M15 13v1" /></Svg>
);
export const Merge = (p: P) => (
  <Svg {...p}><circle cx="18" cy="18" r="2.5" /><circle cx="6" cy="6" r="2.5" /><path d="M6 21V8.5M6 9a9 9 0 0 0 9.5 9" /></Svg>
);
export const PanelRight = (p: P) => (
  <Svg {...p}><rect x="3" y="3" width="18" height="18" rx="2" /><path d="M15 3v18" /></Svg>
);
export const Trash = (p: P) => (
  <Svg {...p}><path d="M3 6h18M8 6V4h8v2M19 6l-1 14H6L5 6" /></Svg>
);
export const MessageSquare = (p: P) => (
  <Svg {...p}><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2Z" /></Svg>
);
export const Clock = (p: P) => <Svg {...p}><circle cx="12" cy="12" r="9" /><path d="M12 7v5l3 2" /></Svg>;
export const Users = (p: P) => (
  <Svg {...p}><circle cx="9" cy="8" r="3.5" /><path d="M2 21a7 7 0 0 1 14 0M17 4a3.5 3.5 0 0 1 0 7M22 21a7 7 0 0 0-4-6.3" /></Svg>
);

export const History = (p: P) => (
  <Svg {...p}><path d="M3 12a9 9 0 1 0 3-6.7L3 8" /><path d="M3 3v5h5M12 7v5l4 2" /></Svg>
);
export const PullRequest = (p: P) => (
  <Svg {...p}><circle cx="6" cy="6" r="2.5" /><circle cx="6" cy="18" r="2.5" /><circle cx="18" cy="18" r="2.5" /><path d="M6 8.5v7M18 15.5V9a3 3 0 0 0-3-3h-4M13 3.5 10.5 6 13 8.5" /></Svg>
);
export const IssueIcon = (p: P) => (
  <Svg {...p}><circle cx="12" cy="12" r="9" /><circle cx="12" cy="12" r="1.5" fill="currentColor" /></Svg>
);
export const Play = (p: P) => (
  <Svg {...p}><circle cx="12" cy="12" r="9" /><path d="m10 8.5 5.5 3.5-5.5 3.5Z" /></Svg>
);
export const InboxIcon = (p: P) => (
  <Svg {...p}><path d="M22 12h-6l-2 3h-4l-2-3H2" /><path d="M5.45 5.11 2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11Z" /></Svg>
);
export const Refresh = (p: P) => (
  <Svg {...p}><path d="M21 12a9 9 0 0 1-15.5 6.3L3 16M3 12a9 9 0 0 1 15.5-6.3L21 8M21 3v5h-5M3 21v-5h5" /></Svg>
);
export const ArrowDown = (p: P) => <Svg {...p}><path d="m19 12-7 7-7-7M12 5v14" /></Svg>;
export const Undo = (p: P) => <Svg {...p}><path d="M9 14 4 9l5-5" /><path d="M4 9h10.5a5.5 5.5 0 0 1 0 11H11" /></Svg>;
export const Gear = (p: P) => (
  <Svg {...p}>
    <path d="M12.2 2h-.4a2 2 0 0 0-2 2v.2a2 2 0 0 1-1 1.7l-.4.3a2 2 0 0 1-2 0l-.2-.1a2 2 0 0 0-2.7.7l-.2.4a2 2 0 0 0 .7 2.7l.2.1a2 2 0 0 1 1 1.7v.5a2 2 0 0 1-1 1.7l-.2.1a2 2 0 0 0-.7 2.7l.2.4a2 2 0 0 0 2.7.7l.2-.1a2 2 0 0 1 2 0l.4.3a2 2 0 0 1 1 1.7v.2a2 2 0 0 0 2 2h.4a2 2 0 0 0 2-2v-.2a2 2 0 0 1 1-1.7l.4-.3a2 2 0 0 1 2 0l.2.1a2 2 0 0 0 2.7-.7l.2-.4a2 2 0 0 0-.7-2.7l-.2-.1a2 2 0 0 1-1-1.7v-.5a2 2 0 0 1 1-1.7l.2-.1a2 2 0 0 0 .7-2.7l-.2-.4a2 2 0 0 0-2.7-.7l-.2.1a2 2 0 0 1-2 0l-.4-.3a2 2 0 0 1-1-1.7V4a2 2 0 0 0-2-2Z" />
    <circle cx="12" cy="12" r="3" />
  </Svg>
);
export const ImageIcon = (p: P) => (
  <Svg {...p}><rect x="3" y="3" width="18" height="18" rx="2" /><circle cx="9" cy="9" r="2" /><path d="m21 15-3.1-3.1a2 2 0 0 0-2.8 0L6 21" /></Svg>
);
export const Minus = (p: P) => <Svg {...p}><path d="M5 12h14" /></Svg>;
export const CornerDownRight = (p: P) => <Svg {...p}><path d="m15 10 5 5-5 5" /><path d="M4 4v7a4 4 0 0 0 4 4h12" /></Svg>;
export const Tag = (p: P) => (
  <Svg {...p}><path d="M12.6 2.6A2 2 0 0 0 11.2 2H4a2 2 0 0 0-2 2v7.2a2 2 0 0 0 .6 1.4l8.7 8.7a2.4 2.4 0 0 0 3.4 0l6.6-6.6a2.4 2.4 0 0 0 0-3.4Z" /><circle cx="7.5" cy="7.5" r="1" fill="currentColor" /></Svg>
);
export const Commit = (p: P) => <Svg {...p}><circle cx="12" cy="12" r="3.5" /><path d="M3 12h5.5M15.5 12H21" /></Svg>;
export const External = (p: P) => (
  <Svg {...p}><path d="M15 3h6v6M10 14 21 3M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" /></Svg>
);
export const Archive = (p: P) => (
  <Svg {...p}><rect x="2" y="4" width="20" height="5" rx="1" /><path d="M4 9v9a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9M10 13h4" /></Svg>
);
export const Circle = (p: P) => <Svg {...p}><circle cx="12" cy="12" r="8" /></Svg>;
export const CircleCheck = (p: P) => <Svg {...p}><circle cx="12" cy="12" r="9" /><path d="m8.5 12 2.5 2.5 4.5-5" /></Svg>;
export const CircleX = (p: P) => <Svg {...p}><circle cx="12" cy="12" r="9" /><path d="m15 9-6 6M9 9l6 6" /></Svg>;
export const CircleDashed = (p: P) => (
  <Svg {...p}><path d="M10.1 2.2a10 10 0 0 1 3.8 0M17.6 3.7a10 10 0 0 1 2.7 2.7M21.8 10.1a10 10 0 0 1 0 3.8M20.3 17.6a10 10 0 0 1-2.7 2.7M13.9 21.8a10 10 0 0 1-3.8 0M6.4 20.3a10 10 0 0 1-2.7-2.7M2.2 13.9a10 10 0 0 1 0-3.8M3.7 6.4a10 10 0 0 1 2.7-2.7" /></Svg>
);
export const CircleSlash = (p: P) => <Svg {...p}><circle cx="12" cy="12" r="9" /><path d="m5.7 5.7 12.6 12.6" /></Svg>;

/** Claude's mark, simplified: an eight-point burst. */
export const ClaudeMark = ({ size = 14, ...rest }: P) => (
  <svg width={size} height={size} viewBox="0 0 24 24" aria-hidden {...rest}>
    <g stroke="#d97757" strokeWidth="2.6" strokeLinecap="round">
      <path d="M12 3v18M3 12h18M5.6 5.6l12.8 12.8M18.4 5.6 5.6 18.4" />
    </g>
  </svg>
);

export function AgentIcon({ agent, size = 13 }: { agent: "claude" | "command" | null; size?: number }) {
  if (agent === "claude") return <ClaudeMark size={size} />;
  if (agent === "command") return <Terminal size={size} className="muted-icon" />;
  return <Bot size={size} className="muted-icon" />;
}

/** The icon for a tool call, by tool name. */
export function ToolIcon({ name, size = 13 }: { name: string; size?: number }) {
  switch (name) {
    case "Bash":
    case "BashOutput":
      return <Terminal size={size} />;
    case "Edit":
    case "MultiEdit":
    case "Write":
    case "NotebookEdit":
      return <Pencil size={size} />;
    case "Read":
    case "LS":
      return <FileIcon size={size} />;
    case "Grep":
    case "Glob":
      return <Search size={size} />;
    case "WebFetch":
    case "WebSearch":
      return <Globe size={size} />;
    case "TodoWrite":
      return <ListIcon size={size} />;
    case "Task":
    case "Agent":
      return <Users size={size} />;
    default:
      return <Bot size={size} />;
  }
}
export const Sparkle = (p: P) => (
  <Svg {...p}>
    <path d="M9.94 15.5a2 2 0 0 0-1.44-1.44l-6.14-1.58a.5.5 0 0 1 0-.96L8.5 9.94A2 2 0 0 0 9.94 8.5l1.58-6.14a.5.5 0 0 1 .96 0l1.58 6.14a2 2 0 0 0 1.44 1.44l6.14 1.58a.5.5 0 0 1 0 .96l-6.14 1.58a2 2 0 0 0-1.44 1.44l-1.58 6.14a.5.5 0 0 1-.96 0z" />
    <path d="M20 3v4M22 5h-4M4 17v2M5 18H3" />
  </Svg>
);
