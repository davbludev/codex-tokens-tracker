import { useState } from "react";
import { costText, exactTime, unavailable } from "./dashboard-data";
import { categoryText, coverageText, useSessionRead } from "./sessions-data";
import { averageText, usageText } from "./analytics-data";
import { AnalyticsPaging, AnalyticsStatus } from "./AnalyticsControls";
import type { AnalyticsPage, ProjectAnalytics, ProjectModelsPage } from "./analytics-types";
import "./sessions.css";
import "./analytics.css";

export function Projects() {
  const read = useSessionRead<AnalyticsPage<ProjectAnalytics>>({ kind: "projectAnalytics", page: { after: null, limit: 25 } }, "Projects");
  const page = read.data?.data.data;
  const load = (after: string | null) => read.choose({ kind: "projectAnalytics", page: { after, limit: 25 } });
  return <section className="sessions analytics" aria-labelledby="projects-title">
    <h2 id="projects-title">Projects</h2>
    <p className="coverage">Direct session usage counts each observed session once, including subagents. Missing-parent placeholders are excluded. Live updates return to the first page.</p>
    <AnalyticsStatus {...read} />
    {page && <><p className="coverage">{page.totalItems} projects · {page.items.length} shown. Global direct session usage: {usageText(page.direct)}. {read.data?.coverageNote}</p>
      {page.items.length ? <div className="sessions-table-wrap" role="region" aria-label="Project comparisons, scroll horizontally" tabIndex={0}><table>
        <caption>Project comparisons · estimated USD at configured prices</caption>
        <thead><tr>{["Project", "Sessions", "Total tokens", "Estimated USD", "Average session cost", "Models", "Direct usage classification", "Observed current-cycle usage"].map(label => <th scope="col" key={label}>{label}</th>)}</tr></thead>
        <tbody>{page.items.map(row => <tr key={row.attribution.id}>
          <th scope="row">{row.attribution.value ?? "Project unavailable"}<small>{row.attribution.basis === "confirmedRepository" ? "Confirmed repository" : row.attribution.basis === "locationDerived" ? "Location-derived bucket" : "Project identity unavailable"}</small><small>{coverageText(row.direct)}</small></th>
          <td>{row.direct.observedSessions}</td><td>{categoryText(row.direct.tokens.totalTokens)}</td><td>{costText(row.direct.estimatedCost)}</td><td>{averageText(row.averageSessionCost)}</td>
          <td><ProjectModels key={row.attribution.id} project={row} /></td>
          <td>{row.classification ? <><strong>Proven subagent usage</strong><p>{usageText(row.classification.provenSubagent)}</p><strong>Parent classification unavailable</strong><p>{usageText(row.classification.parentClassificationUnavailable)}</p></> : "Unavailable while hierarchy is being reconciled"}</td>
          <td><CycleUsage cycle={row.currentCycle} /></td>
        </tr>)}</tbody>
      </table></div> : <p>No projects observed yet.</p>}</>}
    <AnalyticsPaging label="projects" loading={read.loading} nextCursor={page?.nextCursor} load={load} />
  </section>;
}

function ProjectModels({ project }: { project: ProjectAnalytics }) {
  const [expanded, setExpanded] = useState(false);
  return <>{expanded ? <ProjectModelPages project={project.attribution.id} /> : <><span>{project.models.items.map(item => item.value ?? "Model unavailable").join(", ") || "No models detected"}</span><small>{project.models.items.length} of {project.models.totalItems} models</small></>}
    {project.models.totalItems > 5 && <button type="button" aria-expanded={expanded} onClick={() => setExpanded(!expanded)}>{expanded ? "Collapse models" : "Browse models"}</button>}</>;
}
function ProjectModelPages({ project }: { project: string }) {
  const read = useSessionRead<ProjectModelsPage>({ kind: "projectModels", project, page: { after: null, limit: 25 } }, "Project models");
  const page = read.data?.data.data;
  return <><AnalyticsStatus {...read} />{page && <><span>{page.items.map(item => item.value ?? "Model unavailable").join(", ") || "No models detected"}</span><small>{page.items.length} of {page.totalItems} models</small></>}
    <AnalyticsPaging label="project models" loading={read.loading} nextCursor={page?.nextCursor} load={after => read.choose({ kind: "projectModels", project, page: { after, limit: 25 } })} /></>;
}
function CycleUsage({ cycle }: { cycle: ProjectAnalytics["currentCycle"] }) {
  return <>{cycle.direct ? usageText(cycle.direct) : `Unavailable — ${cycle.unavailableReason ? unavailable[cycle.unavailableReason] : "no comparable interval"}`}
    {cycle.start && cycle.end && <small>({exactTime(cycle.start)}, {exactTime(cycle.end)}]</small>}
    {cycle.partial && <small>Partial cycle — since observation began.</small>}
    {cycle.hasAmbiguousObservations && <small>Ambiguous observations present.</small>}
    {cycle.observationAgeSeconds !== null && <small>Last observation age: {cycle.observationAgeSeconds}s at read time.</small>}
    {cycle.direct && <small>{coverageText(cycle.direct)}</small>}<small>Observed local interval only; no allocated account quota percentage.</small></>;
}
