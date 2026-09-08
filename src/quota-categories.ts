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

/** Weighted USD / 1% per category over the retained intervals; unavailable unless every interval is priced. */
export function categoryStats(intervals: QuotaInterval[]) {
  const priced = intervals.filter(interval => interval.categories.reason === null);
  // Never present a priced subset as the monetary result for the whole scope.
  const complete = intervals.length > 0 && priced.length === intervals.length;
  const percent = sumPercentages(priced.map(interval => interval.consumedPercentagePoints));
  const sums = categories.map(category => priced.reduce((sum, interval) => sum + BigInt(interval.categories[category.key]!), 0n));
  const total = sums.reduce((sum, value) => sum + value, 0n);
  const rows = categories.map((category, index) => ({
    ...category,
    usd: complete ? perPercent(String(sums[index]), percent, 12, 6) : null,
    fullUsd: complete ? perPercent(String(sums[index] * 100n), percent, 12, 6) : null,
    // Share is display-only; exact amounts remain in the USD strings.
    share: complete && total > 0n ? Number(sums[index] * 10000n / total) / 100 : null,
  }));
  return {
    count: priced.length, rows,
    totalUsd: complete ? perPercent(String(total), percent, 12, 6) : null,
    totalFullUsd: complete ? perPercent(String(total * 100n), percent, 12, 6) : null,
    reason: intervals.find(interval => interval.categories.reason)?.categories.reason ?? "No comparable priced intervals",
  };
}
