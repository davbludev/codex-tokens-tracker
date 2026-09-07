import { DashboardChart } from "./DashboardChart";
import { costText, exactTime, ranges, unavailable, usd, useDashboard } from "./dashboard-data";
import type { WeeklyEstimate } from "./dashboard-types";
import "./dashboard.css";

export function Dashboard() {
  const { data, range, chooseRange, error, connectionError, loading, now, retry } = useDashboard();
  const weekly = data?.weekly;
  const cycle = weekly?.currentCycle;
  const observation = cycle?.lastObservation;
  const age = observation ? Math.max(0, Math.floor(now / 1000) - observation.time.seconds) : null;
  return <div className="dashboard" aria-label="Usage dashboard">
    {connectionError && <p className="dashboard-warning" role="status">{connectionError}</p>}
    {error && <p className="dashboard-warning" role="alert">{error} <button onClick={retry} disabled={loading}>Retry dashboard</button></p>}
    {loading && <p className="dashboard-muted" role="status">Refreshing dashboard…</p>}
    {data && <>
      <section aria-labelledby="weekly-heading">
        <div className="dashboard-section-heading"><h2 id="weekly-heading">Core codex · weekly quota</h2><span className="dashboard-muted">{cycle?.fullCycleCostKnown ? "Current observation interval" : "Since observation began · partial cycle"}</span></div>
        <div className="dashboard-metrics">
          <Metric label="Weekly used / remaining" value={observation ? `${observation.usedPercent}% / ${observation.remainingPercent}%` : "Unavailable"} />
          <Metric label="Estimated USD · comparable interval" value={costText(weekly!.overall.estimatedCost)} />
          <Metric label="Observed USD / 1%" value={usd(weekly!.overall.effectiveUsdPerPercent)} note={reason(weekly!.overall)} />
          <Metric label="Estimated USD / 100%" value={usd(weekly!.overall.estimatedFullWeekUsd)} />
          <Metric label="Recent USD / 1% · last 15 minutes" value={usd(weekly!.recent.effectiveUsdPerPercent)} note={reason(weekly!.recent)} />
        </div>
        <p className="dashboard-muted">{observation ? <>Last limit observation: <time>{exactTime(observation.time)}</time> ({age}s ago). {observation.resetsAt !== null && <>Expected reset: <time>{exactTime({ seconds: observation.resetsAt, nanos: 0 })}</time>.</>}</> : "Waiting for trustworthy core weekly observations."}</p>
        {weekly!.overall.start && weekly!.overall.end && <p className="dashboard-muted">Comparable interval: {exactTime(weekly!.overall.start)} – {exactTime(weekly!.overall.end)}</p>}
        <p className="dashboard-unmatched">Newer estimated cost after last comparable observation: <strong>{costText(weekly!.unmatchedCost)}</strong>. Excluded from the ratio.</p>
        <p className="dashboard-muted">{weekly!.coverageNote} Estimates reflect locally observed usage and configured prices, not an OpenAI charge or complete account usage.</p>
      </section>
      <section aria-labelledby="tokens-heading">
        <div className="dashboard-section-heading"><h2 id="tokens-heading">Locally observed global tokens · all history</h2><span className="dashboard-muted">{data.global.observedSessions} observed sessions</span></div>
        <dl className="dashboard-tokens">{([ ["totalTokens", "Total"], ["inputTokens", "Input"], ["cachedInputTokens", "Cached input"], ["cacheWriteTokens", "Cache writes"], ["reasoningTokens", "Reasoning"], ["outputTokens", "Output"] ] as const).map(([key, label]) => {
          const category = data.global.tokens[key];
          return <div key={key}><dt>{label}</dt><dd>{category.knownTokens === null ? "Unavailable" : BigInt(category.knownTokens).toLocaleString()}{!category.complete && " · incomplete"}</dd></div>;
        })}</dl>
        <p className="dashboard-muted">Token categories overlap; do not add them together. This all-history scope is independent of the selected chart range. Global estimated USD: {costText(data.global.estimatedCost)}.</p>
        {Object.values(data.global.coverage).some(Boolean) && <p className="dashboard-warning" role="status">Coverage warning: some local usage has incomplete, unavailable, unresolved, unknown-model, unattributed-project, or source diagnostic evidence. Plotted cost covers accepted local observations and is not guaranteed account-wide use. These coverage conditions do not establish timeline gaps.</p>}
      </section>
    </>}
    <section aria-labelledby="chart-heading">
      <div className="dashboard-section-heading"><h2 id="chart-heading">Weekly usage and estimated cost</h2><div className="dashboard-ranges" role="group" aria-label="Chart range">{ranges.map(([value, label]) => <button key={value} aria-pressed={range === value} onClick={() => chooseRange(value)}>{label}</button>)}</div></div>
      {data && <DashboardChart chart={data.chart} />}
    </section>
    {!data && !loading && !error && <p role="status">No dashboard data available.</p>}
  </div>;
}
function reason(estimate: WeeklyEstimate) { return estimate.unavailableReason ? unavailable[estimate.unavailableReason] : undefined; }
function Metric({ label, value, note }: { label: string; value: string; note?: string }) {
  return <div className="dashboard-metric"><h3>{label}</h3><p>{value}</p>{note && <small>{note}</small>}</div>;
}
