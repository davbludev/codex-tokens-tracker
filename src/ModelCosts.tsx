import { useId, useState } from "react";
import { compactCost, compactTokens, costText, exactTokens, money } from "./dashboard-data";
import { categories, percentShares } from "./quota-categories";
import type { CategoryCosts, ModelCost } from "./dashboard-types";
import "./model-costs.css";

/** Estimated USD per million tokens, still exact trillionths of a USD. */
function perMillion(cost: string | null, tokens: string | null): string | null {
  if (cost === null || tokens === null) return null;
  const total = BigInt(tokens);
  return total > 0n ? String((BigInt(cost) * 1_000_000n) / total) : null;
}
function amounts(split: CategoryCosts): bigint[] | null {
  return split.reason === null ? categories.map(category => BigInt(split[category.key]!)) : null;
}
/** Unrounded, for bar widths only; displayed percentages use percentShares. */
function share(part: bigint, whole: bigint): string {
  return whole > 0n ? Number((part * 10000n) / whole) / 100 + "%" : "0%";
}
function percent(value: number | null | undefined): string {
  return value == null ? "—" : value.toLocaleString(undefined, { minimumFractionDigits: 1 }) + "%";
}

/**
 * What each model costs, and what it is costing you for. The four amounts of a
 * row add up to that row's estimated cost, so the bars are directly comparable.
 */
