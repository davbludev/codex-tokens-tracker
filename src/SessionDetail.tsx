import { useEffect, useRef, useState } from "react";
import { costText } from "./dashboard-data";
import { categoryText, coverageText, tokenCategories, useSessionRead } from "./sessions-data";
import type { SessionDetail as Detail } from "./sessions-types";
import type { GlobalSummary } from "./dashboard-types";
import { SessionHierarchy } from "./SessionHierarchy";
import { SessionModels } from "./SessionModels";
import { SessionTimeline } from "./SessionTimeline";

export function tokenText(summary: GlobalSummary): string {
  return categoryText(summary.tokens.totalTokens);
}

export function SessionDetail({ threadId, onClose }: { threadId: string; onClose: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [selected, setSelected] = useState(threadId);
  const heading = useRef<HTMLHeadingElement>(null);
  useEffect(() => {
    const trigger = document.activeElement;
    dialog.current?.showModal();
    return () => { if (trigger instanceof HTMLElement && trigger.isConnected) trigger.focus(); };
  }, []);
  function navigate(id: string) { setSelected(id); heading.current?.focus(); }
  return <dialog className="session-detail" ref={dialog} aria-labelledby="session-detail-title" onClose={onClose}>
    <header><h2 id="session-detail-title" ref={heading} tabIndex={-1}>Session details</h2><button type="button" autoFocus onClick={() => dialog.current?.close()}>Close</button></header>
    <DetailContents key={selected} threadId={selected} navigate={navigate} />
  </dialog>;
}

function DetailContents({ threadId, navigate }: { threadId: string; navigate: (id: string) => void }) {
  const { data, error, loading, retry, connectionError } = useSessionRead<Detail | null>({ kind: "session", thread: threadId });
  const detail = data?.data.data;
  return <>
    <p className="session-id">{threadId}</p>
    {loading && <p role="status">Loading session…</p>}
    {(error || connectionError) && <p role="alert">{error ?? connectionError} <button type="button" onClick={retry}>Retry session</button></p>}
    {data && !detail && <p role="status">This session is no longer available.</p>}
    {detail && <>
      <h3>{detail.title ?? "Title unavailable"}</h3>
      {detail.placeholder && <p role="status">Missing-parent placeholder — this session has not been observed.</p>}
      <p>{detail.project.value ?? "Project unavailable"} · {detail.project.basis === "locationDerived" ? "Location-derived project" : detail.project.basis === "confirmedRepository" ? "Confirmed repository" : "Project identity unavailable"}</p>
      <dl className="session-metadata">
        <dt>Start</dt><dd>{detail.startedAt ?? "Unavailable"}</dd><dt>End</dt><dd>{detail.endedAt ?? "Unavailable"}</dd>
        <dt>Duration</dt><dd>{detail.durationSeconds === null ? "Unavailable" : `${detail.durationSeconds}s`}</dd>
        <dt>First observed usage</dt><dd>{detail.firstObservedAt ?? "Unavailable"}</dd>
        <dt>Last observed usage</dt><dd>{detail.lastObservedAt ?? "Unavailable"}</dd>
        <dt>Direct session usage</dt><dd>{tokenText(detail.direct)} tokens · {costText(detail.direct.estimatedCost)}</dd>
        <dt>Inclusive session usage</dt><dd>{detail.inclusive ? `${tokenText(detail.inclusive)} tokens · ${costText(detail.inclusive.estimatedCost)}` : "Unavailable while hierarchy is being reconciled"}</dd>
        <dt>Effective parent</dt><dd>{detail.parentThreadId ? <button type="button" onClick={() => navigate(detail.parentThreadId!)}>Open parent {detail.parentThreadId}</button> : `Unavailable (${detail.parentState})`}</dd>
        <dt>Weekly percentage impact</dt><dd>Unavailable — not independently measurable</dd>
      </dl>
      <p className="coverage">Direct includes only this session. Inclusive includes this session and all descendants. USD values are estimated token cost. Observed usage times do not establish lifecycle times.</p>
      <h4>Direct token categories</h4>
      <dl className="session-categories">{tokenCategories.map(([key, label]) => <div key={key}><dt>{label}</dt><dd>{categoryText(detail.direct.tokens[key])}</dd></div>)}</dl>
      <p className="coverage">Categories overlap; do not add them together. {coverageText(detail.direct)} {data?.coverageNote}</p>
      <SessionHierarchy threadId={threadId} pending={data!.hierarchyPending} navigate={navigate} />
      <SessionModels threadId={threadId} />
      <SessionTimeline threadId={threadId} />
    </>}
  </>;
}
