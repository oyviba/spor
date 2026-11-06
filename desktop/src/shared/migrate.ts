// Stored settings from older versions, brought up to date before the app
// reads them.

type Store = Pick<Storage, "getItem" | "setItem" | "removeItem">;

/** The Agents section became Work (#81): its stored section, page, what
 * you'd seen and its shortcut carry over. */
export function migrate(store: Store) {
  if (store.getItem("spor.section") === JSON.stringify("agents")) store.setItem("spor.section", JSON.stringify("work"));
  const view = store.getItem("spor.agents.view");
  if (view != null) {
    if (store.getItem("spor.work.view") == null) store.setItem("spor.work.view", view);
    store.removeItem("spor.agents.view");
  }
  // What you'd seen of each task, now by work item.
  const seen = store.getItem("spor.seen");
  if (seen != null) {
    try {
      if (store.getItem("spor.work.seen") == null) {
        const byTask = JSON.parse(seen) as Record<string, number>;
        store.setItem("spor.work.seen", JSON.stringify(Object.fromEntries(Object.entries(byTask).map(([id, at]) => [`task:${id}`, at]))));
      }
    } catch {
      // Nothing worth keeping.
    }
    store.removeItem("spor.seen");
  }
  const raw = store.getItem("spor.settings");
  if (raw) {
    try {
      const settings = JSON.parse(raw);
      const keymap = settings?.keymap;
      if (keymap && "go:agents" in keymap) {
        if (!("go:work" in keymap)) keymap["go:work"] = keymap["go:agents"];
        delete keymap["go:agents"];
        store.setItem("spor.settings", JSON.stringify(settings));
      }
    } catch {
      // Unreadable settings are replaced by the defaults when loaded.
    }
  }
}
