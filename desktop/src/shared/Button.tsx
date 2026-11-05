// A button for something that takes a moment. While what its `onClick`
// returns is pending it can't be clicked again, and after a beat (so quick
// ones don't flicker) a spinner takes the label's place, keeping the width.

import { useRef, useState, type ButtonHTMLAttributes, type MouseEvent } from "react";

type Props = Omit<ButtonHTMLAttributes<HTMLButtonElement>, "onClick"> & {
  /** Return a promise to show the button busy until it settles. */
  onClick?: (e: MouseEvent<HTMLButtonElement>) => unknown;
  /** Busy for a reason of its own (a form submitting, a key pressed). */
  busy?: boolean;
};

export function Button({ onClick, busy = false, disabled, className, children, ...rest }: Props) {
  const [running, setRunning] = useState(false);
  // A second click can land before the re-render that disables the button.
  const inFlight = useRef(false);
  const pending = running || busy;
  return (
    <button
      {...rest}
      className={pending ? `${className ?? ""} busy` : className}
      disabled={disabled || pending}
      aria-busy={pending || undefined}
      onClick={(e) => {
        if (inFlight.current || pending) return;
        const result = onClick?.(e);
        if (!isPromise(result)) return;
        inFlight.current = true;
        setRunning(true);
        result
          .catch(() => undefined)
          .finally(() => {
            inFlight.current = false;
            setRunning(false);
          });
      }}
    >
      <span className="button-label">{children}</span>
    </button>
  );
}

function isPromise(v: unknown): v is Promise<unknown> {
  return typeof (v as Promise<unknown> | undefined)?.then === "function";
}
