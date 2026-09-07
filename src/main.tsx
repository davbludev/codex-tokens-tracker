import React, { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./style.css";
import { ModelPricing } from "./ModelPricing";
import { Dashboard } from "./Dashboard";

type Snapshot = {
  threadId: string | null;
  directTokens: string | null;
  observedAt: string | null;
  coverage: string;
  diagnostic: string | null;
  sourceAvailable: boolean;
};

function App() {
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
  return <main>
    <header><h1>Codex usage</h1><span className="status">{snapshot?.sourceAvailable ? "Watching local sessions" : "Source unavailable"}</span><button type="button" onClick={() => setPricingOpen(true)}>Model Pricing</button></header>
    <ModelPricing open={pricingOpen} onClose={() => setPricingOpen(false)} />
    <Dashboard />
    {(error || snapshot?.diagnostic) && <p className="diagnostic" role="status">{error ?? snapshot?.diagnostic}</p>}
    <footer>Available local history imports automatically while live changes continue. Metadata and usage stay on this device.</footer>
  </main>;
}

createRoot(document.getElementById("root")!).render(<React.StrictMode><App /></React.StrictMode>);
