import { compactCost, compactTokens, costText, exactTokens } from "./dashboard-data";
import type { BreakdownMetric, UsageBreakdown } from "./dashboard-types";

export function UsageBreakdowns({ models, projects, metric, chooseMetric, loading }: { models: UsageBreakdown[]; projects: UsageBreakdown[]; metric: BreakdownMetric; chooseMetric: (metric: BreakdownMetric) => void; loading: boolean }) {
  return <section className="usage-breakdowns" aria-labelledby="breakdown-heading">
    <div className="dashboard-section-heading"><div><h2 id="breakdown-heading">Where your usage goes</h2><p className="dashboard-muted">Top five, with remaining and unattributed usage included</p></div><div className="dashboard-ranges" role="group" aria-label="Breakdown metric"><button type="button" aria-pressed={metric === "tokens"} onClick={() => chooseMetric("tokens")}>Tokens</button><button type="button" aria-pressed={metric === "cost"} onClick={() => chooseMetric("cost")}>Estimated cost</button></div></div>
    <div className="dashboard-chart-grid" aria-busy={loading}><Breakdown title="By model" items={models} metric={metric} /><Breakdown title="By project" items={projects} metric={metric} /></div>
  </section>;
}

function Breakdown({ title, items, metric }: { title: string; items: UsageBreakdown[]; metric: BreakdownMetric }) {
  const amount = (item: UsageBreakdown) => metric === "tokens" ? item.tokens.knownTokens : item.estimatedCost.knownSubtotal;
  const max = items.reduce((largest, item) => { const value = BigInt(amount(item) ?? "0"); return value > largest ? value : largest; }, 0n);
  return <section className={"dashboard-panel breakdown-panel breakdown-" + metric} aria-label={title}><h3>{title}<span>{metric === "tokens" ? "TOKENS" : "EST. USD"}</span></h3>{items.length === 0 ? <p className="breakdown-empty">No recorded usage in this range.</p> : <ol className="breakdown-list">{items.map(item => {
    const value = amount(item);
    const width = value !== null && max > 0n ? Number(BigInt(value) * 10000n / max) / 100 : 0;
    const complete = metric === "tokens" ? item.tokens.complete : item.estimatedCost.complete;
    return <li key={item.key} className={item.kind === "other" || item.kind === "unknown" ? "breakdown-secondary" : undefined}>
      <div className="breakdown-label"><span title={item.label}>{item.label}</span><strong>{metric === "tokens" ? compactTokens(value) : compactCost(value)}{!complete && <span className="incomplete-mark" title="Incomplete known subtotal">*</span>}</strong></div>
      <div className="breakdown-track" aria-hidden="true"><div style={{ width: width + "%" }} /></div>
      <details className="breakdown-exact"><summary>Exact values for {item.label}</summary><p>{exactTokens(item.tokens.knownTokens)} tokens{!item.tokens.complete && " · incomplete"}. Estimated cost: {costText(item.estimatedCost)}.</p></details>
    </li>;
  })}</ol>}</section>;
}
