import { useRef, useState } from "react";
import { costText, exactTime } from "./dashboard-data";
import { categoryText, coverageText, tokenCategories, useSessionRead } from "./sessions-data";
import { modelShareText, usageText } from "./analytics-data";
import { AnalyticsPaging, AnalyticsStatus } from "./AnalyticsControls";
import { ModelHistoryDialog } from "./ModelHistory";
import type { AnalyticsPage, ModelAnalytics } from "./analytics-types";
import "./sessions.css";
import "./analytics.css";

export function Models() {
  const read = useSessionRead<AnalyticsPage<ModelAnalytics>>({ kind: "modelAnalytics", page: { after: null, limit: 25 } }, "Models");
  const page = read.data?.data.data;
  const [selected, setSelected] = useState<ModelAnalytics | null>(null);
  const heading = useRef<HTMLHeadingElement>(null);
  const trigger = useRef<HTMLElement | null>(null);
  const close = () => { setSelected(null); (trigger.current?.isConnected ? trigger.current : heading.current)?.focus(); };
  return <section className="sessions analytics" aria-labelledby="models-title">
    <h2 id="models-title" ref={heading} tabIndex={-1}>Models</h2>
    <p className="coverage">Accepted usage events (not API calls). Sessions used in count accepted usage; detected but unused models retain unavailable token/cost metrics. Categories overlap; do not add them together. Live updates return to the first page.</p>
    <AnalyticsStatus {...read} />
    {page && <><p className="coverage">{page.totalItems} models · {page.items.length} shown. Global direct session usage: {usageText(page.direct)}. Shares use all models, including other pages. Pricing active as of {exactTime(page.evaluatedAt)}; historical valuations stay unchanged. {read.data?.coverageNote}</p>
      {page.items.length ? <div className="sessions-table-wrap" role="region" aria-label="Model comparisons, scroll horizontally" tabIndex={0}><table>
        <caption>Detected models · exact direct usage and estimated USD</caption>
        <thead><tr><th scope="col">Model / history</th><th scope="col">Accepted usage events (not API calls)</th><th scope="col">Sessions used in</th>{tokenCategories.map(([key, label]) => <th scope="col" key={key}>{label}</th>)}<th scope="col">Estimated USD</th><th scope="col">Share of total cost</th><th scope="col">Active pricing version</th></tr></thead>
        <tbody>{page.items.map(row => <tr key={row.attribution.id}>
          <th scope="row"><button type="button" className="session-link" onClick={event => { trigger.current = event.currentTarget; setSelected(row); }}>{row.attribution.value ?? "Model unavailable"}</button><small>{row.acceptedUsageEvents === 0 ? "No accepted usage" : coverageText(row.direct)}</small></th>
          <td>{row.acceptedUsageEvents}</td><td>{row.sessionsUsed}</td>{tokenCategories.map(([key]) => <td key={key}>{categoryText(row.direct.tokens[key])}</td>)}
          <td>{costText(row.direct.estimatedCost)}</td><td>{modelShareText(row)}</td><td>{row.activePricingVersion ? <>Version {row.activePricingVersion.versionId}<small>Effective {exactTime(row.activePricingVersion.effectiveAt)}</small></> : "Unavailable"}</td>
        </tr>)}</tbody>
      </table></div> : <p>No models detected yet.</p>}</>}
    <AnalyticsPaging label="models" loading={read.loading} nextCursor={page?.nextCursor} load={after => read.choose({ kind: "modelAnalytics", page: { after, limit: 25 } })} />
    {selected && <ModelHistoryDialog model={selected.attribution} onClose={close} />}
  </section>;
}
