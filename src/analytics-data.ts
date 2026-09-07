import { costText, money } from "./dashboard-data";
import { categoryText } from "./sessions-data";
import type { GlobalSummary } from "./dashboard-types";
import type { ModelAnalytics, ProjectAnalytics } from "./analytics-types";

export function usageText(summary: GlobalSummary): string {
  return `${categoryText(summary.tokens.totalTokens)} tokens · ${costText(summary.estimatedCost)}`;
}
export function averageText(value: ProjectAnalytics["averageSessionCost"]): string {
  if (value.amount !== null) return money(value.amount);
  const reasons = { noObservedSessions: "no observed sessions", sessionsWithoutAcceptedUsage: "sessions without accepted usage", incompleteCost: "incomplete / unpriced cost" };
  return `Unavailable — ${value.unavailableReason ? reasons[value.unavailableReason] : "cost unavailable"}`;
}
export function modelShareText(row: ModelAnalytics): string {
  if (row.costShare !== null) return `${row.costShare}% of complete global direct cost`;
  const reasons = { incomplete: "incomplete global or model cost", unavailable: "cost unavailable", zeroDenominator: "zero global cost" };
  return `Unavailable — ${row.costShareUnavailableReason ? reasons[row.costShareUnavailableReason] : "cost unavailable"}`;
}
