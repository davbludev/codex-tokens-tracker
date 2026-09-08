import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useRef, useState } from "react";

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

/** Retain exact detected IDs and partial pages; invalidations always restart keyset paging. */
export function usePricingCatalog(active: boolean, saving = false) {
  const [models, setModels] = useState<DetectedModel[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [connectionError, setConnectionError] = useState("");
  const blocked = useRef(saving);
  blocked.current = saving;
  const request = useRef<(restart?: boolean) => void>(() => {});
  const resume = useRef<() => void>(() => {});
  useEffect(() => {
    if (!active) return;
    let disposed = false, running = false, pending = false, pendingRestart = false;
    let cursor: string | null = null;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let stop: (() => void) | undefined;
    async function read(restart = true) {
      if (disposed) return;
      if (running || blocked.current) { pending = true; pendingRestart ||= restart; return; }
      running = true;
      if (restart) cursor = null;
      setLoading(true);
      try {
        do {
          const page = await loadPricingPage(cursor);
          if (disposed) return;
          setModels(current => {
            const merged = new Map(current.map(model => [model.model, model]));
            for (const model of page.models) merged.set(model.model, model);
            return [...merged.values()].sort((a, b) => a.model < b.model ? -1 : a.model > b.model ? 1 : 0);
          });
          cursor = page.nextCursor;
        } while (cursor !== null);
        setError("");
      } catch (failure) {
        if (!disposed) setError(`${pricingError(failure).message} The model list may be incomplete.`);
      } finally {
        running = false;
        if (!disposed) { setLoading(false); resume.current(); }
      }
    }
    resume.current = () => {
      if (!pending || running || blocked.current || disposed) return;
      const restart = pendingRestart;
      pending = false; pendingRestart = false;
      void read(restart);
    };
    request.current = (restart = true) => { clearTimeout(timer); timer = undefined; void read(restart); };
    void listen("usage-updated", () => {
      // Queue immediately during a read so an event cannot be lost at its last page.
      if (running || blocked.current) { pending = true; pendingRestart = true; return; }
      if (timer === undefined) timer = setTimeout(() => { timer = undefined; void read(); }, 150);
    }).then(unlisten => { if (disposed) unlisten(); else { stop = unlisten; setConnectionError(""); } })
      .catch(() => { if (!disposed) setConnectionError("Live model updates unavailable; refreshing every minute."); });
    void read();
    const interval = setInterval(() => void read(), 60_000);
    return () => {
      disposed = true; clearTimeout(timer); clearInterval(interval); stop?.();
      request.current = () => {}; resume.current = () => {};
    };
  }, [active]);
  useEffect(() => { if (!saving) resume.current(); }, [saving]);
  return { models, loading, error, connectionError, reload: (restart = true) => request.current(restart),
    updatePrice: (version: PriceVersion) => setModels(current => current.map(model => model.model === version.model ? { ...model, latestPrice: version } : model)) };
}

export const savePrice = (model: DetectedModel, draft: Draft) => invoke<PriceVersion>("save_model_price", {
  model: model.model, configuration: priceConfiguration(draft), backfillBefore: !model.latestPrice && draft.backfillBefore,
});
