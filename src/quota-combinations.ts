import type { QuotaInterval } from "./dashboard-types";

export type WriteInterpretation = "included" | "additional";
export const components = ["Uncached input", "Cached input", "Cache write", "Visible output", "Reasoning"];
export const combinations = Array.from({ length: 31 }, (_, i) => ({
  mask: i + 1, label: components.filter((_, bit) => (i + 1) & (1 << bit)).join(" + "),
}));

/** Cancel overlapping subsets before requiring counters; output as reported
 * remains usable even when its reasoning breakdown is unavailable. */
export function combinationTokens(interval: QuotaInterval, mask: number, writes: WriteInterpretation): bigint | null {
  if (writes === "included" && (mask & 1)) {
    const { inputTokens: input, cachedInputTokens: cached, cacheWriteTokens: write } = interval.tokens;
    if ([input, cached, write].every(c => c.complete && c.knownTokens !== null) && BigInt(input.knownTokens!) < BigInt(cached.knownTokens!) + BigInt(write.knownTokens!)) return null;
  }
  const selected = (bit: number) => Number(Boolean(mask & (1 << bit)));
  const coefficients = {
    inputTokens: selected(0),
    cachedInputTokens: selected(1) - selected(0),
    cacheWriteTokens: selected(2) - (writes === "included" ? selected(0) : 0),
    outputTokens: selected(3),
    reasoningTokens: selected(4) - selected(3),
  };
  let total = 0n;
  for (const [name, coefficient] of Object.entries(coefficients)) {
    if (!coefficient) continue;
    const category = interval.tokens[name as keyof typeof coefficients];
    if (!category.complete || category.knownTokens === null) return null;
    total += BigInt(category.knownTokens) * BigInt(coefficient);
  }
  return total < 0n ? null : total;
}

// Native percentages are bounded decimal strings, potentially exponential.
function decimalParts(value: string): { integer: bigint; scale: number } {
  const [mantissa, exponent = "0"] = value.toLowerCase().split("e");
  const [whole, fraction = ""] = mantissa.split(".");
  const scale = fraction.length - Number(exponent);
  const integer = BigInt(whole + fraction);
  return scale < 0 ? { integer: integer * 10n ** BigInt(-scale), scale: 0 } : { integer, scale };
}
function sumPercentages(values: string[]): string {
  const parts = values.map(decimalParts);
  const scale = Math.max(0, ...parts.map(p => p.scale));
  const sum = parts.reduce((total, p) => total + p.integer * 10n ** BigInt(scale - p.scale), 0n);
  return `${sum}e-${scale}`;
}
/** Exact integer arithmetic up to the final display rounding (two decimals). */
export function tokensPerPercent(tokens: bigint, percent: string): string {
  const { integer, scale } = decimalParts(percent);
  const numerator = tokens * 10n ** BigInt(scale) * 100n;
  const rounded = (numerator * 2n + integer) / (integer * 2n);
  return `${rounded / 100n}.${String(rounded % 100n).padStart(2, "0")}`;
}
export function combinationStats(intervals: QuotaInterval[], mask: number, writes: WriteInterpretation) {
  const samples = intervals.flatMap(interval => {
    const tokens = combinationTokens(interval, mask, writes);
    return tokens === null ? [] : [{ tokens, percent: interval.consumedPercentagePoints, ratio: tokensPerPercent(tokens, interval.consumedPercentagePoints) }];
  });
  if (!samples.length) return { count: 0, ratio: null, min: null, max: null, variation: null };
  const values = samples.map(sample => Number(sample.ratio));
  const mean = values.reduce((sum, v) => sum + v, 0) / values.length;
  const variation = samples.length >= 3 && mean > 0 ? Math.sqrt(values.reduce((sum, v) => sum + (v - mean) ** 2, 0) / values.length) / mean * 100 : null;
  return { count: samples.length,
    ratio: tokensPerPercent(samples.reduce((sum, sample) => sum + sample.tokens, 0n), sumPercentages(samples.map(sample => sample.percent))),
    min: Math.min(...values), max: Math.max(...values), variation };
}
