import type { GlobalSummary } from "./dashboard-types";

export type Attribution = { id: string; basis: string; value: string | null };
export type SessionFilters = {
  search: string | null; project: string | null; model: string | null;
  fromSeconds: number | null; beforeSeconds: number | null;
  sort: "newest" | "usd" | "tokens";
};
export type SessionQuery = SessionFilters & { limit: number; offset: number };
export type SessionRow = {
  threadId: string; title: string | null; lastObservedAt: string | null;
  durationSeconds: number | null; project: Attribution; models: string[];
  modelCount: number; unknownModel: boolean; direct: GlobalSummary;
  directSubagentCount: number | null; weeklyPercentageImpact: string | null;
};
export type SessionPage = { items: SessionRow[]; totalItems: number; offset: number; nextOffset: number | null };
export type SessionDetail = {
  threadId: string; placeholder: boolean; parentState: string; parentThreadId: string | null;
  project: Attribution; direct: GlobalSummary; inclusive: GlobalSummary | null;
};
export type AggregateResponse<T> = {
  hierarchyPending: boolean; hierarchyRevision: number; coverageNote: string;
  data: { kind: string; data: T };
};
export type AggregateQuery = { kind: "sessionList"; query: SessionQuery } | { kind: "session"; thread: string };
export const initialSessionQuery: SessionQuery = {
  search: null, project: null, model: null, fromSeconds: null, beforeSeconds: null,
  sort: "newest", limit: 25, offset: 0,
};
