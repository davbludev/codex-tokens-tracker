import type { GlobalSummary, ObservationTime, TokenCategory } from "./dashboard-types";

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
  title: string | null; startedAt: string | null; endedAt: string | null;
  durationSeconds: number | null; firstObservedAt: string | null; lastObservedAt: string | null;
  project: Attribution; direct: GlobalSummary; inclusive: GlobalSummary | null;
};
export type AggregatePageRequest = { after: string | null; limit: number };
/** Children/Ancestors reply as kind "sessions"; ID order is not breadcrumb order. */
export type AggregateSessionPage = {
  items: SessionDetail[]; nextCursor: string | null; totalItems: number; direct: GlobalSummary;
};
export type CostShareUnavailable = "incomplete" | "unavailable" | "zeroDenominator";
export type SessionModelUsage = {
  attribution: Attribution; direct: GlobalSummary;
  /** Decimal percentage, rounded half-up to two places, of the complete direct session cost. */
  costShare: string | null; costShareUnavailableReason: CostShareUnavailable | null;
};
export type SessionModels = {
  scope: "direct"; items: SessionModelUsage[]; nextCursor: string | null; totalItems: number;
  /** The whole session, including models beyond this page. */
  direct: GlobalSummary;
};
export type SessionTimelinePoint = {
  time: ObservationTime; cumulativeTotalTokens: TokenCategory;
  cumulativeEstimatedCost: GlobalSummary["estimatedCost"];
  tokensConnectFromPrevious: boolean; costConnectFromPrevious: boolean;
};
export type SessionBoundaryKind = "observationStart" | "missingTokens" | "tokensResumed" | "unpricedUsage" | "pricedUsageResumed";
export type SessionTimelineBoundary = {
  binIndex: number; firstTime: ObservationTime; lastTime: ObservationTime;
  count: number; kinds: SessionBoundaryKind[]; overloaded: boolean;
};
export type SessionTimeline = {
  scope: "direct"; timeSource: "acceptedObservationTimestamp";
  firstObservedAt: ObservationTime | null; lastObservedAt: ObservationTime | null;
  pointBudget: number; binCount: number; sourceObservationCount: number;
  sourcePointCount: number; returnedPointCount: number; untimedObservationCount: number;
  /** Whole-session totals include untimed usage excluded from the points. */
  direct: GlobalSummary; points: SessionTimelinePoint[]; boundaries: SessionTimelineBoundary[];
  coverageNote: string;
};
export type AggregateReadError = "hierarchyPending" | "invalidQuery" | "storage";
export type SessionAggregateData =
  | { kind: "sessionList"; data: SessionPage }
  | { kind: "session"; data: SessionDetail | null }
  | { kind: "sessions"; data: AggregateSessionPage }
  | { kind: "sessionModels"; data: SessionModels | null }
  | { kind: "sessionTimeline"; data: SessionTimeline | null };
export type AggregateResponse<T> = {
  hierarchyPending: boolean; hierarchyRevision: number; coverageNote: string;
  data: { kind: string; data: T };
};
export type SessionAggregateResponse = Omit<AggregateResponse<unknown>, "data"> & { data: SessionAggregateData };
export type AggregateQuery =
  | { kind: "sessionList"; query: SessionQuery }
  | { kind: "session"; thread: string }
  | { kind: "sessions"; page: AggregatePageRequest }
  | { kind: "children" | "ancestors" | "sessionModels"; thread: string; page: AggregatePageRequest }
  | { kind: "sessionTimeline"; thread: string; pointBudget?: number };
export const initialSessionQuery: SessionQuery = {
  search: null, project: null, model: null, fromSeconds: null, beforeSeconds: null,
  sort: "newest", limit: 25, offset: 0,
};
