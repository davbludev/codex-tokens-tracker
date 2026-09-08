import { useEffect, useRef, useState } from "react";
import { Draft, FieldErrors, newDraft, pricingError, rateFields, rateLabels, savePrice, usePricingCatalog, validateDraft } from "./pricing";
import "./pricing.css";

export function ModelPricing({ open, onClose }: { open: boolean; onClose: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [selected, setSelected] = useState("");
  const [search, setSearch] = useState("");
  const [drafts, setDrafts] = useState<Map<string, Draft>>(() => new Map());
  const [errors, setErrors] = useState<FieldErrors>({});
  const [focusErrors, setFocusErrors] = useState(0);
  const [failure, setFailure] = useState("");
  const [status, setStatus] = useState("");
  const [saving, setSaving] = useState(false);
  const savingRef = useRef(false);
  const { models, loading, error: catalogError, connectionError, reload, updatePrice } = usePricingCatalog(open, saving);
  const model = models.find(item => item.model === selected);
  const draft = model ? drafts.get(model.model) ?? newDraft(model) : null;
  const filtered = models.filter(item => item.model.toLocaleLowerCase().includes(search.trim().toLocaleLowerCase()));

  useEffect(() => {
    if (open) dialog.current?.showModal();
    else dialog.current?.close();
  }, [open]);
  useEffect(() => { setSelected(current => current || models[0]?.model || ""); }, [models]);
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
      updatePrice(version);
      setDrafts(current => new Map(current).set(model.model, { ...draft, backfillBefore: false }));
      setStatus("Price saved. Eligible usage is being valued; existing priced history stays unchanged.");
    } catch (error) {
      const problem = pricingError(error);
      if (problem.field) { setErrors({ [problem.field]: problem.message }); setFocusErrors(current => current + 1); }
      setFailure(problem.message);
    } finally { savingRef.current = false; setSaving(false); }
  }

  return <dialog ref={dialog} className="model-pricing" aria-labelledby="pricing-title" onClose={onClose}>
    <header className="pricing-header"><div><span className="pricing-eyebrow">Cost estimates</span><h2 id="pricing-title">Model Pricing</h2></div><button type="button" onClick={() => dialog.current?.close()} autoFocus>Close</button></header>
    <p className="pricing-introduction">Set your USD rates per million tokens. Models appear automatically from your local session history.</p>
    <div className="pricing-workspace">
    <aside className="pricing-catalog" aria-label="Model selection">
      <div className="pricing-catalog-heading"><h3>Detected models</h3><span className="pricing-count">{models.length}</span></div>
      <label htmlFor="pricing-search" className="pricing-visually-hidden">Search detected models</label>
      <input id="pricing-search" type="search" placeholder="Search models…" value={search} onChange={event => setSearch(event.target.value)} />
      <ul className="pricing-models" aria-label="Detected models">
        {filtered.map(item => <li key={item.model}><button type="button" disabled={saving} aria-pressed={selected === item.model} onClick={() => { setSelected(item.model); setErrors({}); setFailure(""); setStatus(""); }}>
          <span>{item.model}</span><small className={item.latestPrice ? "pricing-configured" : "pricing-unpriced"}><i aria-hidden="true" />{item.latestPrice ? "Configured" : "Unpriced"}</small>
        </button></li>)}
      </ul>
      {!filtered.length && models.length > 0 && <p>No models match “{search}”.</p>}
      {!models.length && !loading && <p>No models detected yet. Check the monitored source in Settings.</p>}
      <p className="coverage" role="status">{loading ? `Loading detected models… ${models.length} available` : `${models.length} detected model${models.length === 1 ? "" : "s"}`}</p>
      <button className="pricing-reload" type="button" disabled={loading || saving} onClick={() => reload()}>Reload models</button>
      {catalogError && <div className="diagnostic" role="alert">{catalogError} <button type="button" disabled={loading || saving} onClick={() => reload(false)}>Retry loading</button></div>}
      {connectionError && <p className="coverage" role="status">{connectionError}</p>}
    </aside>
    <div className="pricing-editor">
    {model && draft ? <form onSubmit={event => void submit(event)} noValidate>
      <fieldset disabled={saving}>
        <legend>Prices for {model.model}</legend>
        <p id="pricing-rate-help">USD per 1,000,000 tokens, up to six decimal places. Enter zero only to value a category at $0.</p>
        <div className="rate-grid">{rateFields.filter(field => field !== "reasoning").map(field => <div key={field}>
          <label htmlFor={`price-${field}`}>{rateLabels[field]} (USD / 1M)</label>
          <input id={`price-${field}`} type="text" inputMode="decimal" value={draft[field]} onChange={event => update(field, event.target.value)} aria-invalid={!!errors[field]} aria-describedby={`pricing-rate-help${errors[field] ? ` error-${field}` : ""}`} />
          {errors[field] && <p className="field-error" id={`error-${field}`}>{errors[field]}</p>}
        </div>)}</div>
        <p id="pricing-formula">Estimate formula: (input − cached input) × input rate + cached input × cached rate + cache writes × write rate + output × output rate. Reasoning is already inside output. This monetary estimate assumes additional cache writes; it does not describe subscription quota rules. Compare token combinations on the dashboard without configuring prices.</p>
        {!model.latestPrice ? <label className="backfill-option">
          <input type="checkbox" checked={draft.backfillBefore} onChange={event => update("backfillBefore", event.target.checked)} aria-invalid={!!errors.backfillBefore} aria-describedby={errors.backfillBefore ? "error-backfill" : undefined} />
          <span>Apply this first price to older unpriced usage for this model. Leaving this unchecked covers usage from the save time onward.</span>
        </label> : <p>This model already has a price. Saving creates a new version for usage from the save time onward. Previously priced usage is never recalculated.</p>}
        {errors.backfillBefore && <p className="field-error" id="error-backfill">{errors.backfillBefore} Reload models to see the current version.</p>}
        {model.latestPrice && <p className="coverage">Latest price effective {new Date(model.latestPrice.effectiveSeconds * 1000).toLocaleString()}.</p>}
      </fieldset>
      {failure && <p className="field-error" role="alert">{failure}</p>}
      <p role="status" aria-live="polite">{saving ? "Saving price…" : status}</p>
      <div className="pricing-save"><button type="submit" disabled={saving || loading}>{saving ? "Saving…" : "Save price"}</button><span>Unpriced usage stays unknown.</span></div>
    </form> : <div className="pricing-empty"><h3>{loading ? "Finding your models" : "Your models will appear here"}</h3><p>Once the monitor reads a model from local session metadata, you can configure its prices here.</p></div>}
    </div>
    </div>
  </dialog>;
}
