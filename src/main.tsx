import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { Overlay } from "./Overlay";
import { ErrorBoundary } from "./components/Crash";
import { resolve, type ThemePref } from "./theme";
import "./styles.css";

// Set the theme before first paint so there's no flash of the wrong one.
let pref: ThemePref = "system";
try {
  pref = (localStorage.getItem("framewise.theme") as ThemePref) ?? "system";
} catch {
  /* default */
}
document.documentElement.dataset.theme = resolve(pref);

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <ErrorBoundary>{window.location.hash === "#overlay" ? <Overlay /> : <App />}</ErrorBoundary>
  </React.StrictMode>,
);
