import { useEffect, useRef, useState } from "react";
import { DetectedModel, Draft, FieldErrors, loadPricingPage, newDraft, pricingError, rateFields, rateLabels, savePrice, validateDraft } from "./pricing";
import "./pricing.css";

export function ModelPricing({ open, onClose }: { open: boolean; onClose: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [models, setModels] = useState<DetectedModel[]>([]);
  const [selected, setSelected] = useState("");
  const [drafts, setDrafts] = useState<Map<string, Draft>>(() => new Map());
  const [errors, setErrors] = useState<FieldErrors>({});
  const [focusErrors, setFocusErrors] = useState(0);
  const [failure, setFailure] = useState("");
  const [catalogError, setCatalogError] = useState("");
  const [status, setStatus] = useState("");
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const cursor = useRef<string | null>(null);
  const loadingRef = useRef(false);
  const savingRef = useRef(false);
  const model = models.find(item => item.model === selected);
  const draft = model ? drafts.get(model.model) ?? newDraft(model) : null;

  async function load(restart: boolean) {
    if (loadingRef.current) return;
    loadingRef.current = true;
    setLoading(true); setCatalogError("");
    if (restart) cursor.current = null;
    try {
      do {
        const page = await loadPricingPage(cursor.current);
        setModels(current => {
          const merged = new Map(current.map(item => [item.model, item]));
          for (const item of page.models) merged.set(item.model, item);
          return [...merged.values()];
        });
        setSelected(current => current || page.models[0]?.model || "");
        cursor.current = page.nextCursor;
      } while (cursor.current !== null);
    } catch (error) {
      setCatalogError(`${pricingError(error).message} The model list may be incomplete.`);
    } finally { loadingRef.current = false; setLoading(false); }
  }

  useEffect(() => {
    if (open) { dialog.current?.showModal(); void load(true); }
    else dialog.current?.close();
  }, [open]);
  useEffect(() => {
    if (!saving && Object.values(errors).some(Boolean)) dialog.current?.querySelector<HTMLElement>("[aria-invalid=true]")?.focus();
  }, [focusErrors, saving]);

  function update<K extends keyof Draft>(field: K, value: Draft[K]) {
    if (!model || !draft) return;
    setDrafts(current => new Map(current).set(model.model, { ...draft, [field]: value }));
    setErrors(current => ({ ...current, [field]: undefined }));
    setFailure(""); setStatus("");
  }

  async function submit(event: React.FormEvent) {
    event.preventDefault();
    if (!model || !draft || savingRef.current) return;
    const invalid = validateDraft(draft);
    setErrors(invalid); setFailure(""); setStatus("");
    if (Object.keys(invalid).length) { setFocusErrors(current => current + 1); setFailure("Check the highlighted prices. Your entries have been kept."); return; }
    savingRef.current = true; setSaving(true);
    try {
      const version = await savePrice(model, draft);
      setModels(current => current.map(item => item.model === model.model ? { ...item, latestPrice: version } : item));
      setDrafts(current => new Map(current).set(model.model, { ...draft, backfillBefore: false }));
      setStatus("Price saved. Eligible usage is being valued; existing priced history stays unchanged.");
    } catch (error) {
      const problem = pricingError(error);
      if (problem.field) { setErrors({ [problem.field]: problem.message }); setFocusErrors(current => current + 1); }
      setFailure(problem.message);
    } finally { savingRef.current = false; setSaving(false); }
  }

  return <dialog ref={dialog} className="model-pricing" aria-labelledby="pricing-title" onClose={onClose}>
    <header><h2 id="pricing-title">Model Pricing</h2><button type="button" onClick={() => dialog.current?.close()} autoFocus>Close</button></header>
    <p>Estimated token cost uses your USD prices per 1,000,000 tokens. Unpriced usage stays unknown; any known subtotal is incomplete.</p>
    <div className="model-picker">
      <label htmlFor="pricing-model">Detected model</label>
      <select id="pricing-model" value={selected} disabled={!models.length || saving} onChange={event => { setSelected(event.target.value); setErrors({}); setFailure(""); setStatus(""); }}>
        {!models.length && <option value="">No detected models yet</option>}
        {models.map(item => <option key={item.model} value={item.model}>{item.model}{item.latestPrice ? " — configured" : " — unpriced"}</option>)}
      </select>
      <button type="button" disabled={loading || saving} onClick={() => void load(true)}>Reload models</button>
    </div>
    <p className="coverage" role="status">{loading ? `Loading detected models… ${models.length} available` : `${models.length} detected model${models.length === 1 ? "" : "s"}`}</p>
    {catalogError && <div className="diagnostic" role="alert">{catalogError} <button type="button" disabled={loading || saving} onClick={() => void load(false)}>Retry loading</button></div>}
    {!models.length && !loading && <p>Models appear after local session metadata is detected. Reload to check for newly detected models.</p>}
    {model && draft && <form onSubmit={event => void submit(event)} noValidate>
      <fieldset disabled={saving}>
        <legend>Prices for {model.model}</legend>
        <p id="pricing-rate-help">Enter zero only for a category you intend to value at $0. Use up to six decimal places.</p>
        <div className="rate-grid">{rateFields.filter(field => field !== "reasoning").map(field => <div key={field}>
          <label htmlFor={`price-${field}`}>{rateLabels[field]} (USD / 1M)</label>
          <input id={`price-${field}`} type="text" inputMode="decimal" value={draft[field]} onChange={event => update(field, event.target.value)} aria-invalid={!!errors[field]} aria-describedby={`pricing-rate-help${errors[field] ? ` error-${field}` : ""}`} />
          {errors[field] && <p className="field-error" id={`error-${field}`}>{errors[field]}</p>}
        </div>)}</div>
        <label htmlFor="reasoning-policy">How should reasoning tokens be priced?</label>
        <select id="reasoning-policy" value={draft.reasoningPolicy} onChange={event => update("reasoningPolicy", event.target.value as Draft["reasoningPolicy"])} aria-describedby="reasoning-help">
          <option value="unknown">Unknown — leave affected usage unpriced</option>
          <option value="included">Included in output price</option>
          <option value="separate">Separate reasoning price</option>
        </select>
        <p id="reasoning-help">Reasoning is reported within output tokens. Included counts it once at the output price. Separate removes it from ordinary output and uses the reasoning price. Choose separate only when the model’s pricing calls for it.</p>
        {draft.reasoningPolicy === "separate" && <div>
          <label htmlFor="price-reasoning">Reasoning (USD / 1M)</label>
          <input id="price-reasoning" type="text" inputMode="decimal" value={draft.reasoning} onChange={event => update("reasoning", event.target.value)} aria-invalid={!!errors.reasoning} aria-describedby={`reasoning-help${errors.reasoning ? " error-reasoning" : ""}`} />
          {errors.reasoning && <p className="field-error" id="error-reasoning">{errors.reasoning}</p>}
        </div>}
        <label htmlFor="cache-policy">How do cache-write tokens relate to input?</label>
        <select id="cache-policy" value={draft.cacheWritePolicy} onChange={event => update("cacheWritePolicy", event.target.value as Draft["cacheWritePolicy"])} aria-describedby="cache-help">
          <option value="unknown">Unknown — leave nonzero cache-write usage unpriced</option>
          <option value="included_input_disjoint">Included in input, separate from cached input</option>
          <option value="additional">Additional to input</option>
        </select>
        <p id="cache-help">Cached input replaces the ordinary input price. Included cache writes also replace ordinary input and must not overlap cached input. Additional cache writes add to the input cost. Unknown keeps affected usage unpriced.</p>
        {!model.latestPrice ? <label className="backfill-option">
          <input type="checkbox" checked={draft.backfillBefore} onChange={event => update("backfillBefore", event.target.checked)} aria-invalid={!!errors.backfillBefore} aria-describedby={errors.backfillBefore ? "error-backfill" : undefined} />
          <span>Apply this first price to older unpriced usage for this model. Leaving this unchecked covers usage from the save time onward.</span>
        </label> : <p>This model already has a price. Saving creates a new version for usage from the save time onward. Previously priced usage is never recalculated.</p>}
        {errors.backfillBefore && <p className="field-error" id="error-backfill">{errors.backfillBefore} Reload models to see the current version.</p>}
        {model.latestPrice && <p className="coverage">Latest price effective {new Date(model.latestPrice.effectiveSeconds * 1000).toLocaleString()}.</p>}
      </fieldset>
      {failure && <p className="field-error" role="alert">{failure}</p>}
      <p role="status" aria-live="polite">{saving ? "Saving price…" : status}</p>
      <button type="submit" disabled={saving || loading}>{saving ? "Saving…" : "Save price"}</button>
    </form>}
  </dialog>;
}
