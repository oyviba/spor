// The agents spor runs itself, as the UI offers them: the one place that
// knows each agent's name, models and permission modes. The backend's
// adapters (src/agent/) are the other half; an agent's id is the same in
// both. Its mark is in Icons.tsx (`AGENT_MARKS`).

import type { AgentId, AgentName, Mode } from "../types";

export interface ModeOption {
  id: Mode;
  label: string;
  hint: string;
}

export interface AgentSpec {
  id: AgentId;
  /** How people name it. */
  label: string;
  /** Its model picks. "" leaves it to the agent's own setting. */
  models: { id: string; label: string }[];
  /** Its permission modes, as it reads them: an agent that can't ask
   * refuses what a mode doesn't allow. */
  modes: ModeOption[];
}

const claude: AgentSpec = {
  id: "claude",
  label: "Claude Code",
  // Claude Code's aliases for the latest of each model.
  models: [
    { id: "", label: "Default model" },
    { id: "fable", label: "Fable" },
    { id: "opus", label: "Opus" },
    { id: "sonnet", label: "Sonnet" },
    { id: "haiku", label: "Haiku" },
  ],
  modes: [
    { id: "supervised", label: "Supervised", hint: "Asks before commands and file edits" },
    { id: "acceptEdits", label: "Auto-accept edits", hint: "Edits freely, asks before commands" },
    { id: "auto", label: "Auto", hint: "Claude's own review approves routine actions and asks about the rest" },
    { id: "full", label: "Full access", hint: "Never asks. git push is refused in every mode" },
    {
      id: "plan",
      label: "Plan first",
      hint: "Reads and plans, changes nothing, and asks you to approve its plan before it starts",
    },
  ],
};

const copilot: AgentSpec = {
  id: "copilot",
  label: "Copilot CLI",
  // Copilot CLI's models change often: its default (`/model` in Copilot,
  // or `COPILOT_MODEL`), or Auto.
  models: [
    { id: "", label: "Default model" },
    { id: "auto", label: "Auto" },
  ],
  // It can't ask in prompt mode: what a mode doesn't allow is refused.
  modes: [
    { id: "supervised", label: "Read only", hint: "Reads and runs read-only commands; edits and other commands are refused" },
    { id: "acceptEdits", label: "Auto-accept edits", hint: "Edits freely; commands that change things are refused" },
    { id: "auto", label: "Auto", hint: "Copilot's safety judge approves or refuses each action" },
    { id: "full", label: "Full access", hint: "Never asks. git push is refused in every mode" },
    { id: "plan", label: "Plan first", hint: "Reads and plans, changes nothing. Follow up to start the work" },
  ],
};

/** Every agent spor runs, by id. */
export const AGENTS: Record<AgentId, AgentSpec> = { claude, copilot };

/** The agents in the order they're offered. */
export const AGENT_IDS: AgentId[] = ["claude", "copilot"];

export function isAgentId(s: string | null | undefined): s is AgentId {
  return AGENT_IDS.includes(s as AgentId);
}

/** An agent, by the name people know it by; a custom command is just
 * "Agent". */
export function agentName(agent: AgentName): string {
  return isAgentId(agent) ? AGENTS[agent].label : "Agent";
}
