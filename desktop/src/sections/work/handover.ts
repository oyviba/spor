// Handing a branch to an agent (#80, #82): ask what it should do, then make
// a task whose folder has the branch checked out and start the agent there.
// A branch checked out in your checkout takes its uncommitted work along,
// and your checkout goes back to the base. ⌘Z takes it all back.

import { api } from "../../api";
import type { Shell } from "../../shared/app";
import { choiceOf, type AgentSettings } from "../agents/Composer";

export async function handToAgent(shell: Shell, settings: AgentSettings, branch: string, base: string) {
  const here = shell.state?.branch === branch;
  const answer = await shell.prompt({
    title: `Hand ${branch} to an agent`,
    detail: here
      ? `It moves to its own folder with your uncommitted changes, and your checkout switches to ${base}. ⌘Z brings it back.`
      : `It gets its own folder, and an agent works there. ⌘Z takes it back.`,
    placeholder: "What should the agent do?",
    action: settings.agent === "none" ? "Hand over" : "Start agent",
  });
  const prompt = answer?.value.trim();
  if (!prompt) return;
  let id: string | null = null;
  const ok = await shell.act(
    async () => {
      const t = await api.handToAgent(branch, prompt, base, choiceOf(settings), settings.mode);
      id = t.id;
      return `Handed ${branch} to ${settings.agent === "none" ? "a task" : "an agent"}`;
    },
    { pending: `Handing ${branch} over…` },
  );
  if (!ok || !id) return;
  await shell.reloadTasks();
  shell.go({ section: "work", taskId: id });
}
