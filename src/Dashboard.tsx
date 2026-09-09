import { DashboardChart } from "./DashboardChart";
import { UsageChart } from "./UsageChart";
import { ModelCosts } from "./ModelCosts";
import { TurnActivity } from "./TurnActivity";
import { UsageBreakdowns } from "./UsageBreakdowns";
import { QuotaAnalysis } from "./QuotaAnalysis";
import { QuotaCategoryCosts } from "./QuotaCategoryCosts";
import { compactCost, compactTokens, costText, exactTime, exactTokens, localTime, ranges, unavailable, usd, useDashboard } from "./dashboard-data";
import type { TokenCategory, WeeklyEstimate } from "./dashboard-types";
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
  const tokens = local?.summary.tokens;
  return <div className="dashboard" aria-label="Usage dashboard">
    <div className="dashboard-toolbar">
      <div className="dashboard-scope"><span className="eyebrow">LOCAL USAGE</span><span>{local ? local.summary.observedSessions.toLocaleString() + " sessions in range" : "Your recorded session activity"}</span></div>
      <div className="dashboard-ranges" role="group" aria-label="Chart range">{ranges.map(([value, label]) => <button key={value} type="button" aria-pressed={range === value} onClick={() => chooseRange(value)}>{label}</button>)}</div>
    </div>
    {connectionError && <p className="dashboard-warning" role="status">{connectionError}</p>}
    {error && <p className="dashboard-warning" role="alert">{error} <button onClick={retry} disabled={loading}>Retry dashboard</button></p>}
    <div className="dashboard-refresh" role="status">{loading ? "Updating usage…" : local ? localTime(local.start) + " – " + localTime(local.end) : ""}</div>
    {data && local && tokens ? <>
      <section className="dashboard-summary" aria-label="Usage in selected range">
        <Metric label="Estimated cost" value={local.points.length ? compactCost(local.summary.estimatedCost.knownSubtotal) : "—"} exact={costText(local.summary.estimatedCost)} note={hasUnpriced ? "Part of this range has no price yet" : local.points.length ? "At your configured prices, not a bill" : "No recorded activity"} accent="cost" />
        <Metric label="Total tokens" value={compactTokens(tokens.totalTokens.knownTokens)} exact={exactTokens(tokens.totalTokens.knownTokens)} note={!tokens.totalTokens.complete ? "Incomplete count" : "Input plus output"} accent="tokens" />
        <Metric label="Input tokens" value={compactTokens(tokens.inputTokens.knownTokens)} exact={exactTokens(tokens.inputTokens.knownTokens)} note={!tokens.inputTokens.complete ? "Incomplete count" : portion(tokens.cachedInputTokens, tokens.inputTokens, "read from cache") ?? "Cached input included"} />
        <Metric label="Output tokens" value={compactTokens(tokens.outputTokens.knownTokens)} exact={exactTokens(tokens.outputTokens.knownTokens)} note={!tokens.outputTokens.complete ? "Incomplete count" : portion(tokens.reasoningTokens, tokens.outputTokens, "spent on reasoning") ?? "Reasoning included"} />
      </section>
      <div className="dashboard-chart-grid"><UsageChart usage={local} kind="tokens" /><UsageChart usage={local} kind="cost" onOpenPricing={onOpenPricing} /></div>
      {(hasCoverage || local.untimedObservations > 0 || hasUnpriced) && <div className="dashboard-coverage" role="status"><span className="coverage-indicator" aria-hidden="true">i</span><span>{hasUnpriced ? "Some usage is unpriced. Estimated cost shows only the known subtotal." : "Some usage has incomplete source evidence."}{local.untimedObservations > 0 && " " + local.untimedObservations.toLocaleString() + " observations have no timestamp and cannot be plotted."}</span>{hasUnpriced && onOpenPricing && <button type="button" onClick={onOpenPricing}>Configure prices</button>}</div>}
      <ModelCosts rows={data.breakdowns.modelCosts} totals={data.breakdowns.categoryTotals} onOpenPricing={onOpenPricing} />
      <TurnActivity activity={data.turnActivity} onOpenPricing={onOpenPricing} />
      <UsageBreakdowns projects={data.breakdowns.projects} metric={data.breakdowns.metric} chooseMetric={chooseBreakdownMetric} loading={loading && breakdownMetric !== data.breakdowns.metric} />
      <section className="weekly-overview dashboard-panel" aria-labelledby="weekly-heading">
        <div className="dashboard-section-heading"><div><span className="eyebrow">ACCOUNT OBSERVATIONS</span><h2 id="weekly-heading">Weekly quota</h2></div><span className="dashboard-muted">{cycle?.fullCycleCostKnown ? "Covers the whole current cycle" : "Since this device started watching · part of a cycle"}</span></div>
        <p className="weekly-lead">How much of your weekly limit this device has seen used, and what one percentage point of it has cost in locally recorded usage.</p>
        <div className="weekly-overview-top">
          <div className="quota-overview"><div className="quota-amount">{observation ? <><strong>{Number(observation.usedPercent).toLocaleString(undefined, { maximumFractionDigits: 1 })}%</strong><span>used · {Number(observation.remainingPercent).toLocaleString(undefined, { maximumFractionDigits: 1 })}% remaining</span></> : <><strong>Awaiting quota data</strong><span>Token tracking continues independently.</span></>}</div><div className="quota-track" aria-hidden="true"><div style={{ width: observation ? Math.max(0, Math.min(100, Number(observation.usedPercent))) + "%" : "0%" }} /></div><p className="dashboard-muted">{observation ? <>Observed {elapsed(age!)} ago{observation.resetsAt !== null && " · Resets " + localTime({ seconds: observation.resetsAt, nanos: 0 })}</> : "Waiting for trustworthy weekly observations."}</p></div>
          <div className="dashboard-metrics">
            <Metric label="Cost of 1% of the limit" value={weekly!.overall.effectiveUsdPerPercent === null ? "Unavailable" : displayUsd(weekly!.overall.effectiveUsdPerPercent, 4)} note={reason(weekly!.overall) ?? "From what this device recorded while the limit rose"} />
            <Metric label="Cost of a full 100%" value={weekly!.overall.estimatedFullWeekUsd === null ? "Unavailable" : displayUsd(weekly!.overall.estimatedFullWeekUsd, 2)} note="If the whole week cost the same per point" />
            <Metric label="Cost of 1%, recently" value={weekly!.recent.effectiveUsdPerPercent === null ? "Unavailable" : displayUsd(weekly!.recent.effectiveUsdPerPercent, 4)} note={reason(weekly!.recent) ?? "Same measure, last 15 minutes only"} />
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
      {data.quotaAnalysis && <details className="dashboard-research">
        <summary><span className="eyebrow">ADVANCED</span><span className="dashboard-research-title">What a weekly percentage point is made of</span><span className="dashboard-muted">Research panels: which token categories the weekly limit appears to count, and how the cost of one percentage point splits. Nothing above depends on them.</span></summary>
        <QuotaAnalysis analysis={data.quotaAnalysis} />
        <QuotaCategoryCosts analysis={data.quotaAnalysis} />
      </details>}
      <p className="dashboard-disclaimer">Estimated costs reflect locally recorded usage and your configured prices. They are not an OpenAI charge or complete account usage.</p>
    </> : loading ? <div className="dashboard-loading" aria-label="Loading dashboard"><div /><div /><div /><div /><div /><div /></div> : !error && <div className="chart-empty" role="status"><strong>No dashboard data available</strong><p>Check your session source in Settings.</p></div>}
  </div>;
}
/** Seconds, minutes, then hours and days, so a stale observation reads as stale. */
function elapsed(seconds: number): string {
  if (seconds < 60) return seconds + "s";
  if (seconds < 5400) return Math.floor(seconds / 60) + "m";
  if (seconds < 172800) return Math.floor(seconds / 3600) + "h " + Math.floor((seconds % 3600) / 60) + "m";
  return Math.floor(seconds / 86400) + "d";
}
function reason(estimate: WeeklyEstimate) { return estimate.unavailableReason ? unavailable[estimate.unavailableReason] : undefined; }
/** Plain-language share of one token category inside another, when both are known. */
function portion(part: TokenCategory, whole: TokenCategory, suffix: string): string | undefined {
  if (part.knownTokens === null || whole.knownTokens === null || !part.complete) return undefined;
  const total = BigInt(whole.knownTokens);
  if (total <= 0n) return undefined;
  return Number((BigInt(part.knownTokens) * 1000n) / total) / 10 + "% " + suffix;
}
/** Fixed decimals, so two tiles measuring the same thing never differ in precision. */
function displayUsd(value: string, digits: number) { const [whole, fraction = ""] = value.split("."); return "$" + BigInt(whole).toLocaleString() + "." + fraction.slice(0, digits).padEnd(digits, "0"); }
function Metric({ label, value, note, exact, accent }: { label: string; value: string; note?: string; exact?: string; accent?: "tokens" | "cost" }) {
  return <div className={"dashboard-metric" + (accent ? " metric-" + accent : "")}><h3>{label}</h3><p title={exact}>{value}</p>{note && <small>{note}</small>}{exact && <details className="metric-exact"><summary>Exact {label.toLowerCase()}</summary><span>{exact}</span></details>}</div>;
}
