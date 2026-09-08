# Desktop monitoring

A normal launch opens the main window and starts monitoring. Settings → Startup
& tray preferences controls three persisted, opt-in choices:

- **Start when I sign in** registers the current executable with the operating
  system through Tauri's autostart plugin. Disabling it removes that registration.
  A moved portable executable must be registered again from its new location.
- **Enable system tray** adds the monitor icon and its Open, Pause monitoring /
  Resume monitoring, and Exit actions. Left-clicking the icon opens the window.
- **Minimize or close window to tray** hides the main window when its close button
  is used, or when it is minimized on Windows.
  Monitoring continues. This requires an active tray; removing the tray first
  reveals the main window. Without this option, normal window minimization also
  preserves monitoring.

The tray shows the last observed core weekly percentage, estimated USD for the
comparable observation interval, and observed ~USD per percentage point. Menu
rows retain exact monetary values, incomplete known subtotals, unavailable
reasons, the observation's UTC timestamp, and newer unmatched cost separately.
The observations cover local usage since observation began, not complete account
usage. The compact tooltip uses shorter values; the menu supplies full detail.
Updates are coalesced while data changes; idle monitoring does not poll the database.

Pause takes effect between atomic ingestion batches. Source events remain bounded;
resume recovers available source files from durable checkpoints even if events
were missed or overflowed while paused. A source change while paused stays paused.
Pause is temporary: reopening the application starts monitoring again.

Exit stops at an atomic batch boundary, lets any active CSV export finish, closes
the writer, and checkpoints SQLite before the process exits. New exports are
rejected once Exit begins. It does not wait for the entire historical backlog:
the next launch resumes it idempotently. Desktop preference errors are reported
without claiming a successful save; failed saves attempt to restore the previous
OS registration and tray state. No PC restart or shutdown is needed to use these
controls.
