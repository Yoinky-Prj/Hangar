import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import App from "./App";
import "./styles.css";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);

// The window starts hidden (no white flash); show it once the first frame is painted.
requestAnimationFrame(() =>
  requestAnimationFrame(() => {
    getCurrentWindow()
      .show()
      .catch(() => {});
  }),
);
