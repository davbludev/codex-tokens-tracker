import { useEffect, useRef } from "react";
import { costText } from "./dashboard-data";
import { useSessionRead } from "./sessions-data";
import type { SessionDetail as Detail, SessionRow } from "./sessions-types";
import type { GlobalSummary } from "./dashboard-types";

export function tokenText(summary: GlobalSummary): string {
  const value = summary.tokens.totalTokens;
  return value.knownTokens === null ? "Unavailable" : `${BigInt(value.knownTokens).toLocaleString()}${value.complete ? "" : " (incomplete)"}`;
}

export function SessionDetail({ row, onClose }: { row: SessionRow; onClose: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const { data, error, loading, retry, connectionError } = useSessionRead<Detail | null>({ kind: "session", thread: row.threadId });
  const detail = data?.data.data;
  useEffect(() => { dialog.current?.showModal(); }, []);
  return <dialog className="session-detail" ref={dialog} aria-labelledby="session-detail-title" onClose={onClose}>
    <header><h2 id="session-detail-title">Session details</h2><button type="button" autoFocus onClick={() => dialog.current?.close()}>Close</button></header>
    <h3>{row.title ?? `Untitled: ${row.threadId}`}</h3>
    <p className="session-id">{row.threadId}</p>
    {loading && <p role="status">Loading session…</p>}
    {(error || connectionError) && <p role="alert">{error ?? connectionError} <button type="button" onClick={retry}>Retry session</button></p>}
    {data && !detail && <p role="status">This session is no longer available.</p>}
    {detail && <>
      <p>{detail.project.value ?? "Project unavailable"} · {detail.project.basis === "locationDerived" ? "Location-derived project" : detail.project.basis === "confirmedRepository" ? "Confirmed repository" : "Project identity unavailable"}</p>
      <dl><dt>Direct session usage</dt><dd>{tokenText(detail.direct)} tokens · {costText(detail.direct.estimatedCost)}</dd>
        <dt>Inclusive session usage</dt><dd>{detail.inclusive ? `${tokenText(detail.inclusive)} tokens · ${costText(detail.inclusive.estimatedCost)}` : "Unavailable while hierarchy is being reconciled"}</dd>
        <dt>Parent session</dt><dd>{detail.parentThreadId ?? `Unavailable (${detail.parentState})`}</dd>
        <dt>Weekly percentage impact</dt><dd>Unavailable — not independently measurable</dd></dl>
      <p className="coverage">Direct includes only this session. Inclusive includes this session and all descendants. USD values are estimated token cost.</p>
      <p className="coverage">{data?.coverageNote}</p>
    </>}
  </dialog>;
}
