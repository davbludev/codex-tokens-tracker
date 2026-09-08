import { useEffect, useRef, useState } from "react";
import { exportCsv, exportKinds, isAbsolutePath, loadDiagnostics, loadSettings, saveSettings, settingsError } from "./settings-data";
import type { ExportKind, ExportResult, SettingsDraft, TrackerDiagnostics, TrackerSettings } from "./settings-data";
import "./settings.css";

function Diagnostics({ revision }: { revision: number }) {
  const [diagnostics, setDiagnostics] = useState<TrackerDiagnostics | null>(null);
  const [reload, setReload] = useState(0);
  const [loading, setLoading] = useState(true);
  const [failure, setFailure] = useState("");
  useEffect(() => {
    let disposed = false;
    setLoading(true); setFailure("");
    void loadDiagnostics().then(result => { if (!disposed) setDiagnostics(result); }).catch(error => {
      if (!disposed) setFailure(settingsError(error).message);
    }).finally(() => { if (!disposed) setLoading(false); });
    return () => { disposed = true; };
  }, [reload, revision]);
  return <section className="settings-section" aria-labelledby="diagnostics-title">
    <div className="settings-section-heading"><h3 id="diagnostics-title">Local database & diagnostics</h3><button type="button" disabled={loading} onClick={() => setReload(value => value + 1)}>{loading ? "Refreshing…" : "Refresh diagnostics"}</button></div>
    <p>Operational details only. Conversation text and prompts are not included.</p>
    {failure && <p className="field-error" role="alert">{failure}</p>}
    {loading && <p role="status">Loading diagnostics…</p>}
    {diagnostics && <>
      <dl className="settings-diagnostics">
        <dt>Database location</dt><dd>{diagnostics.database_path}</dd>
        <dt>Database size</dt><dd>{diagnostics.database_size_bytes.toLocaleString()} bytes</dd>
        <dt>Tracked sessions</dt><dd>{diagnostics.tracked_session_count.toLocaleString()}</dd>
        <dt>Usage records</dt><dd>{diagnostics.usage_record_count.toLocaleString()}</dd>
        <dt>Monitored Codex home</dt><dd>{diagnostics.monitored_directory ?? "No directory selected"}</dd>
        <dt>Source status</dt><dd>{diagnostics.source_available ? "Available" : "Unavailable"}</dd>
        <dt>Last successful ingestion</dt><dd>{diagnostics.last_successful_ingestion_at_ms === null ? "No successful ingestion recorded" : new Date(diagnostics.last_successful_ingestion_at_ms).toLocaleString()}</dd>
        <dt>Ingestion error</dt><dd>{diagnostics.error ?? "None reported"}</dd>
      </dl>
      <p>The database stores local usage metadata, settings, and configured model prices. Prices are saved independently through Model Pricing.</p>
    </>}
  </section>;
}

