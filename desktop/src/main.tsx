import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { migrate } from "./shared/migrate";
import "./styles.css";

try {
  migrate(localStorage);
} catch {
  // Blocked storage: nothing stored to bring up to date.
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
