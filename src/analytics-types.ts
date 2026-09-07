import type { GlobalSummary, ObservationTime, UnavailableReason } from "./dashboard-types";
import type { Attribution, CostShareUnavailable } from "./sessions-types";

export type AnalyticsPage<T> = { evaluatedAt: ObservationTime; items: T[]; totalItems: number; nextCursor: string | null; direct: GlobalSummary };
export type ProjectModelsPage = { items: Attribution[]; totalItems: number; nextCursor: string | null };
export type ProjectAnalytics = {
  attribution: Attribution; direct: GlobalSummary;
  averageSessionCost: { amount: string | null; unavailableReason: "noObservedSessions" | "sessionsWithoutAcceptedUsage" | "incompleteCost" | null };
  models: ProjectModelsPage;
  classification: { provenSubagent: GlobalSummary; parentClassificationUnavailable: GlobalSummary } | null;
  currentCycle: {
    label: string; cycleKey: string | null; start: ObservationTime | null; end: ObservationTime | null;
    observationAgeSeconds: number | null; partial: boolean; hasAmbiguousObservations: boolean;
    unavailableReason: UnavailableReason | null; direct: GlobalSummary | null;
  };
};
export type ModelAnalytics = {
  attribution: Attribution; direct: GlobalSummary; acceptedUsageEvents: number; sessionsUsed: number;
  costShare: string | null; costShareUnavailableReason: CostShareUnavailable | null;
  activePricingVersion: { versionId: number; effectiveAt: ObservationTime } | null;
};
export type ModelHistoryBin = { index: number; start: ObservationTime; end: ObservationTime; acceptedUsageEvents: number; tokens: GlobalSummary["tokens"]; estimatedCost: GlobalSummary["estimatedCost"] };
export type ModelHistory = { attribution: Attribution; start: ObservationTime; end: ObservationTime; pointBudget: number; bins: ModelHistoryBin[]; untimedAcceptedUsageEvents: number; coverageNote: string };