function CsvExport() {
  const [kind, setKind] = useState<ExportKind>("sessions");
  const [destination, setDestination] = useState("");
  const [pathError, setPathError] = useState("");
  const [failure, setFailure] = useState("");
  const [result, setResult] = useState<ExportResult | null>(null);
  const [pending, setPending] = useState(false);
  const pendingRef = useRef(false);
  const pathInput = useRef<HTMLInputElement>(null);
  useEffect(() => { if (!pending && pathError) pathInput.current?.focus(); }, [pending, pathError]);
  function clearFeedback() { setPathError(""); setFailure(""); setResult(null); }
  async function submit(event: React.FormEvent) {
    event.preventDefault();
    if (pendingRef.current) return;
    clearFeedback();
    const path = destination.trim();
    if (!isAbsolutePath(path) || !/\.csv$/i.test(path)) { setPathError("Enter a full .csv file path, such as C:\\Exports\\sessions.csv."); pathInput.current?.focus(); return; }
    pendingRef.current = true; setPending(true);
    try { setResult(await exportCsv(kind, path)); }
    catch (error) {
      const problem = settingsError(error);
      if (["invalid_destination", "destination_exists", "io", "write"].includes(problem.code)) { setPathError(problem.message); pathInput.current?.focus(); }
      setFailure(problem.message);
    } finally { pendingRef.current = false; setPending(false); }
  }
  return <section className="settings-section" aria-labelledby="export-title">
    <h3 id="export-title">CSV exports</h3>
    <p>Export locally observed usage metadata. Estimated token costs use saved prices; unpriced usage remains unknown.</p>
    <form onSubmit={event => void submit(event)} noValidate>
      <fieldset disabled={pending}>
        <legend className="settings-visually-hidden">Export options</legend>
        <div className="settings-export-fields"><div>
          <label htmlFor="export-kind">Data to export</label>
          <select id="export-kind" value={kind} onChange={event => { setKind(event.target.value as ExportKind); clearFeedback(); }} aria-describedby="export-kind-help">{exportKinds.map(([value, label]) => <option key={value} value={value}>{label}</option>)}</select>
        </div><div>
          <label htmlFor="export-destination">Destination CSV file</label>
          <input ref={pathInput} id="export-destination" type="text" value={destination} placeholder="C:\Exports\sessions.csv" onChange={event => { setDestination(event.target.value); clearFeedback(); }} aria-invalid={!!pathError} aria-describedby={`export-path-help${pathError ? " export-path-error" : ""}`} />
          {pathError && <p className="field-error" id="export-path-error" role="alert">{pathError}</p>}
        </div></div>
        <p id="export-kind-help">{exportKinds.find(([value]) => value === kind)?.[2]}</p>
        <p id="export-path-help">Enter an absolute .csv path in an existing folder. Choose a new filename for each export.</p>
      </fieldset>
      {failure && <p className="field-error" role="alert">{failure}</p>}
      <div className="settings-actions"><button type="submit" disabled={pending}>{pending ? "Exporting…" : "Export CSV"}</button><p role="status" aria-live="polite">{pending ? "Writing CSV…" : result ? `Exported ${result.row_count.toLocaleString()} rows to ${result.path}` : ""}</p></div>
    </form>
  </section>;
}

