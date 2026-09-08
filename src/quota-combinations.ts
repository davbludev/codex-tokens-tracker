import type { QuotaInterval } from "./dashboard-types";

export type WriteInterpretation = "included" | "additional";
const optional = ["Cached input", "Cache writes", "Reasoning"];
const names = ["Baseline", "+ Cache read", "+ Cache write", "+ Cache read + write", "+ Reasoning", "+ Cache read + reasoning", "+ Cache write + reasoning", "+ All three"];
const colors = ["#80b7ff", "#65d8ad", "#edbe74", "#c1a0ff", "#ef94ac", "#86dce5", "#e9e38a", "#f6a476"];
export const combinations = [0, 1, 2, 4, 3, 5, 6, 7].map(mask => ({ mask, name: names[mask], color: colors[mask], components: ["Input", "Output", ...optional.filter((_, bit) => mask & (1 << bit))] }));

/** Locale-grouped exact decimal string; null means unavailable. */
export function formatted(value: string | null, money = false) {
  if (value === null) return "Unavailable";
  const [whole, fraction] = value.split(".");
  const separator = (1.1).toLocaleString().replace(/\d/g, "");
  return (money ? "$" : "") + BigInt(whole).toLocaleString() + separator + fraction;
}
export function sample(interval: QuotaInterval, mask: number, writes: WriteInterpretation) {
  return interval.hypotheses.find(h => h.mask === mask && h.writesIncluded === (writes === "included"));
}
function decimalParts(value: string): { integer: bigint; scale: number } {
  const [mantissa, exponent = "0"] = value.toLowerCase().split("e");
  const [whole, fraction = ""] = mantissa.split(".");
  const scale = fraction.length - Number(exponent);
  const integer = BigInt(whole + fraction);
  return scale < 0 ? { integer: integer * 10n ** BigInt(-scale), scale: 0 } : { integer, scale };
}
/** Exact decimal sum of percentage-point strings, as `<integer>e-<scale>`. */
export function sumPercentages(values: string[]): string {
  const parts = values.map(decimalParts);
  const scale = Math.max(0, ...parts.map(p => p.scale));
  return `${parts.reduce((total, p) => total + p.integer * 10n ** BigInt(scale - p.scale), 0n)}e-${scale}`;
}
/** Exact arithmetic until rounding to the requested display precision. */
export function perPercent(units: string, percent: string, unitScale = 0, digits = 2): string {
  const { integer, scale } = decimalParts(percent);
  const numerator = BigInt(units) * 10n ** BigInt(scale + digits);
  const denominator = integer * 10n ** BigInt(unitScale);
  const rounded = (numerator * 2n + denominator) / (denominator * 2n);
  const factor = 10n ** BigInt(digits);
  return `${rounded / factor}.${String(rounded % factor).padStart(digits, "0")}`;
}
export function combinationStats(intervals: QuotaInterval[], mask: number, writes: WriteInterpretation) {
  const samples = intervals.map(interval => ({ interval, value: sample(interval, mask, writes) }));
  const usable = samples.filter(s => s.value?.tokens != null);
  const priced = usable.filter(s => s.value?.estimatedUsd != null);
  const percent = sumPercentages(usable.map(s => s.interval.consumedPercentagePoints));
  const tokens = usable.reduce((sum, s) => sum + BigInt(s.value!.tokens!), 0n).toString();
  // Never present a priced subset as the monetary result for the whole scope.
  const complete = usable.length > 0 && priced.length === usable.length;
  const amount = priced.reduce((sum, s) => sum + BigInt(s.value!.estimatedUsd!), 0n);
  const ratios = usable.map(s => Number(perPercent(s.value!.tokens!, s.interval.consumedPercentagePoints)));
  const mean = ratios.reduce((sum, value) => sum + value, 0) / ratios.length;
  return {
    count: usable.length, pricedCount: priced.length,
    tokens: usable.length ? perPercent(tokens, percent) : null,
    usd: complete ? perPercent(String(amount), percent, 12, 6) : null,
    fullUsd: complete ? perPercent(String(amount * 100n), percent, 12, 6) : null,
    variation: ratios.length >= 3 && mean > 0 ? Math.sqrt(ratios.reduce((sum, value) => sum + (value - mean) ** 2, 0) / ratios.length) / mean * 100 : null,
    tokenReason: samples.find(s => s.value?.tokenReason)?.value?.tokenReason ?? "No comparable local intervals",
    priceReason: samples.find(s => s.value?.priceReason)?.value?.priceReason ?? "No comparable priced intervals",
  };
}
