import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { costText, exactTime, exactTokens, localTime } from "./dashboard-data";
import type { GlobalSummary, ObservationTime, TimeWindow, TokenCategory } from "./dashboard-types";
import { PreserveReadingPosition } from "./PreserveReadingPosition";
import "./call-timeline.css";

type Cost = GlobalSummary["estimatedCost"];
type BilledCategory = { tokens: TokenCategory; estimatedCost: Cost };
type Categories = { input: BilledCategory; cachedInput: BilledCategory; cacheWrites: BilledCategory; output: BilledCategory };
type Call = { id: string; time: ObservationTime; threadId: string; turnId: string | null; responseId: string | null; model: string | null; effort: string | null; tokens: GlobalSummary["tokens"]; categories: Categories; estimatedCost: Cost; priceVersionId: string | null; price: Record<string, string | null> | null; categoryReason: string | null };
type CallsPage = TimeWindow & { items: Call[]; totalItems: number; summary: { categories: Categories; estimatedCost: Cost }; nextCursor: string | null };
type ActivityField = { label: string; preview: string; textRef: string };
type ActivityEvent = { id: string; time: ObservationTime; kind: string; label: string; association: string; parentId: string | null; fields: ActivityField[]; status: string | null; paths: string[]; pathsInferred: boolean };
type ActivityPage = { events: ActivityEvent[]; nextCursor: string | null; scannedBytes: number; totalBytes: number; complete: boolean; association: string; notices: string[] };
type TextPage = { text: string; nextOffset: number | null };
export type CallFilters = { model: string | null; thread: string | null };
const associations: Record<string, string> = { logOrder: "Matched by log order", itemId: "Matched by item / tool ID", responseId: "Matched by response ID", activeBatch: "Matched to the active tool batch", ambiguous: "Ambiguous association", unassigned: "Unassigned activity", context: "Incoming context", unavailable: "Activity unavailable" };
const categories = [["input", "Uncached input"], ["cachedInput", "Cached input"], ["cacheWrites", "Cache writes"], ["output", "Output · includes reasoning"]] as const;

export function CategoryTable({ values, total, label, placeholder }: { values: Categories; total: Cost; label: string; placeholder?: string }) {
  return <div className="call-table-wrap"><table className="call-categories"><caption>{label}</caption><thead><tr><th>Category</th><th>Tokens</th><th>Estimated USD</th></tr></thead><tbody>{categories.filter(([key]) => key !== "cacheWrites" || values.cacheWrites.tokens.knownTokens !== "0").map(([key, title]) => <tr key={key}><th scope="row">{title}</th><td>{placeholder ?? <>{exactTokens(values[key].tokens.knownTokens)}{!values[key].tokens.complete && " · incomplete"}</>}</td><td>{placeholder ?? costText(values[key].estimatedCost)}</td></tr>)}</tbody><tfoot><tr><th>Total API equivalent</th><td /><td>{placeholder ?? costText(total)}</td></tr></tfoot></table></div>;
}

const compareTime = (a: ObservationTime, b: ObservationTime) => a.seconds - b.seconds || a.nanos - b.nanos;
const inside = (time: ObservationTime, window: TimeWindow) => compareTime(time, window.start) > 0 && compareTime(time, window.end) <= 0;
const windowKey = (window: TimeWindow) => `${exactTime(window.start)}:${exactTime(window.end)}`;
const compareCalls = (a: Call, b: Call) => compareTime(a.time, b.time) || (BigInt(a.id) < BigInt(b.id) ? -1 : a.id === b.id ? 0 : 1);
type CallsSnapshot = { scope: string; window: string; page: CallsPage; items: Call[]; pages: number };

