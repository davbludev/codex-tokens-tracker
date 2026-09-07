import { AnalyticsPaging, AnalyticsStatus } from "./AnalyticsControls";
import { costText, exactTime, unavailable, usd } from "./dashboard-data";
import { categoryText, tokenCategories } from "./sessions-data";
import type { WeeklyEstimate } from "./dashboard-types";
import { useWeeklyHistory } from "./weekly-data";
import "./sessions.css";
import "./analytics.css";
import "./weekly.css";

function Interval({ estimate }: { estimate: WeeklyEstimate }) {
  return <>{estimate.start && estimate.end ? `(${exactTime(estimate.start)}, ${exactTime(estimate.end)}]` : "Unavailable — no comparable interval"}</>;
}
export function WeeklyHistory() {
  const read = useWeeklyHistory();
  const selected = read.history?.history.find(cycle => cycle.key === read.cycleKey);
  return <section className="sessions analytics weekly-history" aria-labelledby="weekly-history-title">
    <h2 id="weekly-history-title">Weekly History</h2>
    <p className="coverage">Completed observed core Codex weekly cycles, newest first. Observed bounds are not actual cycle boundaries; actual start, end and reset time are unknown. All totals cover only the compared interval, never a complete cycle.</p>
    <p className="coverage">USD/1% compares local estimated token cost with account-wide consumed percentage points. Estimated USD/100% extrapolates that observed model mix; it is not an OpenAI charge or proof of complete account usage.</p>
    <AnalyticsStatus {...read} />
    <AnalyticsPaging label="cycles" loading={read.loading} nextCursor={read.history?.nextCursor} load={read.loadHistory} />
    {read.history && <>
      <p className="coverage">{read.history.excludedSamples} excluded samples. Live changes return to the newest page and select its first cycle.</p>
      {read.history.history.length === 0 ? <p>No completed weekly cycles observed yet.</p> : <div className="sessions-table-wrap" role="region" aria-label="Completed cycle comparison" tabIndex={0}><table>
        <caption>Compared interval totals · start excluded, end included · up to 25 cycles per page</caption>
        <thead><tr><th scope="col">Observed cycle start / end (UTC)</th><th scope="col">Compared interval (UTC)</th><th scope="col">Consumed percentage points</th><th scope="col">Total tokens</th><th scope="col">Estimated USD</th><th scope="col">USD/1%</th><th scope="col">Estimated USD/100%</th></tr></thead>
        <tbody>{read.history.history.map(cycle => <tr key={cycle.key} aria-selected={cycle.key === read.cycleKey}>
          <th scope="row"><button type="button" aria-label={`View models for observed cycle starting ${exactTime(cycle.firstObservation.time)}`} aria-controls="weekly-model-breakdown" aria-pressed={cycle.key === read.cycleKey} onClick={() => read.loadModels(cycle.key, null)}>{exactTime(cycle.firstObservation.time)}</button><small>through {exactTime(cycle.lastObservation.time)}</small>
            <small>{cycle.detectedReset ? "Since observation began in this cycle — reset detected from a decrease" : "Since observation began — partial first cycle"}</small>
            <small>Reported reset metadata: {cycle.lastObservation.resetsAt === null ? "Unknown" : exactTime({ seconds: cycle.lastObservation.resetsAt, nanos: 0 })}. Actual reset time unknown.</small></th>
          <td><Interval estimate={cycle.estimate} />{cycle.hasAmbiguousObservations && <small>Ambiguous observations excluded; any available estimate uses the final trustworthy segment only.</small>}{cycle.estimate.unavailableReason && <small>{unavailable[cycle.estimate.unavailableReason]}</small>}</td>
          <td>{cycle.estimate.consumedPercentagePoints ?? "Unavailable"}</td><td>{cycle.tokens ? categoryText(cycle.tokens.totalTokens) : "Unavailable"}</td>
          <td>{costText(cycle.estimate.estimatedCost)}</td><td>{usd(cycle.estimate.effectiveUsdPerPercent)}</td><td>{usd(cycle.estimate.estimatedFullWeekUsd)}</td>
        </tr>)}</tbody>
      </table></div>}
    </>}
    {selected && <section id="weekly-model-breakdown" aria-labelledby="weekly-models-title">
      <h3 id="weekly-models-title">Selected cycle model breakdown</h3>
      <p className="coverage">Observed cycle: {exactTime(selected.firstObservation.time)} through {exactTime(selected.lastObservation.time)}. Compared interval: <Interval estimate={read.models?.estimate ?? selected.estimate} />.</p>
      {selected.tokens && <dl className="session-categories">{tokenCategories.map(([key, label]) => <div key={key}><dt>{label} tokens</dt><dd>{categoryText(selected.tokens![key])}</dd></div>)}</dl>}
      <p className="coverage">Categories overlap; do not add them together. Model costs use stored price history. No account-wide percentage is allocated to individual models.</p>
      {read.missing && <p role="status">This cycle is no longer in completed history. <button type="button" onClick={() => read.loadHistory(null)}>Reload cycles</button></p>}
      {read.models && <>
        {read.models.estimate.unavailableReason && <p className="coverage">{unavailable[read.models.estimate.unavailableReason]}</p>}
        <AnalyticsPaging label="cycle models" loading={read.loading} nextCursor={read.models.nextCursor} load={after => read.loadModels(selected.key, after)} />
        {read.models.items.length === 0 ? <p>{read.models.estimate.start === null ? "Model breakdown unavailable without a comparable interval." : "No accepted model usage in this compared interval."}</p> : <div className="sessions-table-wrap" role="region" aria-label="Selected cycle models" tabIndex={0}><table>
          <caption>Model usage in the compared interval · up to 25 models per page</caption>
          <thead><tr><th scope="col">Model</th>{tokenCategories.map(([key, label]) => <th scope="col" key={key}>{label} tokens</th>)}<th scope="col">Estimated USD</th></tr></thead>
          <tbody>{read.models.items.map(model => <tr key={model.id}><th scope="row">{model.model ?? "Model unavailable"}</th>{tokenCategories.map(([key]) => <td key={key}>{categoryText(model.tokens[key])}</td>)}<td>{costText(model.estimatedCost)}</td></tr>)}</tbody>
        </table></div>}
      </>}
    </section>}
  </section>;
}
