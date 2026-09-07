import { invoke } from "@tauri-apps/api/core";

export type PriceInput = {
  input: string; cachedInput: string; cacheWrite: string; output: string;
  reasoning: string | null;
  reasoningPolicy: "unknown" | "included" | "separate";
  cacheWritePolicy: "unknown" | "included_input_disjoint" | "additional";
};
export type PriceVersion = {
  id: number; model: string; effectiveSeconds: number; effectiveNanos: number;
  backfillBefore: boolean; configuration: PriceInput;
};
export type DetectedModel = { model: string; latestPrice: PriceVersion | null };
export type PricingPage = { models: DetectedModel[]; nextCursor: string | null };
export type Draft = Omit<PriceInput, "reasoning"> & { reasoning: string; backfillBefore: boolean };
export type FieldErrors = Partial<Record<keyof Draft, string>>;

export const rateFields = ["input", "cachedInput", "cacheWrite", "output", "reasoning"] as const;
export const rateLabels = { input: "Input", cachedInput: "Cached input", cacheWrite: "Cache write", output: "Output", reasoning: "Reasoning" };

export function newDraft(model: DetectedModel): Draft {
  return { input: "", cachedInput: "", cacheWrite: "", output: "", reasoningPolicy: "unknown", cacheWritePolicy: "unknown",
    ...model.latestPrice?.configuration, reasoning: model.latestPrice?.configuration.reasoning ?? "", backfillBefore: false };
}

export function validateDraft(draft: Draft): FieldErrors {
  const errors: FieldErrors = {};
  for (const field of rateFields) {
    if (field === "reasoning" && draft.reasoningPolicy !== "separate") continue;
    const value = draft[field];
    if (!/^\d+(\.\d{1,6})?$/.test(value)) {
      errors[field] = "Enter a nonnegative decimal, such as 1.25, with up to six decimal places.";
      continue;
    }
    const [whole, fraction = ""] = value.split(".");
    if (BigInt(whole + fraction.padEnd(6, "0")) > 9223372036854775807n) {
      errors[field] = "Enter a price no greater than 9223372036854.775807.";
    }
  }
  return errors;
}

export function priceConfiguration(draft: Draft): PriceInput {
  const { backfillBefore: _backfill, ...configuration } = draft;
  return { ...configuration, reasoning: draft.reasoningPolicy === "separate" ? draft.reasoning : null };
}

export function pricingError(error: unknown): { message: string; field?: keyof Draft } {
  if (typeof error === "object" && error !== null && "message" in error && typeof error.message === "string") {
    const field = "field" in error && typeof error.field === "string" && [...rateFields, "backfillBefore"].includes(error.field as typeof rateFields[number])
      ? error.field as keyof Draft : undefined;
    return { message: error.message, field };
  }
  return { message: "Pricing could not connect. Reload models before retrying a save." };
}

export const loadPricingPage = (after: string | null) => invoke<PricingPage>("pricing_models", { after });
export const savePrice = (model: DetectedModel, draft: Draft) => invoke<PriceVersion>("save_model_price", {
  model: model.model, configuration: priceConfiguration(draft), backfillBefore: !model.latestPrice && draft.backfillBefore,
});
