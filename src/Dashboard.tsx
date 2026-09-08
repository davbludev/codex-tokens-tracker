import { DashboardChart } from "./DashboardChart";
import { UsageChart } from "./UsageChart";
import { UsageBreakdowns } from "./UsageBreakdowns";
import { compactCost, compactTokens, costText, exactTime, exactTokens, localTime, ranges, unavailable, usd, useDashboard } from "./dashboard-data";
import type { WeeklyEstimate } from "./dashboard-types";
import "./dashboard.css";

export function Dashboard({ onOpenPricing }: { onOpenPricing?: () => void }) {
  const { data, range, chooseRange, breakdownMetric, chooseBreakdownMetric, error, connectionError, loading, now, retry } = useDashboard();
  const weekly = data?.weekly;
  const cycle = weekly?.currentCycle;
  const observation = cycle?.lastObservation;
  const age = observation ? Math.max(0, Math.floor(now / 1000) - observation.time.seconds) : null;
  const local = data?.localUsage;
  const hasUnpriced = local && local.points.length > 0 && !local.summary.estimatedCost.complete;
  const hasCoverage = data && Object.values(data.global.coverage).some(Boolean);
  return <div className="dashboard" aria-label="Usage dashboard">
    <div className="dashboard-toolbar">
      <div className="dashboard-scope"><span className="eyebrow">LOCAL USAGE</span><span>{local ? local.summary.observedSessions.toLocaleString() + " sessions in range" : "Your recorded session activity"}</span></div>
      <div className="dashboard-ranges" role="group" aria-label="Chart range">{ranges.map(([value, label]) => <button key={value} type="button" aria-pressed={range === value} onClick={() => chooseRange(value)}>{label}</button>)}</div>
    </div>
    {connectionError && <p className="dashboard-warning" role="status">{connectionError}</p>}
    {error && <p className="dashboard-warning" role="alert">{error} <button onClick={retry} disabled={loading}>Retry dashboard</button></p>}
    <div className="dashboard-refresh" role="status">{loading ? "Updating usage…" : local ? localTime(local.start) + " – " + localTime(local.end) : ""}</div>
    {data && local ? <>
      <section className="dashboard-summary" aria-label="Usage in selected range">
        <Metric label="Total tokens" value={compactTokens(local.summary.tokens.totalTokens.knownTokens)} exact={exactTokens(local.summary.tokens.totalTokens.knownTokens)} note={!local.summary.tokens.totalTokens.complete ? "Incomplete count" : "All observed token usage"} accent="tokens" />
        <Metric label="Input tokens" value={compactTokens(local.summary.tokens.inputTokens.knownTokens)} exact={exactTokens(local.summary.tokens.inputTokens.knownTokens)} note={!local.summary.tokens.inputTokens.complete ? "Incomplete count" : "Includes cached input"} />
        <Metric label="Output tokens" value={compactTokens(local.summary.tokens.outputTokens.knownTokens)} exact={exactTokens(local.summary.tokens.outputTokens.knownTokens)} note={!local.summary.tokens.outputTokens.complete ? "Incomplete count" : "Includes reasoning"} />
        <Metric label="Estimated cost" value={local.points.length ? compactCost(local.summary.estimatedCost.knownSubtotal) : "—"} exact={costText(local.summary.estimatedCost)} note={hasUnpriced ? "Incomplete known subtotal" : local.points.length ? "USD · configured prices" : "No recorded activity"} accent="cost" />
      </section>
      <div className="dashboard-chart-grid"><UsageChart usage={local} kind="tokens" /><UsageChart usage={local} kind="cost" onOpenPricing={onOpenPricing} /></div>
      {(hasCoverage || local.untimedObservations > 0 || hasUnpriced) && <div className="dashboard-coverage" role="status"><span className="coverage-indicator" aria-hidden="true">i</span><span>{hasUnpriced ? "Some usage is unpriced. Estimated cost shows only the known subtotal." : "Some usage has incomplete source evidence."}{local.untimedObservations > 0 && " " + local.untimedObservations.toLocaleString() + " observations have no timestamp and cannot be plotted."}</span>{hasUnpriced && onOpenPricing && <button type="button" onClick={onOpenPricing}>Configure prices</button>}</div>}
      <UsageBreakdowns models={data.breakdowns.models} projects={data.breakdowns.projects} metric={data.breakdowns.metric} chooseMetric={chooseBreakdownMetric} loading={loading && breakdownMetric !== data.breakdowns.metric} />
      <section className="weekly-overview dashboard-panel" aria-labelledby="weekly-heading">
        <div className="dashboard-section-heading"><div><span className="eyebrow">ACCOUNT OBSERVATIONS</span><h2 id="weekly-heading">Weekly quota</h2></div><span className="dashboard-muted">{cycle?.fullCycleCostKnown ? "Current observation interval" : "Since observation began · partial cycle"}</span></div>
        <div className="weekly-overview-top">
          <div className="quota-overview"><div className="quota-amount">{observation ? <><strong>{Number(observation.usedPercent).toLocaleString(undefined, { maximumFractionDigits: 1 })}%</strong><span>used · {Number(observation.remainingPercent).toLocaleString(undefined, { maximumFractionDigits: 1 })}% remaining</span></> : <><strong>Awaiting quota data</strong><span>Token tracking continues independently.</span></>}</div><div className="quota-track" aria-hidden="true"><div style={{ width: observation ? Math.max(0, Math.min(100, Number(observation.usedPercent))) + "%" : "0%" }} /></div><p className="dashboard-muted">{observation ? <>Observed {age! < 60 ? age + "s" : Math.floor(age! / 60) + "m"} ago{observation.resetsAt !== null && " · Resets " + localTime({ seconds: observation.resetsAt, nanos: 0 })}</> : "Waiting for trustworthy weekly observations."}</p></div>
          <div className="dashboard-metrics">
            <Metric label="Observed USD / 1%" value={weekly!.overall.effectiveUsdPerPercent === null ? "Unavailable" : displayUsd(weekly!.overall.effectiveUsdPerPercent)} note={reason(weekly!.overall)} />
            <Metric label="Estimated USD / 100%" value={weekly!.overall.estimatedFullWeekUsd === null ? "Unavailable" : displayUsd(weekly!.overall.estimatedFullWeekUsd)} />
            <Metric label="Recent USD / 1%" value={weekly!.recent.effectiveUsdPerPercent === null ? "Unavailable" : displayUsd(weekly!.recent.effectiveUsdPerPercent)} note={reason(weekly!.recent)} />
          </div>
        </div>
        <details className="weekly-details"><summary>Observation details and coverage</summary>
          <dl className="chart-readout"><dt>Weekly used / remaining</dt><dd>{observation ? observation.usedPercent + "% / " + observation.remainingPercent + "%" : "Unavailable"}</dd><dt>Estimated USD · comparable interval</dt><dd>{costText(weekly!.overall.estimatedCost)}</dd><dt>Observed USD / 1%</dt><dd>{usd(weekly!.overall.effectiveUsdPerPercent)}</dd><dt>Estimated USD / 100%</dt><dd>{usd(weekly!.overall.estimatedFullWeekUsd)}</dd><dt>Recent USD / 1% · last 15 minutes</dt><dd>{usd(weekly!.recent.effectiveUsdPerPercent)}</dd></dl>
          {observation && <p className="dashboard-muted">Last limit observation: <time>{exactTime(observation.time)}</time>.</p>}
          {weekly!.overall.start && weekly!.overall.end && <p className="dashboard-muted">Comparable interval: {exactTime(weekly!.overall.start)} – {exactTime(weekly!.overall.end)}</p>}
          <p className="dashboard-unmatched">Newer estimated cost after last comparable observation: <strong>{costText(weekly!.unmatchedCost)}</strong>. Excluded from the ratio.</p>
          <p className="dashboard-muted">{weekly!.coverageNote}</p>
        </details>
        <h3 className="quota-chart-title">Weekly usage and estimated cost</h3>
        <DashboardChart chart={data.chart} />
      </section>
      <details className="dashboard-history-details"><summary>All-history totals and data coverage</summary>
        <h2>Locally observed global tokens · all history</h2>
        <dl className="dashboard-tokens">{([["totalTokens", "Total"], ["inputTokens", "Input"], ["cachedInputTokens", "Cached input"], ["cacheWriteTokens", "Cache writes"], ["reasoningTokens", "Reasoning"], ["outputTokens", "Output"]] as const).map(([key, label]) => {
          const category = data.global.tokens[key];
          return <div key={key}><dt>{label}</dt><dd>{exactTokens(category.knownTokens)}{!category.complete && " · incomplete"}</dd></div>;
        })}</dl>
        <p className="dashboard-muted">Token categories overlap; do not add them together. This all-history scope is independent of the selected chart range. Global estimated USD: {costText(data.global.estimatedCost)}.</p>
        {hasCoverage && <p className="dashboard-warning" role="status">Coverage warning: some local usage has incomplete, unavailable, unresolved, unknown-model, unattributed-project, or source diagnostic evidence. These conditions do not establish timeline gaps.</p>}
        <p className="dashboard-muted">{local.coverageNote}</p>
      </details>
      <p className="dashboard-disclaimer">Estimated costs reflect locally recorded usage and your configured prices. They are not an OpenAI charge or complete account usage.</p>
    </> : loading ? <div className="dashboard-loading" aria-label="Loading dashboard"><div /><div /><div /><div /><div /><div /></div> : !error && <div className="chart-empty" role="status"><strong>No dashboard data available</strong><p>Check your session source in Settings.</p></div>}
  </div>;
}
function reason(estimate: WeeklyEstimate) { return estimate.unavailableReason ? unavailable[estimate.unavailableReason] : undefined; }
function displayUsd(value: string) { const [whole, fraction = ""] = value.split("."); const digits = fraction.slice(0, 4).replace(/0+$/, ""); return "$" + BigInt(whole).toLocaleString() + (digits ? "." + digits : ""); }
function Metric({ label, value, note, exact, accent }: { label: string; value: string; note?: string; exact?: string; accent?: "tokens" | "cost" }) {
  return <div className={"dashboard-metric" + (accent ? " metric-" + accent : "")}><h3>{label}</h3><p title={exact}>{value}</p>{note && <small>{note}</small>}{exact && <details className="metric-exact"><summary>Exact {label.toLowerCase()}</summary><span>{exact}</span></details>}</div>;
}
