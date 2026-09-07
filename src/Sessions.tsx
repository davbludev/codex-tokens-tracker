import { Fragment, useState } from "react";
import { costText } from "./dashboard-data";
import { useSessionRead } from "./sessions-data";
import { initialSessionQuery, type SessionFilters, type SessionPage, type SessionRow } from "./sessions-types";
import { SessionDetail, tokenText } from "./SessionDetail";
import "./sessions.css";

export function Sessions() {
  const [filters, setFilters] = useState<SessionFilters>(initialSessionQuery);
  const [dateError, setDateError] = useState<string | null>(null);
  const [selected, setSelected] = useState<SessionRow | null>(null);
  const { data, error, connectionError, loading, choose, retry } = useSessionRead<SessionPage>({ kind: "sessionList", query: initialSessionQuery });
  const page = data?.data.data;
  function navigate(offset: number, nextFilters = filters) {
    choose({ kind: "sessionList", query: { ...nextFilters, limit: 25, offset } });
  }
  function apply(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    const text = (key: string) => String(form.get(key) ?? "").trim() || null;
    const from = text("from"), through = text("through");
    const fromSeconds = from ? Date.parse(`${from}T00:00:00Z`) / 1000 : null;
    const beforeSeconds = through ? Date.parse(`${through}T00:00:00Z`) / 1000 + 86400 : null;
    if ((fromSeconds !== null && !Number.isFinite(fromSeconds)) || (beforeSeconds !== null && !Number.isFinite(beforeSeconds)) || (fromSeconds !== null && beforeSeconds !== null && fromSeconds >= beforeSeconds)) {
      setDateError("Choose an end date on or after the start date."); return;
    }
    const next: SessionFilters = { search: text("search"), project: text("project"), model: text("model"), sort: form.get("sort") as SessionFilters["sort"], fromSeconds, beforeSeconds };
    setDateError(null); setFilters(next); navigate(0, next);
  }
  return <section className="sessions" aria-labelledby="sessions-title">
    <h2 id="sessions-title">Sessions</h2>
    <p className="coverage">Grouped by project. Sort applies within each project using direct session usage. Dates filter last accepted usage (UTC); rows show lifetime usage.</p>
    <form className="sessions-filters" onSubmit={apply}>
      <label>Search session ID<input name="search" type="search" maxLength={256} /></label>
      <label>Project contains<input name="project" type="search" maxLength={256} placeholder="Path or Unavailable" /></label>
      <label>Model contains<input name="model" type="search" maxLength={256} placeholder="Name or Unavailable" /></label>
      <label>From (UTC)<input name="from" type="date" aria-invalid={!!dateError} aria-describedby={dateError ? "session-date-error" : undefined} /></label>
      <label>Through (UTC)<input name="through" type="date" aria-invalid={!!dateError} aria-describedby={dateError ? "session-date-error" : undefined} /></label>
      <label>Sort within project<select name="sort"><option value="newest">Newest observed</option><option value="usd">Estimated USD (complete first)</option><option value="tokens">Direct tokens</option></select></label>
      <button type="submit">Apply filters</button>
      {dateError && <p id="session-date-error" className="sessions-error" role="alert">{dateError}</p>}
      {error && <p className="sessions-error" role="alert">{error} <button type="button" onClick={retry}>Retry sessions</button></p>}
    </form>
    {connectionError && <p role="status">{connectionError}</p>}
    <div className="sessions-paging">
      <p role="status">{loading ? "Loading sessions…" : page ? `${page.totalItems} sessions · ${page.items.length ? `${page.offset + 1}–${page.offset + page.items.length}` : "0"} shown` : "Sessions unavailable"}</p>
      <button type="button" disabled={loading || !page || page.offset === 0} onClick={() => navigate(Math.max(0, (page?.offset ?? 0) - 25))}>Previous page</button>
      <button type="button" disabled={loading || page?.nextOffset == null} onClick={() => navigate(page!.nextOffset!)}>Next page</button>
    </div>
    <p className="sessions-note">Live updates return to the first page. Unknown values are unavailable; incomplete costs are known subtotals. Titles and durations are unavailable in the current metadata source.</p>
    {page && page.items.length === 0 && <p role="status">No sessions match these filters.</p>}
    {page && page.items.length > 0 && <div className="sessions-table-wrap" tabIndex={0} role="region" aria-label="Session results, scroll horizontally for more columns">
      <table><caption>Direct session usage · estimated USD · immediate observed subagents</caption><thead><tr><th scope="col">Session / ID</th><th scope="col">Last observed / duration</th><th scope="col">Models</th><th scope="col">Direct tokens</th><th scope="col">Estimated USD</th><th scope="col">Subagents</th><th scope="col">Weekly impact</th></tr></thead>
        <tbody>{page.items.map((row, index) => <Fragment key={row.threadId}>
          {(index === 0 || page.items[index - 1].project.id !== row.project.id) && <tr className="sessions-project"><th colSpan={7} scope="colgroup">{row.project.value ?? "Project unavailable"} <span>· {row.project.basis === "locationDerived" ? "Location-derived" : row.project.basis === "confirmedRepository" ? "Confirmed repository" : "Identity unavailable"}</span></th></tr>}
          <tr className="session-row"><th scope="row"><button type="button" className="session-link" onClick={() => setSelected(row)}>{row.title ?? `Untitled: ${row.threadId}`}</button><span className="session-id">{row.threadId}</span></th>
            <td>{row.lastObservedAt ?? "Unavailable"}<small>Duration: {row.durationSeconds === null ? "Unavailable" : `${row.durationSeconds}s`}</small></td>
            <td>{row.models.join(", ") || "Unavailable"}{row.modelCount > row.models.length && <small>+{row.modelCount - row.models.length} more models</small>}{row.unknownModel && row.models.length > 0 && <small>Some model attribution unavailable</small>}</td>
            <td>{tokenText(row.direct)}</td><td>{costText(row.direct.estimatedCost)}</td><td>{row.directSubagentCount ?? "Unavailable"}</td><td>{row.weeklyPercentageImpact === null ? "Unavailable" : `${row.weeklyPercentageImpact}%`}</td></tr>
        </Fragment>)}</tbody>
      </table>
    </div>}
    {data?.hierarchyPending && <p role="status">Subagent counts are unavailable while hierarchy is being reconciled.</p>}
    {selected && <SessionDetail row={selected} onClose={() => setSelected(null)} />}
  </section>;
}