export function CallTimeline({ window, refreshKey, filters, setFilters, onOpenPricing }: { window: TimeWindow; refreshKey: string; filters: CallFilters; setFilters: (filters: CallFilters) => void; onOpenPricing?: () => void }) {
  const scope = JSON.stringify([filters.model, filters.thread]);
  const bounds = windowKey(window);
  const [snapshot, setSnapshot] = useState<CallsSnapshot | null>(null);
  const [depth, setDepth] = useState({ scope, pages: 1 });
  const pages = depth.scope === scope ? depth.pages : 1;
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [retry, setRetry] = useState(0);
  const requestKey = JSON.stringify([scope, bounds, refreshKey, pages, retry]);
  const active = useRef(requestKey); active.current = requestKey;
  useEffect(() => {
    let disposed = false;
    const current = () => !disposed && active.current === requestKey;
    if (depth.scope !== scope) {
      setDepth({ scope, pages: 1 });
      setSnapshot(null);
    }
    setLoading(true); setError(null);
    async function read() {
      let after: string | null = null;
      const items = new Map<string, Call>();
      const retained = depth.scope === scope && snapshot?.scope === scope ? snapshot : null;
      const lastLoaded = retained?.items.filter(call => inside(call.time, window)).at(-1);
      const wantedPages = Math.max(pages, retained?.pages ?? 1);
      try {
        for (let index = 0; ; index++) {
          const page: CallsPage = await invoke("usage_calls", { query: { start: window.start, end: window.end, ...filters, after, limit: 50 } });
          if (!current()) return;
          for (const call of page.items) items.set(call.id, call);
          after = page.nextCursor;
          const last = page.items.at(-1);
          const reachedLoaded = !lastLoaded || (last && compareCalls(last, lastLoaded) >= 0);
          if (!after || (index + 1 >= wantedPages && reachedLoaded)) {
            // Publish the entire loaded prefix together. Never reuse an old-window cursor.
            // Late imports may add earlier rows; keep the previously reached calls too.
            setSnapshot({ scope, window: bounds, page, items: [...items.values()], pages: index + 1 });
            break;
          }
        }
      } catch {
        if (current()) setError(retained ? "Calls could not be refreshed. Previously loaded calls remain available inside this interval." : "Calls could not be loaded. Retry this interval.");
      } finally { if (current()) setLoading(false); }
    }
    void read();
    return () => { disposed = true; };
  }, [requestKey]);
  // Explicit period changes remount this view in Dashboard; a moving period must not.
  const shown = snapshot?.scope === scope ? snapshot : null;
  const page = shown?.page;
  const items = shown?.items.filter(call => inside(call.time, window)) ?? [];
  const totalsCurrent = shown?.window === bounds;
  const filtered = !!(filters.model || filters.thread);
  return <PreserveReadingPosition scope={scope}><section className="call-timeline dashboard-panel" aria-labelledby="calls-title" aria-busy={loading}>
    <div className="dashboard-section-heading"><div><span className="eyebrow">SELECTED INTERVAL</span><h2 id="calls-title">Model calls</h2></div><span className="dashboard-muted">{localTime(window.start)} – {localTime(window.end)}</span></div>
    <p>One price per model invocation, including its entire tool batch. Expand a call to inspect the available text and actions inside this interval.</p>
    <form className="call-filters" onSubmit={event => { event.preventDefault(); const data = new FormData(event.currentTarget); setFilters({ model: String(data.get("model") ?? "").trim() || null, thread: String(data.get("thread") ?? "").trim() || null }); }}>
      <label>Model ID<input name="model" maxLength={512} placeholder="All models" defaultValue={filters.model ?? ""} /></label><label>Task ID<input name="thread" maxLength={512} placeholder="All tasks and subagents" defaultValue={filters.thread ?? ""} /></label><button>Filter calls</button><button type="button" onClick={event => { const form = event.currentTarget.form; if (form) { (form.elements.namedItem("model") as HTMLInputElement).value = ""; (form.elements.namedItem("thread") as HTMLInputElement).value = ""; } setFilters({ model: null, thread: null }); }}>Clear filters</button>
    </form>
    {error && <p role="alert" className="dashboard-warning">{error} <button onClick={() => setRetry(value => value + 1)}>Retry calls</button></p>}
    {page && <>
      <CategoryTable values={page.summary.categories} total={page.summary.estimatedCost} placeholder={totalsCurrent ? undefined : error ? "Unavailable" : "Updating…"} label={filtered ? "Filtered calls · totals across every result" : "Selected interval · totals across every call"} />
      <p className="dashboard-muted">Billed quantities follow each saved price version. Cached input is counted once; output includes reasoning. Raw source counters remain available inside each call.</p>
      {totalsCurrent && !page.summary.estimatedCost.complete && <p className="dashboard-warning">Some calls have no preserved price. The total shows the known subtotal. {onOpenPricing && <button onClick={onOpenPricing}>Configure prices</button>}</p>}
      <p role="status">{totalsCurrent ? page.totalItems.toLocaleString() + (filtered ? " matching calls" : " calls") : "Refreshing interval"} · {items.length.toLocaleString()} shown{loading ? " · loading…" : ""}</p>
      {items.length === 0 && totalsCurrent && <p>No recorded calls in this interval.</p>}
      <div className="call-list" key={scope}>{items.map(call => <CallCard key={call.id} call={call} window={window} />)}</div>
      {page.nextCursor && <button disabled={loading || !totalsCurrent} onClick={() => setDepth({ scope, pages: shown!.pages + 1 })}>Load next 50 calls</button>}
    </>}
    {!page && loading && <p role="status">Loading calls and exact interval totals…</p>}
  </section></PreserveReadingPosition>;
}

