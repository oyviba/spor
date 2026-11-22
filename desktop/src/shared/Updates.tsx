// Updating the app from inside itself (#152). Builds without an updater key
// (development, CI) say so and offer nothing. An update is never installed
// while agents are running: the restart would stop them, so the install
// waits until they're idle, if you ask it to.

import { useCallback, useEffect, useRef, useState } from "react";
import { api, errorText } from "../api";
import { Button } from "./Button";
import { usePoll, useStored } from "./hooks";
import type { ToastAction } from "./app";
import type { UpdateCheck } from "../types";

const DAY_MS = 24 * 60 * 60 * 1000;

export interface Updates {
  info: UpdateCheck | null;
  checking: boolean;
  installing: boolean;
  /** An install that will start once no agent is running. */
  whenIdle: boolean;
  error: string | null;
  /** Check automatically on start and daily. */
  auto: boolean;
  setAuto: (on: boolean) => void;
  running: number;
  check: () => Promise<void>;
  install: () => void;
  cancelWhenIdle: () => void;
}

export function useUpdates(
  running: number,
  toast: (text: string, tone?: "ok" | "error", undo?: boolean, action?: ToastAction) => void,
): Updates {
  const [auto, setAuto] = useStored("updates.check", true);
  const [info, setInfo] = useState<UpdateCheck | null>(null);
  const [checking, setChecking] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [whenIdle, setWhenIdle] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const offered = useRef<string | null>(null);

  const doInstall = useCallback(async () => {
    setInstalling(true);
    setWhenIdle(false);
    setError(null);
    try {
      await api.updateInstall(); // restarts the app when it works
    } catch (e) {
      setError(errorText(e));
      toast(`Couldn't update: ${errorText(e)}`, "error");
    } finally {
      setInstalling(false);
    }
  }, [toast]);

  const install = useCallback(() => {
    if (running > 0) {
      setWhenIdle(true);
      toast(`Will update when the ${running === 1 ? "agent is" : `${running} agents are`} idle`);
    } else {
      void doInstall();
    }
  }, [running, doInstall, toast]);

  // The wait is over once nothing is running.
  useEffect(() => {
    if (whenIdle && running === 0 && !installing) void doInstall();
  }, [whenIdle, running, installing, doInstall]);

  const check = useCallback(
    async (quiet = false) => {
      setChecking(true);
      setError(null);
      try {
        const got = await api.updateCheck();
        setInfo(got);
        const v = got.available?.version;
        if (v && offered.current !== v) {
          offered.current = v;
          toast(`Spor ${v} is available`, "ok", false, { label: "Update", run: install });
        } else if (!quiet && got.enabled && !v) {
          toast("Spor is up to date");
        }
      } catch (e) {
        if (!quiet) setError(errorText(e));
      } finally {
        setChecking(false);
      }
    },
    [install, toast],
  );

  usePoll(() => check(true), DAY_MS, [auto], auto);

  return {
    info,
    checking,
    installing,
    whenIdle,
    error,
    auto,
    setAuto,
    running,
    check: () => check(false),
    install,
    cancelWhenIdle: () => setWhenIdle(false),
  };
}

export function UpdatesPage({ u }: { u: Updates }) {
  const { info } = u;
  const available = info?.available;
  return (
    <div className="settings-group">
      <h4>Updates</h4>
      <p className="small-text muted">
        Spor {info?.current ?? ""}
        {info && !info.enabled && " · This build can't update itself (it has no update key, as in development)."}
      </p>
      {info?.enabled !== false && (
        <>
          <label className="small-text">
            <input type="checkbox" checked={u.auto} onChange={(e) => u.setAuto(e.target.checked)} /> Check for updates
            when Spor starts, and daily
          </label>
          {available ? (
            <div>
              <p>
                <strong>Spor {available.version}</strong> is available.
              </p>
              {available.notes && <pre className="small-text">{available.notes}</pre>}
              {u.running > 0 && !u.whenIdle && (
                <p className="small-text muted">
                  {u.running === 1 ? "An agent is" : `${u.running} agents are`} running; restarting would stop{" "}
                  {u.running === 1 ? "it" : "them"}.
                </p>
              )}
              {u.whenIdle ? (
                <Button className="ghost" onClick={u.cancelWhenIdle}>
                  Waiting for agents to finish — cancel
                </Button>
              ) : (
                <Button className="primary" busy={u.installing} onClick={u.install}>
                  {u.running > 0 ? "Update when idle" : "Update and restart"}
                </Button>
              )}
            </div>
          ) : (
            <Button onClick={u.check} busy={u.checking}>
              Check now
            </Button>
          )}
          {u.error && (
            <p className="small-text" style={{ color: "var(--red, #e5534b)" }}>
              {u.error}
            </p>
          )}
        </>
      )}
    </div>
  );
}
