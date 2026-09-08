import React, { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./style.css";
import { ModelPricing } from "./ModelPricing";
import { Dashboard } from "./Dashboard";
import { Sessions } from "./Sessions";
import { Projects } from "./Projects";
import { Models } from "./Models";
import { WeeklyHistory } from "./WeeklyHistory";
import { Settings } from "./Settings";

type Snapshot = {
  threadId: string | null;
  directTokens: string | null;
  observedAt: string | null;
  coverage: string;
  diagnostic: string | null;
  sourceAvailable: boolean;
};

function App() {
  const [view, setView] = useState<"dashboard" | "sessions" | "projects" | "models" | "weekly" | "settings">("dashboard");
  const [pricingOpen, setPricingOpen] = useState(false);
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    let receivedEvent = false;
    async function connect() {
      try {
        const stop = await listen<Snapshot>("usage-updated", event => {
          receivedEvent = true;
          if (!disposed) { setSnapshot(event.payload); setError(null); }
        });
        if (disposed) { stop(); return; }
        unlisten = stop;
        const initial = await invoke<Snapshot>("usage_snapshot");
        if (!disposed && !receivedEvent) setSnapshot(initial);
      } catch {
        if (!disposed) setError("The desktop usage connection is unavailable. Restart the monitor to reconnect.");
      }
    }
    void connect();
    return () => { disposed = true; unlisten?.(); };
  }, []);
  const views = ["dashboard", "sessions", "projects", "models", "weekly", "settings"] as const;
  const labels = { dashboard: "Dashboard", sessions: "Sessions", projects: "Projects", models: "Models", weekly: "Weekly History", settings: "Settings" };
  const subtitles = { dashboard: "Your local usage, at a glance.", sessions: "Explore the work behind your usage.", projects: "See where your tokens go.", models: "Compare the models you use.", weekly: "Follow your observed weekly quota.", settings: "Manage your source, prices, and preferences." };
  return <div className="app-shell">
    <aside className="app-sidebar">
      <div className="app-brand"><span className="brand-mark" aria-hidden="true">◒</span><span>Codex usage<small>LOCAL ANALYTICS</small></span></div>
      <nav className="app-navigation" aria-label="Usage views">{views.map(item => <button key={item} type="button" aria-pressed={view === item} onClick={() => setView(item)}><NavIcon view={item} /><span>{labels[item]}</span></button>)}</nav>
      <div className="sidebar-status"><span className={`status-dot${snapshot?.sourceAvailable ? " is-live" : ""}`} aria-hidden="true" /><span className="status">{snapshot?.sourceAvailable ? "Watching local sessions" : "Source unavailable"}</span><small>Usage stays on this device.</small></div>
    </aside>
    <ModelPricing open={pricingOpen} onClose={() => setPricingOpen(false)} />
    <main className="app-content">
      <header className="app-header"><div><h1>{labels[view]}</h1><p>{subtitles[view]}</p></div><button className="pricing-trigger" type="button" onClick={() => setPricingOpen(true)}>Model Pricing <span aria-hidden="true">↗</span></button></header>
      {(error || snapshot?.diagnostic) && <p className="diagnostic" role="status">{error ?? snapshot?.diagnostic}</p>}
      {view === "dashboard" ? <Dashboard onOpenPricing={() => setPricingOpen(true)} /> : view === "sessions" ? <Sessions /> : view === "projects" ? <Projects /> : view === "models" ? <Models /> : view === "weekly" ? <WeeklyHistory /> : <Settings onOpenPricing={() => setPricingOpen(true)} />}
    </main>
  </div>;
}

function NavIcon({ view }: { view: string }) {
  const paths: Record<string, React.ReactNode> = {
    dashboard: <><rect x="3" y="3" width="7" height="7" rx="1.5" /><rect x="14" y="3" width="7" height="11" rx="1.5" /><rect x="3" y="14" width="7" height="7" rx="1.5" /><rect x="14" y="18" width="7" height="3" rx="1" /></>,
    sessions: <><path d="M4 5h16v12H9l-5 4z" /><path d="M8 9h8M8 13h5" /></>,
    projects: <path d="M3 7V5h7l2 3h9v12H3z" />,
    models: <path d="m12 3 9 5-9 5-9-5zM3 12l9 5 9-5M3 16l9 5 9-5" />,
    weekly: <><rect x="3" y="5" width="18" height="16" rx="2" /><path d="M7 3v4M17 3v4M3 10h18M7 14h3M14 14h3M7 17h3" /></>,
    settings: <><path d="M4 6h16M4 12h16M4 18h16" /><circle cx="8" cy="6" r="2" /><circle cx="16" cy="12" r="2" /><circle cx="10" cy="18" r="2" /></>,
  };
  return <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">{paths[view]}</svg>;
}

createRoot(document.getElementById("root")!).render(<React.StrictMode><App /></React.StrictMode>);
