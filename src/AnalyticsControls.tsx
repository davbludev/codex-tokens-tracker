export function AnalyticsStatus({ loading, error, connectionError, retry }: { loading: boolean; error: string | null; connectionError: string | null; retry: () => void }) {
  return <>{loading && <p role="status">Loading analytics…</p>}{(error || connectionError) && <p role="alert">{error ?? connectionError} <button type="button" onClick={retry}>Retry analytics</button></p>}</>;
}
export function AnalyticsPaging({ label, loading, nextCursor, load }: { label: string; loading: boolean; nextCursor: string | null | undefined; load: (after: string | null) => void }) {
  return <div className="session-page-controls"><button type="button" disabled={loading} onClick={() => load(null)}>First {label} page</button><button type="button" disabled={loading || nextCursor == null} onClick={() => load(nextCursor!)}>Next {label} page</button></div>;
}
