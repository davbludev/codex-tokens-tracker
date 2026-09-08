import type { QuotaCategoryCosts, QuotaInterval } from "./dashboard-types";
import { perPercent, sumPercentages } from "./quota-combinations";

export type CategoryKey = Exclude<keyof QuotaCategoryCosts, "reason">;
/** Fixed order and hues; adjacent pairs pass the color-vision separation check on the dark surface. */
export const categories: { key: CategoryKey; label: string; color: string }[] = [
  { key: "input", label: "Input", color: "#80b7ff" },
  { key: "cachedInput", label: "Cached input", color: "#edbe74" },
  { key: "cacheWrites", label: "Cache writes", color: "#c1a0ff" },
  { key: "output", label: "Output", color: "#65d8ad" },
];

/** Exact USD / 1% for one category of one interval, six display decimals. */
export function categoryPerPercent(interval: QuotaInterval, key: CategoryKey): string | null {
  const amount = interval.categories[key];
  return amount === null ? null : perPercent(amount, interval.consumedPercentagePoints, 12, 6);
}

export const NO_LOCAL_USAGE = "No local usage observations";

/**
 * Weighted USD / 1% per category over the priced intervals. Intervals without
 * local usage or with unpriced usage are counted and excluded, never blocking:
 * the statistics describe the priced intervals and say so.
 */
export function categoryStats(intervals: QuotaInterval[]) {
  const priced = intervals.filter(interval => interval.categories.reason === null);
  const withoutUsage = intervals.filter(interval => interval.categories.reason === NO_LOCAL_USAGE).length;
  const excluded = intervals.length - priced.length - withoutUsage;
  const available = priced.length > 0;
  const percent = sumPercentages(priced.map(interval => interval.consumedPercentagePoints));
  const sums = categories.map(category => priced.reduce((sum, interval) => sum + BigInt(interval.categories[category.key]!), 0n));
  const total = sums.reduce((sum, value) => sum + value, 0n);
  const rows = categories.map((category, index) => ({
    ...category,
    usd: available ? perPercent(String(sums[index]), percent, 12, 6) : null,
    fullUsd: available ? perPercent(String(sums[index] * 100n), percent, 12, 6) : null,
    // Share is display-only; exact amounts remain in the USD strings.
    share: available && total > 0n ? Number(sums[index] * 10000n / total) / 100 : null,
  }));
  return {
    count: priced.length, withoutUsage, excluded, rows,
    totalUsd: available ? perPercent(String(total), percent, 12, 6) : null,
    totalFullUsd: available ? perPercent(String(total * 100n), percent, 12, 6) : null,
    reason: intervals.find(interval => interval.categories.reason && interval.categories.reason !== NO_LOCAL_USAGE)?.categories.reason
      ?? intervals.find(interval => interval.categories.reason)?.categories.reason ?? "No comparable priced intervals",
  };
}
