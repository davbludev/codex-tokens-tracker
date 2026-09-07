import { useEffect, useRef, useState } from "react";
import uPlot from "uplot";
import "uplot/dist/uPlot.min.css";
import { costText, exactTime } from "./dashboard-data";
import { categoryText, plotTickText, tokenCategories, useSessionRead } from "./sessions-data";
import { AnalyticsStatus } from "./AnalyticsControls";
import type { Attribution, AggregateQuery } from "./sessions-types";
import type { ModelHistory as History } from "./analytics-types";

const ranges = [[1, "24 hours"], [7, "7 days"], [30, "30 days"], [0, "All observed time"]] as const;
function historyQuery(model: string, days: number): AggregateQuery {
  const now = Date.now();
  const end = { seconds: Math.floor(now / 1000), nanos: (now % 1000) * 1e6 };
  return { kind: "modelHistory", model, start: { seconds: days ? end.seconds - days * 86400 : 0, nanos: days ? end.nanos : 0 }, end, pointBudget: 512 };
}
export function ModelHistoryDialog({ model, onClose }: { model: Attribution; onClose: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => { dialog.current?.showModal(); }, []);
  return <dialog ref={dialog} className="session-detail" aria-labelledby="model-history-title" onClose={onClose}>
    <header><h2 id="model-history-title">Model history: {model.value ?? "Model unavailable"}</h2><button type="button" autoFocus onClick={() => dialog.current?.close()}>Close</button></header>
    <HistoryContents model={model.id} />
  </dialog>;
}
function HistoryContents({ model }: { model: string }) {
  const [days, setDays] = useState(7);
  const read = useSessionRead<History>(historyQuery(model, days), "Model history");
  const history = read.data?.data.data;
  return <>
    <div className="session-page-controls" aria-label="Model history range">{ranges.map(([value, label]) => <button type="button" key={value} aria-pressed={days === value} onClick={() => { setDays(value); read.choose(historyQuery(model, value)); }}>{label}</button>)}<button type="button" onClick={() => read.choose(historyQuery(model, days))}>Update range to now</button></div>
    <AnalyticsStatus {...read} />
    {history && <><p className="coverage">Interval ({exactTime(history.start)}, {exactTime(history.end)}]. Live reads replace this fixed interval; update the range to include newer observations. {history.bins.length} populated bins of at most {history.pointBudget}. Each point is usage within its bin, not cumulative usage. Omitted bins have no accepted observation, not a proven zero.</p>
      <p className="coverage">{history.untimedAcceptedUsageEvents} untimed accepted usage events are excluded from history; model totals include them. {history.coverageNote}</p>
      {history.bins.length ? <HistoryChart history={history} /> : <p>No timed accepted usage in this interval.</p>}</>}
  </>;
}
function HistoryChart({ history }: { history: History }) {
  const host = useRef<HTMLDivElement>(null);
  const [selected, setSelected] = useState(0);
  useEffect(() => {
    setSelected(0);
    if (!host.current) return;
    // Numbers are drawing coordinates only. Exact original strings remain in the readout.
    const data: uPlot.AlignedData = [history.bins.map(bin => bin.end.seconds + bin.end.nanos / 1e9),
      history.bins.map(bin => bin.tokens.totalTokens.knownTokens === null ? null : Number(bin.tokens.totalTokens.knownTokens)),
      history.bins.map(bin => bin.estimatedCost.knownSubtotal === null ? null : Number(bin.estimatedCost.knownSubtotal) / 1e12)];
    const plot = new uPlot({ width: Math.max(260, host.current.clientWidth), height: 240,
      legend: { show: false }, cursor: { drag: { x: false, y: false } },
      scales: { x: { time: true, range: [history.start.seconds + history.start.nanos / 1e9, history.end.seconds + history.end.nanos / 1e9] }, tokens: { auto: true }, cost: { auto: true } },
      axes: [{ stroke: "#aebccc", grid: { show: false } }, { scale: "tokens", label: "Total tokens", stroke: "#aebccc", grid: { stroke: "#293442" }, size: 48, values: (_plot, ticks) => ticks.map(plotTickText) }, { scale: "cost", label: "Estimated USD", side: 1, stroke: "#65d8ad", grid: { show: false }, size: 48, values: (_plot, ticks) => ticks.map(plotTickText) }],
      // Discrete points avoid implying continuity across sparse or unpriced bins.
      series: [{}, { scale: "tokens", stroke: "#80b7ff", paths: () => null, points: { show: true, size: 6 } }, { scale: "cost", stroke: "#65d8ad", paths: () => null, points: { show: true, size: 6 } }],
      hooks: { setCursor: [plot => { if (plot.cursor.idx != null) setSelected(plot.cursor.idx); }] },
    }, data, host.current);
    const resize = new ResizeObserver(() => { if (host.current) plot.setSize({ width: Math.max(260, host.current.clientWidth), height: 240 }); });
    resize.observe(host.current);
    return () => { resize.disconnect(); plot.destroy(); };
  }, [history]);
  const index = Math.min(selected, history.bins.length - 1), bin = history.bins[index];
  return <figure className="session-chart"><figcaption>Known usage per bin · total tokens (blue) and estimated USD (green), independent scales</figcaption><div ref={host} aria-hidden="true" />
    <label htmlFor="model-history-point">Inspect populated bin ({index + 1} of {history.bins.length})</label>
    <input id="model-history-point" type="range" min={0} max={history.bins.length - 1} value={index} onChange={event => setSelected(Number(event.target.value))} aria-describedby="model-history-selected" />
    <p className="coverage">Arrow keys move; Home / End jump to first / last. Categories overlap; do not add them together.</p>
    <div id="model-history-selected" role="status" aria-live="polite" aria-atomic="true"><p>({exactTime(bin.start)}, {exactTime(bin.end)}]</p><p>{bin.acceptedUsageEvents} accepted usage events (not API calls)</p>
      <dl className="session-categories">{tokenCategories.map(([key, label]) => <div key={key}><dt>{label}</dt><dd>{categoryText(bin.tokens[key])}</dd></div>)}</dl><p>Estimated USD: {costText(bin.estimatedCost)}</p></div>
  </figure>;
}