function CallCard({ call, window }: { call: Call; window: TimeWindow }) {
  const [open, setOpen] = useState(false);
  return <details className="call-card" onToggle={event => setOpen(event.currentTarget.open)}><summary>
    <time dateTime={exactTime(call.time)} title={exactTime(call.time)}><small>{new Date(call.time.seconds * 1000).toLocaleDateString(undefined, { month: "short", day: "numeric" })}</small>{new Date(call.time.seconds * 1000 + call.time.nanos / 1e6).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit", second: "2-digit", fractionalSecondDigits: 3 })}</time>
    <span className="call-model">{call.model ?? "Unknown model"}<small>{call.effort ?? "Reasoning level unavailable"}</small></span><span className="call-task" title={call.threadId}>{call.threadId}</span><strong>{costText(call.estimatedCost)}</strong>
    <span className="call-token-summary">{categories.filter(([key]) => key !== "cacheWrites" || call.categories.cacheWrites.tokens.knownTokens !== "0").map(([key, title]) => <span key={key} title={costText(call.categories[key].estimatedCost)}>{title}: <b>{exactTokens(call.categories[key].tokens.knownTokens)}</b>{!call.categories[key].tokens.complete && " · incomplete"}</span>)}{call.responseId && <span className="call-response" title={call.responseId}>Response: {call.responseId}</span>}</span>
  </summary>{open && <div className="call-body">
    <dl className="call-identities"><dt>Task</dt><dd>{call.threadId}</dd><dt>Response</dt><dd>{call.responseId ?? "ID unavailable; one recorded usage delta"}</dd><dt>Turn</dt><dd>{call.turnId ?? "Unavailable"}</dd><dt>Usage observed</dt><dd>{exactTime(call.time)}</dd></dl>
    <CategoryTable values={call.categories} total={call.estimatedCost} label="This invocation" />
    {call.categoryReason && <p role="status">{call.categoryReason}</p>}
    <details><summary>Raw token counters and saved rates</summary><dl className="call-identities">{Object.entries(call.tokens).map(([key, value]) => <div key={key}><dt>{key.replace(/([A-Z])/g, " $1")}</dt><dd>{exactTokens(value.knownTokens)}{!value.complete && " · incomplete"}</dd></div>)}</dl><p>Input includes cached input. Output includes reasoning; these subsets must not be added again.</p>{call.price ? <><p>Saved price version {call.priceVersionId}. Rates are USD per million tokens.</p><dl className="call-identities">{Object.entries(call.price).map(([key, value]) => <div key={key}><dt>{key.replace(/([A-Z])/g, " $1")}</dt><dd>{value ?? "Unavailable"}</dd></div>)}</dl></> : <p>No saved price is available.</p>}</details>
    <ActivityPanel id={call.id} window={window} />
  </div>}</details>;
}

function ActivityPanel({ id, window }: { id: string; window: TimeWindow }) {
  type RecordedEvent = ActivityEvent & { window: TimeWindow };
  const [events, setEvents] = useState<RecordedEvent[]>([]);
  const [page, setPage] = useState<ActivityPage | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [retry, setRetry] = useState(0);
  const scope = `${id}:${windowKey(window)}:${retry}`;
  const active = useRef(scope); active.current = scope;
  useEffect(() => {
    let disposed = false;
    const current = () => !disposed && active.current === scope;
    setLoading(true); setError(null);
    async function scan() {
      let cursor: string | null = null;
      const collected = new Map<string, RecordedEvent>();
      try {
        do {
          const result: ActivityPage = await invoke("usage_call_activity", { query: { observationId: id, start: window.start, end: window.end, cursor } });
          if (!current()) return;
          if (result.association === "unavailable") {
            setError(result.notices.join(" ") || "Activity is unavailable. Previously read content is retained inside this interval.");
            return;
          }
          for (const event of result.events) collected.set(event.id, { ...event, window });
          const fresh = [...collected.values()];
          setEvents(old => result.nextCursor || !result.complete
            ? [...new Map([...old, ...fresh].map(event => [event.id, event])).values()]
            : fresh);
          setPage(result);
          cursor = result.nextCursor;
          // Yield between bounded reads; closing a call stops all continuations.
          if (cursor) await new Promise(resolve => setTimeout(resolve, 0));
        } while (cursor && current());
      } catch { if (current()) setError("Activity could not be refreshed. Previously read content is retained inside this interval."); }
      finally { if (current()) setLoading(false); }
    }
    void scan(); return () => { disposed = true; };
  }, [scope]);
  const sorted = events.filter(event => inside(event.time, window)).sort((a,b) => compareTime(a.time, b.time) || a.id.localeCompare(b.id));
  const clipped = sorted.length !== events.length;
  return <PreserveReadingPosition scope={id}><section className="call-activity" aria-label="Invocation activity" aria-busy={loading}>
    <h3>Activity from the source log</h3><p className="dashboard-muted">{page ? associations[page.association] : "Reading source…"}. Command results are joined by tool IDs where available. There is one cost for this invocation; no additional command charges.</p>
    {loading && <p role="status">Reading activity… {page?.totalBytes ? Math.floor(page.scannedBytes / page.totalBytes * 100) + "% of captured log" : ""}</p>}
    {page?.notices.map(notice => <p className="activity-notice" key={notice}>{notice}</p>)}
    {clipped && <p className="activity-notice">Some related activity is outside the selected interval and is hidden.</p>}
    {error && <p role="alert">{error} <button onClick={() => setRetry(value => value + 1)}>Retry activity</button></p>}
    {!loading && sorted.length === 0 && <p>No readable activity inside this interval. The recorded call cost remains available above.</p>}
    <ol className="activity-list">{sorted.map(event => {
      // Command start times are checked by the backend and are not in the DTO.
      const checkCommand = event.kind === "CommandExecution" && (compareTime(window.start, event.window.start) > 0 || compareTime(window.end, event.window.end) < 0);
      const labels = new Map<string, number>();
      return <li key={event.id} className={event.association === "unassigned" || event.association === "ambiguous" ? "unassigned" : ""}><div className="activity-heading"><time title={exactTime(event.time)}>{new Date(event.time.seconds * 1000).toLocaleTimeString()}</time><strong>{event.label}</strong>{event.status && <span>{event.status}</span>}<small>{associations[event.association] ?? event.association}</small></div>
        {event.paths.length > 0 && !checkCommand && <div className="activity-paths"><span>{event.pathsInferred ? "Paths inferred from command arguments" : "Paths recorded in the log"}</span>{event.paths.map((path,index) => <code key={`${path}:${index}`}>{path}</code>)}</div>}
        {checkCommand && <p className="dashboard-muted">Command details await validation against the current interval.</p>}
        {event.fields.map(field => {
          const index = labels.get(field.label) ?? 0; labels.set(field.label, index + 1);
          return <TextField key={`${field.label}:${index}`} field={field} hidden={checkCommand && (field.label === "Command" || field.label === "Working directory")} />;
        })}
      </li>;
    })}</ol>
  </section></PreserveReadingPosition>;
}

function TextField({ field, hidden = false }: { field: ActivityField; hidden?: boolean }) {
  const [open, setOpen] = useState(false);
  const [text, setText] = useState<string | null>(null);
  const [next, setNext] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const mounted = useRef(true);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  async function load(offset = 0) {
    setLoading(true); setError(null);
    try { const result = await invoke<TextPage>("usage_activity_text", { query: { textRef: field.textRef, offset } }); if (mounted.current) { setText(old => offset ? (old ?? "") + result.text : result.text); setNext(result.nextOffset); } }
    catch (error) { if (mounted.current) setError(typeof error === "string" ? error : "Text is unavailable. Reopen the call to refresh its source."); }
    finally { if (mounted.current) setLoading(false); }
  }
  return <PreserveReadingPosition><details className="activity-text" hidden={hidden} onToggle={event => { const expanded = event.currentTarget.open; setOpen(expanded); if (expanded && !hidden && text === null && !loading) void load(); }}><summary>{field.label}<span>{field.preview.slice(0, 100).replace(/\s+/g, " ")}</span></summary>{open && !hidden && <><pre>{text ?? field.preview}</pre>{error && <p role="alert">{error} <button onClick={() => void load(next ?? 0)}>Retry text</button></p>}{loading && <p role="status">Loading text…</p>}{next !== null && <button disabled={loading} onClick={() => void load(next)}>Read more text</button>}</>}</details></PreserveReadingPosition>;
}
