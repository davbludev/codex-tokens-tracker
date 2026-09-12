import { useEffect, useRef, useState } from "react";
import type { RangeSelection, TimeWindow } from "./dashboard-types";
import { fromMilliseconds, milliseconds } from "./time-navigation";

function localInput(ms: number) { const date = new Date(ms); return new Date(ms - date.getTimezoneOffset() * 60_000).toISOString().slice(0, 23); }
export function PeriodControls({ window, choose, back, reset, depth, loading, requestError }: { window?: TimeWindow; choose: (period: RangeSelection) => void; back: () => void; reset: () => void; depth: number; loading: boolean; requestError?: string | null }) {
  const [error, setError] = useState<string | null>(null);
  const [durationError, setDurationError] = useState<string | null>(null);
  const [submitted, setSubmitted] = useState<"dates" | "trailing" | null>(null);
  const [startText, setStartText] = useState("");
  const [endText, setEndText] = useState("");
  const editing = useRef(false);
  useEffect(() => {
    if (window && !editing.current) { setStartText(localInput(milliseconds(window.start))); setEndText(localInput(milliseconds(window.end))); }
  }, [window?.start.seconds, window?.start.nanos, window?.end.seconds, window?.end.nanos]);
  return <div className="period-controls">
    <div className="time-navigation-help"><span>Drag to select · Ctrl + wheel to zoom · Shift + drag to move</span><button type="button" disabled={!depth} onClick={back}>Back</button><button type="button" disabled={!depth} onClick={reset}>Reset zoom</button></div>
    <details><summary>Custom period</summary>
      <form noValidate onSubmit={event => {
        event.preventDefault(); const form = new FormData(event.currentTarget);
        const start = Date.parse(String(form.get("start"))), end = Date.parse(String(form.get("end")));
        if (!Number.isFinite(start) || !Number.isFinite(end) || start >= end || end > Date.now()) { setError("Enter both dates, with the start before the end and the end no later than now."); return; }
        setError(null); setDurationError(null); setSubmitted("dates"); editing.current = false; choose({ range: "custom", start: fromMilliseconds(start), end: fromMilliseconds(end) });
      }}>
        <p>Exact bounds in your local time zone ({Intl.DateTimeFormat().resolvedOptions().timeZone}).</p>
        <div className="period-fields">
          <label>Start<input name="start" type="datetime-local" step="0.001" required value={startText} onChange={event => { editing.current = true; setStartText(event.target.value); }} aria-invalid={!!error} aria-describedby={error ? "period-error" : undefined} /></label>
          <label>End<input name="end" type="datetime-local" step="0.001" required value={endText} onChange={event => { editing.current = true; setEndText(event.target.value); }} aria-invalid={!!error} aria-describedby={error ? "period-error" : undefined} /></label>
          <button disabled={loading}>Apply dates</button>
        </div>
        {error && <p id="period-error" role="alert" className="dashboard-warning">{error}</p>}
        {!loading && submitted === "dates" && requestError && <p role="alert" className="dashboard-warning">{requestError}</p>}
      </form>
      <form className="period-trailing" noValidate onSubmit={event => {
        event.preventDefault(); const form = new FormData(event.currentTarget); const seconds = Number(form.get("amount")) * Number(form.get("unit"));
        if (!Number.isSafeInteger(Number(form.get("amount"))) || !Number.isSafeInteger(seconds) || seconds <= 0) { setDurationError("Enter a positive whole number of minutes, hours or days."); return; }
        setError(null); setDurationError(null); setSubmitted("trailing"); editing.current = false; choose({ range: "trailing", durationSeconds: seconds });
      }}><label>Last<input name="amount" type="number" min="1" step="1" required defaultValue="60" aria-invalid={!!durationError} aria-describedby={durationError ? "duration-error" : undefined} /></label><label>Unit<select name="unit" aria-label="Duration unit" defaultValue="60"><option value="60">Minutes</option><option value="3600">Hours</option><option value="86400">Days</option></select></label><button disabled={loading}>Follow this period</button><span>Updates with the clock. A mouse selection stays fixed.</span>{durationError && <p id="duration-error" role="alert" className="dashboard-warning">{durationError}</p>}{!loading && submitted === "trailing" && requestError && <p role="alert" className="dashboard-warning">{requestError}</p>}</form>
    </details>
  </div>;
}
