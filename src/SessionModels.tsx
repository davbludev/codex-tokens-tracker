import { useEffect, useRef, useState } from "react";
import uPlot from "uplot";
import "uplot/dist/uPlot.min.css";
import { costText } from "./dashboard-data";
import { categoryText, plotTickText, tokenCategories, useSessionRead } from "./sessions-data";
import type { SessionModels as Models, SessionModelUsage } from "./sessions-types";

const colors = ["#e7edf5", "#80b7ff", "#65d8ad", "#edbe74", "#dc9afa", "#ff9b91", "#84d5ee"];
function shareText(row: SessionModelUsage): string {
  if (row.costShare !== null) return `${row.costShare}% of complete direct session cost`;
  const reasons = { incomplete: "incomplete session or model cost", unavailable: "cost unavailable", zeroDenominator: "zero session cost" };
  return `Unavailable — ${row.costShareUnavailableReason ? reasons[row.costShareUnavailableReason] : "cost unavailable"}`;
}
export function SessionModels({ threadId }: { threadId: string }) {
  const { data, error, connectionError, loading, retry, choose } = useSessionRead<Models | null>({ kind: "sessionModels", thread: threadId, page: { after: null, limit: 50 } });
  const page = data?.data.data;
  const load = (after: string | null) => choose({ kind: "sessionModels", thread: threadId, page: { after, limit: 50 } });
  return <section aria-label="Direct model usage">
    <h3>Direct model usage</h3>
    {loading && <p role="status">Loading models…</p>}
    {(error || connectionError) && <p role="alert">{error ?? connectionError} <button type="button" onClick={retry}>Retry models</button></p>}
    {data && !page && <p>Model usage unavailable.</p>}
    {page && <>
      <p className="coverage">{page.totalItems} models · {page.items.length} shown. Whole-session direct cost: {costText(page.direct.estimatedCost)}. Shares come from the complete direct session, including models on other pages. Categories overlap. Live updates return to the first page.</p>
      {page.items.length ? <>
        <ModelChart rows={page.items} />
        <div className="sessions-table-wrap" role="region" aria-label="Direct model usage table, scroll horizontally" tabIndex={0}>
          <table><caption>Exact direct model usage and share of session cost</caption>
            <thead><tr><th scope="col">Model / chart number</th>{tokenCategories.map(([key, label]) => <th key={key} scope="col">{label}</th>)}<th scope="col">Estimated USD</th><th scope="col">Share of session cost</th></tr></thead>
            <tbody>{page.items.map((row, i) => <tr key={row.attribution.id}><th scope="row">{i + 1}. {row.attribution.value ?? "Model unavailable"}</th>
              {tokenCategories.map(([key]) => <td key={key}>{categoryText(row.direct.tokens[key])}</td>)}
              <td>{costText(row.direct.estimatedCost)}</td><td>{shareText(row)}</td></tr>)}</tbody>
          </table>
        </div>
      </> : <p>No model usage available.</p>}
    </>}
    <div className="session-page-controls"><button type="button" disabled={loading} onClick={() => load(null)}>First models page</button>
      <button type="button" disabled={loading || page?.nextCursor == null} onClick={() => load(page!.nextCursor)}>Next models page</button></div>
  </section>;
}

function ModelChart({ rows }: { rows: SessionModelUsage[] }) {
  const host = useRef<HTMLDivElement>(null);
  const [selected, setSelected] = useState(0);
  useEffect(() => {
    setSelected(0);
    if (!host.current) return;
    // Floating point is restricted to drawing; the adjacent table retains exact values.
    const data: uPlot.AlignedData = [rows.map((_, i) => i + 1),
      ...tokenCategories.map(([key]) => rows.map(row => row.direct.tokens[key].knownTokens === null ? null : Number(row.direct.tokens[key].knownTokens))),
      rows.map(row => row.direct.estimatedCost.knownSubtotal === null ? null : Number(row.direct.estimatedCost.knownSubtotal) / 1e12)];
    const plot = new uPlot({ width: Math.max(260, host.current.clientWidth), height: 230,
      legend: { show: false }, cursor: { drag: { x: false, y: false } },
      scales: { x: { time: false, range: [0.5, rows.length + 0.5] }, tokens: { auto: true }, cost: { auto: true } },
      axes: [{ stroke: "#aebccc", label: "Model number in current page", incrs: [1, 2, 5, 10, 20, 50], grid: { show: false } },
        { scale: "tokens", label: "Tokens", stroke: "#aebccc", grid: { stroke: "#293442" }, size: 48, values: (_plot, ticks) => ticks.map(plotTickText) },
        { scale: "cost", label: "Estimated USD", side: 1, stroke: colors[6], grid: { show: false }, size: 48, values: (_plot, ticks) => ticks.map(plotTickText) }],
      series: [{}, ...[...tokenCategories.map(() => "tokens"), "cost"].map((scale, i) => ({ scale, stroke: colors[i], paths: () => null, points: { show: true, size: 6 } }))],
      hooks: { setCursor: [plot => { if (plot.cursor.idx != null) setSelected(plot.cursor.idx); }] },
    }, data, host.current);
    const resize = new ResizeObserver(() => { if (host.current) plot.setSize({ width: Math.max(260, host.current.clientWidth), height: 230 }); });
    resize.observe(host.current);
    return () => { resize.disconnect(); plot.destroy(); };
  }, [rows]);
  const index = Math.min(selected, rows.length - 1);
  const row = rows[index];
  return <figure className="session-chart"><figcaption>Model usage chart · current page · direct known values</figcaption>
    <div className="session-chart-key">{[...tokenCategories.map(([, label]) => label), "Estimated USD"].map((label, i) => <span key={label} style={{ color: colors[i] }}>{label}</span>)}</div>
    <div ref={host} aria-hidden="true" />
    <p className="coverage">Each point represents one category for the numbered model below. Unavailable values are omitted; incomplete values are known subtotals.</p>
    <label htmlFor="session-model-point">Inspect model ({index + 1} of {rows.length} on this page)</label>
    <input id="session-model-point" type="range" min={0} max={rows.length - 1} value={index} onChange={event => setSelected(Number(event.target.value))} aria-describedby="session-model-selected" />
    <p className="coverage">Arrow keys move; Home / End jump to first / last.</p>
    <div id="session-model-selected" role="status" aria-live="polite" aria-atomic="true">
      <strong>{row.attribution.value ?? "Model unavailable"}</strong>
      <dl className="session-categories">{tokenCategories.map(([key, label]) => <div key={key}><dt>{label}</dt><dd>{categoryText(row.direct.tokens[key])}</dd></div>)}</dl>
      <p>Estimated USD: {costText(row.direct.estimatedCost)} · Share of session cost: {shareText(row)}</p>
    </div>
  </figure>;
}
