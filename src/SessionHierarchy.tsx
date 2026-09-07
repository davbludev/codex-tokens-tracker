import { costText } from "./dashboard-data";
import { categoryText, useSessionRead } from "./sessions-data";
import type { AggregateSessionPage } from "./sessions-types";

export function SessionHierarchy({ threadId, pending, navigate }: { threadId: string; pending: boolean; navigate: (id: string) => void }) {
  return <section aria-label="Session hierarchy">
    <h3>Subagent hierarchy</h3>
    <p className="coverage">Open a child to explore its descendants at any depth. Ancestors are an ID-ordered set, not a breadcrumb. Live updates return each view to its first page.</p>
    {pending ? <p role="status">Hierarchy pending — relationships and inclusive usage are unavailable.</p> : <div className="session-relationships">
      <Relationships threadId={threadId} kind="children" navigate={navigate} />
      <Relationships threadId={threadId} kind="ancestors" navigate={navigate} />
    </div>}
  </section>;
}

function Relationships({ threadId, kind, navigate }: { threadId: string; kind: "children" | "ancestors"; navigate: (id: string) => void }) {
  const { data, error, connectionError, loading, retry, choose } = useSessionRead<AggregateSessionPage>({ kind, thread: threadId, page: { after: null, limit: 50 } });
  const page = data?.data.data;
  const title = kind === "children" ? "Children" : "Ancestors";
  function load(after: string | null) { choose({ kind, thread: threadId, page: { after, limit: 50 } }); }
  return <section aria-label={title}>
    <h4>{title}</h4>
    {loading && <p role="status">Loading {title.toLowerCase()}…</p>}
    {(error || connectionError) && <p role="alert">{error ?? connectionError} <button type="button" onClick={retry}>Retry {title.toLowerCase()}</button></p>}
    {page && <>
      <p className="coverage">{page.totalItems} {title.toLowerCase()} · {page.items.length} shown</p>
      {!page.items.length && <p>No {title.toLowerCase()} available.</p>}
      <ul className="session-relations">{page.items.map(item => <li key={item.threadId}>
        <button type="button" className="session-link" onClick={() => navigate(item.threadId)}>{item.title ?? item.threadId}</button>
        {item.title && <span className="session-id">{item.threadId}</span>}
        {item.placeholder && <small>Missing-parent placeholder</small>}
        <small>Direct: {categoryText(item.direct.tokens.totalTokens)} tokens · {costText(item.direct.estimatedCost)}</small>
        <small>Inclusive: {item.inclusive ? `${categoryText(item.inclusive.tokens.totalTokens)} tokens · ${costText(item.inclusive.estimatedCost)}` : "Unavailable"}</small>
      </li>)}</ul>
    </>}
    <div className="session-page-controls"><button type="button" disabled={loading} onClick={() => load(null)}>First {title.toLowerCase()} page</button>
      <button type="button" disabled={loading || page?.nextCursor == null} onClick={() => load(page!.nextCursor)}>Next {title.toLowerCase()} page</button></div>
  </section>;
}