export function ModelCosts({ rows, totals, onOpenPricing }: { rows: ModelCost[]; totals: CategoryCosts; onOpenPricing?: () => void }) {
  const [detail, setDetail] = useState<"cost" | "tokens">("cost");
  const id = useId();
  const unpriced = rows.filter(row => row.categories.reason !== null);
  const rowTotals = rows.map(row => BigInt(row.estimatedCost.knownSubtotal ?? "0"));
  const rowShares = percentShares(rowTotals, rowTotals.reduce((sum, value) => sum + value, 0n));
  const overall = amounts(totals);
  const categoryShares = overall ? percentShares(overall, overall.reduce((sum, value) => sum + value, 0n)) : [];
  const grand = overall?.reduce((sum, value) => sum + value, 0n) ?? 0n;
  return <section className="model-costs" aria-labelledby={id + "-heading"}>
    <div className="dashboard-section-heading">
      <div><span className="eyebrow">ESTIMATED SPEND</span><h2 id={id + "-heading"}>Cost by model</h2></div>
      <span className="dashboard-muted">{describe(rows)}</span>
    </div>
    <p className="model-costs-lead">What each model would cost at your configured prices, and which kind of token the money went to. Each observation is priced once and its cost lands in exactly one of the four amounts, so a row's four amounts add up to that row's total, and the rows add up to the All models total.</p>
    {rows.length === 0 ? <p className="breakdown-empty">No recorded usage in this range.</p> : <>
      <div className="model-costs-total">
        <div className="model-costs-total-head"><span>All models{unpriced.length > 0 && <em> · excludes {compactTokens(unpricedTokens(unpriced))} unpriced tokens</em>}</span><strong title={costText({ knownSubtotal: overall ? String(grand) : null, complete: true })}>{overall ? compactCost(String(grand)) : "Unpriced"}</strong></div>
        {overall ? <>
          <div className="model-cost-bar" aria-hidden="true">{categories.map((category, index) => <span key={category.key} style={{ width: grand > 0n ? share(overall[index], grand) : "0%", background: category.color }} />)}</div>
          <ul className="model-costs-key">{categories.map((category, index) => <li key={category.key}>
            <span className="hypothesis-dot" style={{ background: category.color }} aria-hidden="true" />
            <span className="model-costs-key-label">{category.label}</span>
            <strong title={money(String(overall[index]))}>{compactCost(String(overall[index]))}</strong>
            <small>{percent(categoryShares[index])}</small>
          </li>)}</ul>
        </> : <p className="model-costs-unpriced">{totals.reason}{onOpenPricing && <button type="button" onClick={onOpenPricing}>Configure prices</button>}</p>}
      </div>
      <ol className="model-cost-list">{rows.map((row, position) => {
        const split = amounts(row.categories);
        const rate = perMillion(row.estimatedCost.knownSubtotal, row.tokens.totalTokens.knownTokens);
        return <li key={row.key} className={row.kind === "model" ? undefined : "model-cost-secondary"}>
          <div className="model-cost-head">
            <span className="model-cost-name" title={row.label}>{row.label}</span>
            <strong title={costText(row.estimatedCost)}>{compactCost(row.estimatedCost.knownSubtotal)}{!row.estimatedCost.complete && row.estimatedCost.knownSubtotal !== null && <span className="incomplete-mark" title="Incomplete known subtotal">*</span>}</strong>
            {split && grand > 0n && <span className="model-cost-share">{percent(rowShares[position])}</span>}
          </div>
          {split
            ? <div className="model-cost-bar" aria-hidden="true">{categories.map((category, index) => <span key={category.key} style={{ width: grand > 0n ? share(split[index], grand) : "0%", background: category.color }} title={category.label + " " + money(String(split[index]))} />)}</div>
            : <div className="model-cost-bar model-cost-bar-unpriced" aria-hidden="true" />}
          <p className="model-cost-meta">{compactTokens(row.tokens.totalTokens.knownTokens)} tokens{rate !== null && <> · {compactCost(rate)} per 1M tokens, blended across categories</>}{row.observedSessions !== null && <> · {row.observedSessions.toLocaleString()} session{row.observedSessions === 1 ? "" : "s"}</>}{split === null && <> · <span className="model-cost-reason">{row.categories.reason}</span></>}</p>
        </li>;
      })}</ol>
      <details className="model-cost-details">
        <summary>Exact per-model amounts</summary>
        <div className="dashboard-ranges" role="group" aria-label="Detail values">
          <button type="button" aria-pressed={detail === "cost"} onClick={() => setDetail("cost")}>Estimated cost</button>
          <button type="button" aria-pressed={detail === "tokens"} onClick={() => setDetail("tokens")}>Tokens</button>
        </div>
        <div className="model-cost-table-wrap" role="region" aria-label="Per-model amounts, scroll horizontally" tabIndex={0}><table>
          <caption>{detail === "cost" ? "Exact estimated USD per token category. The four amounts add up to the model's estimated cost." : "Exact token counts. Cached input is part of input and reasoning is part of output, so these categories must not be added together."}</caption>
          <thead><tr><th scope="col">Model</th>{(detail === "cost" ? categories.map(category => category.label) : ["Input, cached included", "Cached input", "Cache writes", "Output, reasoning included"]).map(label => <th scope="col" key={label}>{label}</th>)}<th scope="col">{detail === "cost" ? "Estimated cost" : "Total tokens"}</th><th scope="col">{detail === "cost" ? "Share of cost" : "Reasoning"}</th></tr></thead>
          <tbody>{rows.map((row, position) => {
            const split = amounts(row.categories);
            const cells = detail === "cost"
              ? categories.map((_category, index) => (split ? money(String(split[index])) : "Unavailable"))
              : [row.tokens.inputTokens, row.tokens.cachedInputTokens, row.tokens.cacheWriteTokens, row.tokens.outputTokens].map(value => exactTokens(value.knownTokens) + (value.complete ? "" : " *"));
            return <tr key={row.key}>
              <th scope="row">{row.label}<small>{row.acceptedObservations.toLocaleString()} usage event{row.acceptedObservations === 1 ? "" : "s"}</small></th>
              {cells.map((value, index) => <td key={index}>{value}</td>)}
              <td>{detail === "cost" ? costText(row.estimatedCost) : exactTokens(row.tokens.totalTokens.knownTokens)}</td>
              <td>{detail === "cost" ? (split ? percent(rowShares[position]) : "—") : exactTokens(row.tokens.reasoningTokens.knownTokens)}</td>
            </tr>;
          })}</tbody>
          {overall && detail === "cost" && <tfoot><tr><th scope="row">All models</th>{categories.map((category, index) => <td key={category.key}>{money(String(overall[index]))}</td>)}<td>{money(String(grand))}</td><td>100.0%</td></tr></tfoot>}
        </table></div>
        <p className="dashboard-muted">An asterisk marks a value that does not cover every accepted observation. Amounts use each observation's own stored price version, so changing a price later never rewrites past cost. {unpricedNote(rows)}</p>
      </details>
    </>}
  </section>;
}

function unpricedNote(rows: ModelCost[]): string {
  const unpriced = rows.filter(row => row.categories.reason !== null);
  return unpriced.length === 0 ? "Every model in this range is priced." : `${unpriced.length} of ${rows.length} rows have no usable split: ${[...new Set(unpriced.map(row => row.categories.reason))].join("; ")}.`;
}
function unpricedTokens(rows: ModelCost[]): string {
  return String(rows.reduce((sum, row) => sum + BigInt(row.tokens.totalTokens.knownTokens ?? "0"), 0n));
}
/** Names what the rows are, so an unattributed or folded row is never miscounted as a model. */
function describe(rows: ModelCost[]): string {
  const named = rows.filter(row => row.kind === "model").length;
  const rest = rows.length - named;
  if (rows.length === 0) return "No models in range";
  return `${named} model${named === 1 ? "" : "s"} in range${rest > 0 ? ` · ${rest} more row${rest === 1 ? "" : "s"}` : ""}`;
}
