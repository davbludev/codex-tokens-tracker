# Session details interface

## Dialog and selection
- Owner: `src/SessionDetail.tsx`; caller: `src/Sessions.tsx`.
- Responsibility: Retain one selected session, display its metadata and direct/inclusive scopes, manage modal focus and depth navigation.
- Look here when: Changing lifecycle availability, observed usage labels, or session drill-down.

## Live aggregate reads
- Owner: `src/sessions-data.ts`; symbols: `useSessionRead`, `categoryText`.
- Responsibility: Serialize reads per view, invalidate stale replies, reset live cursors, retain exact category text and coverage labels.
- Look here when: Changing paging, retry, notification fallback or cleanup.

## Relationships
- Owner: `src/SessionHierarchy.tsx`.
- Responsibility: Display one bounded children page and one ID-ordered ancestor page with readiness and placeholders.
- Look here when: Changing effective hierarchy navigation.

## Model usage
- Owner: `src/SessionModels.tsx`.
- Responsibility: Display one direct model page, exact categories and backend cost shares alongside a uPlot chart.
- Look here when: Changing model pagination or incomplete cost presentation.

## Timeline
- Owner: `src/SessionTimeline.tsx`.
- Responsibility: Render bounded direct usage with independent token/USD scales and connections, exact keyboard inspection and coverage disclosure.
- Look here when: Changing timeline gaps, untimed observations or chart cleanup.

## Presentation and browser validation
- Owner: `src/sessions.css`; checks: `tests/sessions-ui.mjs`.
- Responsibility: Compact responsive session surfaces and browser workflows against mocked aggregate transport.
- Look here when: Checking navigation, paging, chart paths, focus or stale/live delivery.
