/** Native decimal strings remain exact; convert only coordinates used for drawing. */
export type ObservationTime = { seconds: number; nanos: number };
export type DashboardRange = "currentCycle" | "last24Hours" | "last7Days" | "last30Days" | "all";
export type BreakdownMetric = "tokens" | "cost";
export type DashboardQuery = { range: DashboardRange; pointBudget?: number; breakdownMetric?: BreakdownMetric };
export type DashboardReadError = "invalidQuery" | "storage";
export type UnavailableReason = "insufficientObservations" | "ambiguousObservation" | "belowOnePercentagePoint" | "unpricedUsage";
export type EstimatedCost = {
  /** Integer trillionths of USD. Null means no known priced subtotal. */
  knownSubtotal: string | null;
  complete: boolean;
  acceptedObservations: number;
};
export type WeeklyObservation = {
  time: ObservationTime;
  usedPercent: string;
  remainingPercent: string;
  resetsAt: number | null;
};
export type WeeklyCycle = {
  key: string;
  firstObservation: WeeklyObservation;
  lastObservation: WeeklyObservation;
  detectedReset: boolean;
  hasAmbiguousObservations: boolean;
  fullCycleCostKnown: boolean;
};
export type WeeklyEstimate = {
  start: ObservationTime | null;
  end: ObservationTime | null;
  consumedPercentagePoints: string | null;
  estimatedCost: EstimatedCost | null;
  /** USD decimal, half-even rounded to 12 decimal places. */
  effectiveUsdPerPercent: string | null;
  estimatedFullWeekUsd: string | null;
  unavailableReason: UnavailableReason | null;
};
export type HistoricalCycle = WeeklyCycle & { estimate: WeeklyEstimate; tokens: GlobalSummary["tokens"] | null };
export type WeeklySummary = {
  evaluatedAt: ObservationTime;
  currentCycle: WeeklyCycle | null;
  observationAgeSeconds: number | null;
  overall: WeeklyEstimate;
  recent: WeeklyEstimate;
  unmatchedCost: EstimatedCost | null;
  unmatchedCostStart: ObservationTime | null;
  history: HistoricalCycle[];
  nextCursor: string | null;
  excludedSamples: number;
  sessionWeeklyPercentageImpact: string | null;
  coverageNote: string;
};
export type TokenCategory = { knownTokens: string | null; complete: boolean };
export type GlobalSummary = {
  tokens: {
    totalTokens: TokenCategory;
    inputTokens: TokenCategory;
    cachedInputTokens: TokenCategory;
    cacheWriteTokens: TokenCategory;
    outputTokens: TokenCategory;
    reasoningTokens: TokenCategory;
  };
  estimatedCost: { knownSubtotal: string | null; complete: boolean };
  coverage: {
    incompleteSessions: number;
    unavailableSessions: number;
    unresolvedUsage: boolean;
    unknownModel: boolean;
    unattributedProject: boolean;
    sourceDiagnostics: boolean;
  };
  observedAt: string | null;
  observedSessions: number;
  placeholders: number;
};
export type ChartPoint = {
  time: ObservationTime;
  segmentId: string | null;
  weeklyUsedPercent: string | null;
  cumulativeEstimatedCost: EstimatedCost | null;
  effectiveUsdPerPercent: string | null;
  unavailableReason: UnavailableReason | null;
  /** False means every series must break before this actual observation. */
  connectFromPrevious: boolean;
};
export type BoundaryKind = "observationStart" | "reset" | "ambiguousObservation" | "unpricedUsage" | "pricedUsageResumed";
export type ChartBoundary = {
  binIndex: number;
  firstTime: ObservationTime;
  lastTime: ObservationTime;
  count: number;
  kinds: BoundaryKind[];
  overloaded: boolean;
};
export type DashboardChart = {
  range: DashboardRange;
  start: ObservationTime;
  end: ObservationTime;
  binCount: number;
  returnedObservationCount: number;
  sourceObservationCount: number;
  points: ChartPoint[];
  boundaries: ChartBoundary[];
  coverageNote: string;
};
export type DashboardResponse = {
  evaluatedAt: ObservationTime;
  weekly: WeeklySummary;
  global: GlobalSummary;
  tokenScope: string;
  chart: DashboardChart;
  localUsage: LocalUsage;
  breakdowns: { metric: BreakdownMetric; models: UsageBreakdown[]; projects: UsageBreakdown[] };
  quotaAnalysis: QuotaAnalysis;
};

export type QuotaHypothesis = { mask: number; writesIncluded: boolean; tokens: string | null; estimatedUsd: string | null; tokenReason: string | null; priceReason: string | null };
export type QuotaInterval = { start: ObservationTime; end: ObservationTime; consumedPercentagePoints: string; tokens: GlobalSummary["tokens"]; hypotheses: QuotaHypothesis[] };
export type QuotaAnalysis = { intervals: QuotaInterval[]; totalIntervals: number };

export type LocalUsageSummary = Pick<GlobalSummary, "tokens" | "estimatedCost" | "observedSessions">;
export type UsageBin = LocalUsageSummary & { index: number; start: ObservationTime; end: ObservationTime };
export type LocalUsage = {
  start: ObservationTime;
  end: ObservationTime;
  binCount: number;
  summary: LocalUsageSummary;
  points: UsageBin[];
  untimedObservations: number;
  coverageNote: string;
};
export type UsageBreakdown = {
  key: string;
  label: string;
  kind: "model" | "project" | "unknown" | "other";
  tokens: TokenCategory;
  estimatedCost: GlobalSummary["estimatedCost"];
};
