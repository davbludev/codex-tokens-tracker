import type { CategoryCosts, QuotaInterval } from "./dashboard-types";
import { perPercent, sumPercentages } from "./quota-combinations";

export type CategoryKey = Exclude<keyof CategoryCosts, "reason">;
/** Fixed order and hues; adjacent pairs pass the color-vision separation check on the dark surface. */
export const categories: { key: CategoryKey; label: string; color: string }[] = [
  { key: "input", label: "Uncached input", color: "#80b7ff" },
  { key: "cachedInput", label: "Cached input", color: "#edbe74" },
  { key: "cacheWrites", label: "Cache writes", color: "#c1a0ff" },
  { key: "output", label: "Output", color: "#65d8ad" },
];

/**
 * Percentages of `total` rounded to one decimal by largest remainder, so the
 * displayed shares add up to exactly 100 instead of 99.9 or 100.1.
 */
export function percentShares(parts: bigint[], total: bigint): (number | null)[] {
  if (total <= 0n) return parts.map(() => null);
  const scaled = parts.map(part => (part * 1000n) / total);
  const remainders = parts.map((part, index) => ({ index, rest: part * 1000n - scaled[index] * total }));
  let left = 1000n - scaled.reduce((sum, value) => sum + value, 0n);
  for (const { index } of remainders.sort((a, b) => (a.rest === b.rest ? a.index - b.index : a.rest < b.rest ? 1 : -1))) {
    if (left <= 0n) break;
    scaled[index] += 1n;
    left -= 1n;
  }
  return scaled.map(value => Number(value) / 10);
}

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
  // Share is display-only; exact amounts remain in the USD strings.
  const shares = available ? percentShares(sums, total) : categories.map(() => null);
  const rows = categories.map((category, index) => ({
    ...category,
    usd: available ? perPercent(String(sums[index]), percent, 12, 6) : null,
    fullUsd: available ? perPercent(String(sums[index] * 100n), percent, 12, 6) : null,
    share: shares[index],
  }));
  return {
    count: priced.length, withoutUsage, excluded, rows,
    totalUsd: available ? perPercent(String(total), percent, 12, 6) : null,
    totalFullUsd: available ? perPercent(String(total * 100n), percent, 12, 6) : null,
    reason: intervals.find(interval => interval.categories.reason && interval.categories.reason !== NO_LOCAL_USAGE)?.categories.reason
      ?? intervals.find(interval => interval.categories.reason)?.categories.reason ?? "No comparable priced intervals",
  };
}
