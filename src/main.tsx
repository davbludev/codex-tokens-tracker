import React, { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./style.css";
import { ModelPricing } from "./ModelPricing";

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
  const tokens = snapshot?.directTokens;
  return <main>
    <header><h1>Codex usage</h1><span className="status">{snapshot?.sourceAvailable ? "Watching local sessions" : "Source unavailable"}</span><button type="button" onClick={() => setPricingOpen(true)}>Model Pricing</button></header>
    <ModelPricing open={pricingOpen} onClose={() => setPricingOpen(false)} />
    <section aria-labelledby="direct-heading">
      <h2 id="direct-heading">Direct session tokens</h2>
      <p className="value" aria-live="polite" aria-atomic="true">{tokens != null ? BigInt(tokens).toLocaleString() : "Unavailable"}</p>
      <p className="coverage" role="status">{snapshot?.coverage ?? "Connecting to local usage…"}</p>
      <dl><dt>Session</dt><dd>{snapshot?.threadId ?? "Waiting for supported usage"}</dd>
        <dt>Last observation</dt><dd>{snapshot?.observedAt ? <time dateTime={snapshot.observedAt}>{snapshot.observedAt}</time> : "Unavailable"}</dd></dl>
    </section>
    {(error || snapshot?.diagnostic) && <p className="diagnostic" role="status">{error ?? snapshot?.diagnostic}</p>}
    <footer>Shows the session with the latest source observation. Available history imports automatically while live changes continue. Descendants are excluded. Metadata and usage stay on this device.</footer>
  </main>;
}

createRoot(document.getElementById("root")!).render(<React.StrictMode><App /></React.StrictMode>);
