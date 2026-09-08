import { invoke } from "@tauri-apps/api/core";

export type SettingsDraft = {
  codex_directory_override: string | null;
  autostart: boolean;
  tray_enabled: boolean;
  close_to_tray: boolean;
};
export type TrackerSettings = SettingsDraft & {
  automatic_directory: string | null;
  monitored_directory: string | null;
};
export type TrackerDiagnostics = {
  database_path: string;
  database_size_bytes: number;
  tracked_session_count: number;
  usage_record_count: number;
  monitored_directory: string | null;
  last_successful_ingestion_at_ms: number | null;
  source_available: boolean;
  error: string | null;
};
export const exportKinds = [
  ["sessions", "Sessions", "Direct session usage with estimated token costs; descendants are counted in their own rows."],
  ["model_usage", "Model usage", "All locally observed direct usage and estimated token costs grouped by model."],
  ["weekly_cycles", "Weekly cycles", "Completed and current observed cycles. Tokens and costs cover each comparable observation interval; newer unmatched usage is excluded."],
  ["project_totals", "Project totals", "All locally observed direct usage and estimated token costs grouped by project."],
] as const;
export type ExportKind = typeof exportKinds[number][0];
export type ExportResult = { path: string; row_count: number };

export function loadSettings() { return invoke<TrackerSettings>("tracker_settings"); }
export function saveSettings(settings: SettingsDraft) { return invoke<TrackerSettings>("save_tracker_settings", { settings }); }
export function loadDiagnostics() { return invoke<TrackerDiagnostics>("tracker_diagnostics"); }
export function exportCsv(kind: ExportKind, destination: string) { return invoke<ExportResult>("export_usage_csv", { request: { kind, destination } }); }

export function isAbsolutePath(path: string) {
  return /^(?:[a-z]:[\\/]|\\\\[^\\]+\\[^\\]+|\/)/i.test(path);
}

export function settingsError(error: unknown): { code: string; message: string } {
  const code = typeof error === "string" ? error : typeof error === "object" && error !== null && "code" in error && typeof error.code === "string" ? error.code : "unknown";
  const messages: Record<string, string> = {
    invalid_directory: "Choose an existing, readable Codex home directory containing your session history.",
    invalid_destination: "Choose an absolute .csv file path in an existing, writable folder.",
    destination_exists: "A file already exists at that path. Choose a new filename.",
    busy: "Another operation is in progress. Try again when it finishes.",
    unavailable: "The desktop service is unavailable. Restart the monitor and try again.",
    storage: "The local database could not complete this operation. Check the database location and try again.",
    io: "The file could not be written. Check the folder and its permissions, then try again.",
    write: "The file could not be written. Check the folder and its permissions, then try again.",
  };
  return { code, message: messages[code] ?? "The desktop operation could not be completed. Your entries have been kept; try again." };
}