export function Settings({ onOpenPricing }: { onOpenPricing: () => void }) {
  const [saved, setSaved] = useState<TrackerSettings | null>(null);
  const [draft, setDraft] = useState<SettingsDraft | null>(null);
  const [customDirectory, setCustomDirectory] = useState(false);
  const [directory, setDirectory] = useState("");
  const [directoryError, setDirectoryError] = useState("");
  const [failure, setFailure] = useState("");
  const [loadFailure, setLoadFailure] = useState("");
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [status, setStatus] = useState("");
  const [reload, setReload] = useState(0);
  const [revision, setRevision] = useState(0);
  const savingRef = useRef(false);
  const directoryInput = useRef<HTMLInputElement>(null);
  useEffect(() => { if (!saving && directoryError) directoryInput.current?.focus(); }, [saving, directoryError]);
  useEffect(() => {
    let disposed = false;
    setLoading(true); setLoadFailure("");
    void loadSettings().then(settings => {
      if (disposed) return;
      setSaved(settings); setDraft(settings); setCustomDirectory(settings.codex_directory_override !== null); setDirectory(settings.codex_directory_override ?? "");
    }).catch(error => { if (!disposed) setLoadFailure(settingsError(error).message); })
      .finally(() => { if (!disposed) setLoading(false); });
    return () => { disposed = true; };
  }, [reload]);
  function clearFeedback() { setDirectoryError(""); setFailure(""); setStatus(""); }
  function updatePreference(field: "autostart" | "tray_enabled" | "close_to_tray", checked: boolean) {
    clearFeedback();
    setDraft(current => current ? { ...current, [field]: checked, ...(field === "tray_enabled" && !checked ? { close_to_tray: false } : {}) } : current);
  }
  async function submit(event: React.FormEvent) {
    event.preventDefault();
    if (!draft || savingRef.current) return;
    clearFeedback();
    const path = directory.trim();
    if (customDirectory && !isAbsolutePath(path)) { setDirectoryError("Enter the full path to your Codex home directory, such as C:\\Users\\you\\.codex."); directoryInput.current?.focus(); return; }
    savingRef.current = true; setSaving(true);
    try {
      const settings = await saveSettings({ codex_directory_override: customDirectory ? path : null, autostart: draft.autostart, tray_enabled: draft.tray_enabled, close_to_tray: draft.tray_enabled && draft.close_to_tray });
      setSaved(settings); setDraft(settings); setDirectory(settings.codex_directory_override ?? ""); setCustomDirectory(settings.codex_directory_override !== null);
      setRevision(value => value + 1); setStatus("Settings saved. The monitor uses the selected Codex home. Startup and tray preferences are saved for a future release.");
    } catch (error) {
      const problem = settingsError(error);
      if (problem.code === "invalid_directory") { setDirectoryError(problem.message); directoryInput.current?.focus(); }
      setFailure(problem.message);
    } finally { savingRef.current = false; setSaving(false); }
  }
  return <section className="settings" aria-labelledby="settings-title">
    <h2 id="settings-title">Settings</h2>
    {loading && <p role="status">Loading settings…</p>}
    {loadFailure && <p className="field-error" role="alert">{loadFailure} <button type="button" disabled={loading} onClick={() => setReload(value => value + 1)}>Retry settings</button></p>}
    {saved && draft && <form onSubmit={event => void submit(event)} noValidate>
      <fieldset className="settings-section" disabled={saving}>
        <legend>Codex session source</legend>
        <p>Choose the Codex home directory containing your local session history. A saved override takes effect while the monitor is running.</p>
        <div className="settings-source-options">
          <label className="settings-option"><input type="radio" name="codex-source" checked={!customDirectory} onChange={() => { setCustomDirectory(false); clearFeedback(); }} /><span><strong>Discover automatically</strong><small>Uses CODEX_HOME when configured, otherwise your user’s .codex directory.</small></span></label>
          <label className="settings-option"><input type="radio" name="codex-source" checked={customDirectory} onChange={() => { setCustomDirectory(true); clearFeedback(); }} /><span><strong>Use a custom Codex home</strong><small>Override automatic discovery with a local directory.</small></span></label>
        </div>
        {customDirectory && <div className="settings-directory">
          <label htmlFor="codex-directory">Codex home directory</label>
          <input ref={directoryInput} id="codex-directory" type="text" value={directory} onChange={event => { setDirectory(event.target.value); clearFeedback(); }} placeholder="C:\Users\you\.codex" aria-invalid={!!directoryError} aria-describedby={`codex-directory-help${directoryError ? " codex-directory-error" : ""}`} />
          <p id="codex-directory-help">Choose the home folder that contains sessions, not an individual session file.</p>
          {directoryError && <p className="field-error" id="codex-directory-error" role="alert">{directoryError}</p>}
        </div>}
        <dl className="settings-source-paths"><dt>Automatic discovery</dt><dd>{saved.automatic_directory ?? "No automatic directory available"}</dd><dt>Currently monitored</dt><dd>{saved.monitored_directory ?? "No directory selected"}</dd></dl>
      </fieldset>
      <fieldset className="settings-section" disabled={saving} aria-describedby="startup-help">
        <legend>Startup & tray preferences</legend>
        <p id="startup-help">These preferences are saved only. Automatic startup, tray controls, and closing to the tray will become available with desktop integration in a future release.</p>
        <div className="settings-preferences">
          <label className="settings-option"><input type="checkbox" checked={draft.autostart} onChange={event => updatePreference("autostart", event.target.checked)} /><span><strong>Start when I sign in</strong><small>Future preference</small></span></label>
          <label className="settings-option"><input type="checkbox" checked={draft.tray_enabled} onChange={event => updatePreference("tray_enabled", event.target.checked)} /><span><strong>Enable system tray</strong><small>Future preference</small></span></label>
          <label className={`settings-option${!draft.tray_enabled ? " settings-option-disabled" : ""}`}><input type="checkbox" checked={draft.close_to_tray} disabled={!draft.tray_enabled} onChange={event => updatePreference("close_to_tray", event.target.checked)} /><span><strong>Close window to tray</strong><small>Requires the system tray preference</small></span></label>
        </div>
      </fieldset>
      {failure && <p className="field-error" role="alert">{failure}</p>}
      <div className="settings-actions"><button type="submit" disabled={saving || loading}>{saving ? "Saving…" : "Save settings"}</button><p role="status" aria-live="polite">{saving ? "Saving settings…" : status}</p></div>
    </form>}
    <section className="settings-section settings-pricing" aria-labelledby="settings-pricing-title"><div><h3 id="settings-pricing-title">Model Pricing</h3><p>Configure USD prices per million tokens for detected models. Existing priced history is preserved.</p></div><button type="button" onClick={onOpenPricing}>Configure model prices</button></section>
    <Diagnostics revision={revision} />
    <CsvExport />
  </section>;
}
