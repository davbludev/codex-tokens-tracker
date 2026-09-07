import { useEffect, useRef, useState } from "react";
import uPlot from "uplot";
import "uplot/dist/uPlot.min.css";
import { costText, exactTime } from "./dashboard-data";
import { categoryText, coverageText, plotTickText, useSessionRead } from "./sessions-data";
import type { SessionTimeline as Timeline } from "./sessions-types";

export function SessionTimeline({ threadId }: { threadId: string }) {
  const { data, error, connectionError, loading, retry } = useSessionRead<Timeline | null>({ kind: "sessionTimeline", thread: threadId, pointBudget: 512 });
  const timeline = data?.data.data;
  return <section aria-label="Direct usage timeline">
    <h3>Direct usage over time</h3>
    {loading && <p role="status">Loading timeline…</p>}
    {(error || connectionError) && <p role="alert">{error ?? connectionError} <button type="button" onClick={retry}>Retry timeline</button></p>}
    {data && !timeline && <p>Timeline unavailable.</p>}
    {timeline && <TimelineChart timeline={timeline} />}
  </section>;
}

function TimelineChart({ timeline }: { timeline: Timeline }) {
  const host = useRef<HTMLDivElement>(null);
  const [selected, setSelected] = useState(0);
  useEffect(() => {
    setSelected(0);
    if (!host.current || !timeline.points.length) return;
    const points = timeline.points;
    const data: uPlot.AlignedData = [points.map(p => p.time.seconds + p.time.nanos / 1e9),
      points.map(p => p.cumulativeTotalTokens.knownTokens === null ? null : Number(p.cumulativeTotalTokens.knownTokens)),
      points.map(p => p.cumulativeEstimatedCost.knownSubtotal === null ? null : Number(p.cumulativeEstimatedCost.knownSubtotal) / 1e12)];
    // Connectivity is independent for each series and authoritative even at subpixel gaps.
    const paths: uPlot.Series.PathBuilder = (plot, series, first, last) => {
      const stroke = new Path2D();
      let connected = false;
      for (let i = first; i <= last; i++) {
        const value = data[series][i];
        if (value == null) { connected = false; continue; }
        const x = plot.valToPos(data[0][i], "x", true);
        const y = plot.valToPos(value, plot.series[series].scale!, true);
        const connects = series === 1 ? points[i].tokensConnectFromPrevious : points[i].costConnectFromPrevious;
        if (connected && connects) stroke.lineTo(x, y); else stroke.moveTo(x, y);
        connected = true;
      }
      return { stroke };
    };
    const plot = new uPlot({ width: Math.max(260, host.current.clientWidth), height: 260,
      legend: { show: false }, cursor: { drag: { x: false, y: false } },
      scales: { x: { time: true, range: [data[0][0], Math.max(data[0][data[0].length - 1], data[0][0] + 0.001)] }, tokens: { auto: true }, cost: { auto: true } },
      axes: [{ stroke: "#aebccc", grid: { stroke: "#293442" } },
        { scale: "tokens", label: "Cumulative tokens", stroke: "#80b7ff", size: 48, grid: { stroke: "#293442" }, values: (_plot, ticks) => ticks.map(plotTickText) },
        { scale: "cost", label: "Cumulative est. USD", side: 1, stroke: "#65d8ad", size: 48, grid: { show: false }, values: (_plot, ticks) => ticks.map(plotTickText) }],
      series: [{}, ...["tokens", "cost"].map((scale, i) => ({ scale, stroke: i ? "#65d8ad" : "#80b7ff", width: 1.5, paths, points: { show: true, size: 4 }, spanGaps: false }))],
      hooks: { setCursor: [plot => { if (plot.cursor.idx != null) setSelected(plot.cursor.idx); }] },
    }, data, host.current);
    const resize = new ResizeObserver(() => { if (host.current) plot.setSize({ width: Math.max(260, host.current.clientWidth), height: 260 }); });
    resize.observe(host.current);
    return () => { resize.disconnect(); plot.destroy(); };
  }, [timeline]);
  const index = Math.min(selected, timeline.points.length - 1);
  const point = timeline.points[index];
  return <>
    <p className="coverage">Cumulative direct tokens and estimated USD use independent scales. Gaps break each series separately; disconnected points may retain earlier known subtotals and do not establish complete usage at that time.</p>
    <div className="session-chart-key"><span style={{ color: "#80b7ff" }}>Cumulative tokens</span><span style={{ color: "#65d8ad" }}>Cumulative estimated USD</span></div>
    {!point ? <p>No timed accepted usage available.</p> : <>
      <div className="session-timeline-plot" ref={host} aria-hidden="true" />
      <label htmlFor="session-timeline-point">Inspect usage point ({index + 1} of {timeline.points.length})</label>
      <input id="session-timeline-point" type="range" min={0} max={timeline.points.length - 1} value={index} onChange={event => setSelected(Number(event.target.value))} aria-describedby="session-timeline-selected" />
      <p className="coverage">Arrow keys move; Home / End jump to first / last.</p>
      <dl id="session-timeline-selected" className="session-metadata" role="status" aria-live="polite" aria-atomic="true">
        <dt>Observed usage time</dt><dd>{exactTime(point.time)}</dd>
        <dt>Cumulative tokens</dt><dd>{categoryText(point.cumulativeTotalTokens)} · {point.tokensConnectFromPrevious ? "Connected from previous point" : "Token gap / observation boundary; no connection from previous point"}</dd>
        <dt>Cumulative estimated USD</dt><dd>{costText(point.cumulativeEstimatedCost)} · {point.costConnectFromPrevious ? "Connected from previous point" : "Cost gap / observation boundary; no connection from previous point"}</dd>
      </dl>
    </>}
    <p className="coverage">{timeline.returnedPointCount} retained points of {timeline.sourcePointCount} timestamp groups from {timeline.sourceObservationCount} timed observations. {timeline.untimedObservationCount} untimed observations contribute to direct totals but cannot be placed on this timeline. {timeline.coverageNote} {coverageText(timeline.direct)}</p>
    {!!timeline.boundaries.length && <details><summary>Timeline boundaries ({timeline.boundaries.reduce((sum, b) => sum + b.count, 0)})</summary>
      <ul className="session-boundaries">{timeline.boundaries.map(b => <li key={b.binIndex}>{exactTime(b.firstTime)} – {exactTime(b.lastTime)}: {b.kinds.map(kind => kind.replace(/([A-Z])/g, " $1").toLowerCase()).join(", ")} ({b.count}){b.overloaded ? " — multiple boundaries grouped; connections conservatively broken" : ""}</li>)}</ul>
    </details>}
  </>;
}
