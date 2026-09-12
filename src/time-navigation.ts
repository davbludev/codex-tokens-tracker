import type uPlot from "uplot";
import type { ObservationTime, TimeWindow } from "./dashboard-types";

export function fromMilliseconds(ms: number): ObservationTime {
  const rounded = Math.round(ms);
  return { seconds: Math.floor(rounded / 1000), nanos: ((rounded % 1000 + 1000) % 1000) * 1e6 };
}
export function milliseconds(time: ObservationTime): number { return time.seconds * 1000 + time.nanos / 1e6; }

/** All gestures commit a shared native query, never a chart-only zoom. */
export function attachTimeNavigation(plot: uPlot, select: (window: TimeWindow) => void) {
  const over = plot.over;
  let gesture: { id: number; start: number; last: number; pan: boolean; min: number; max: number } | null = null;
  let wheelWindow: [number, number] | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const x = (event: { clientX: number }) => Math.max(0, Math.min(over.clientWidth, event.clientX - over.getBoundingClientRect().left));
  const hide = () => plot.setSelect({ left: 0, top: 0, width: 0, height: 0 }, false);
  const commit = (min: number, max: number) => {
    let first = Math.min(min, max) * 1000, last = Math.max(min, max) * 1000;
    if (!Number.isFinite(first) || !Number.isFinite(last) || last - first < 1) return;
    const shift = Math.max(0, last - Date.now()); first -= shift; last -= shift;
    select({ start: fromMilliseconds(first), end: fromMilliseconds(last) });
  };
  const cancel = () => { if (gesture && over.hasPointerCapture(gesture.id)) over.releasePointerCapture(gesture.id); gesture = null; over.classList.remove("is-panning"); hide(); };
  const down = (event: PointerEvent) => {
    if (event.button !== 0 || event.ctrlKey) return;
    event.preventDefault();
    gesture = { id: event.pointerId, start: x(event), last: x(event), pan: event.shiftKey, min: plot.scales.x.min!, max: plot.scales.x.max! };
    over.setPointerCapture(event.pointerId);
    over.classList.toggle("is-panning", event.shiftKey);
  };
  const move = (event: PointerEvent) => {
    if (!gesture) return;
    gesture.last = x(event);
    if (!gesture.pan) plot.setSelect({ left: Math.min(gesture.start, gesture.last), top: 0, width: Math.abs(gesture.last - gesture.start), height: over.clientHeight }, false);
  };
  const up = (event: PointerEvent) => {
    if (!gesture || gesture.id !== event.pointerId) return;
    const value = gesture; value.last = x(event); cancel();
    if (Math.abs(value.last - value.start) < 5) return;
    if (value.pan) { const shift = (value.start - value.last) / over.clientWidth * (value.max - value.min); commit(value.min + shift, value.max + shift); }
    else commit(plot.posToVal(value.start, "x"), plot.posToVal(value.last, "x"));
  };
  const wheel = (event: WheelEvent) => {
    if (!event.ctrlKey) return;
    event.preventDefault();
    const [min, max] = wheelWindow ?? [plot.scales.x.min!, plot.scales.x.max!];
    const at = min + x(event) / over.clientWidth * (max - min);
    const factor = Math.exp(Math.max(-200, Math.min(200, event.deltaY)) * .002);
    wheelWindow = [at - (at - min) * factor, at + (max - at) * factor];
    clearTimeout(timer);
    timer = setTimeout(() => { if (wheelWindow) commit(...wheelWindow); wheelWindow = null; }, 150);
  };
  const key = (event: KeyboardEvent) => { if (event.key === "Escape") { cancel(); clearTimeout(timer); wheelWindow = null; } };
  over.classList.add("time-navigable");
  over.addEventListener("pointerdown", down);
  over.addEventListener("pointermove", move);
  over.addEventListener("pointerup", up);
  over.addEventListener("pointercancel", cancel);
  over.addEventListener("wheel", wheel, { passive: false });
  document.addEventListener("keydown", key);
  return () => { clearTimeout(timer); over.removeEventListener("pointerdown", down); over.removeEventListener("pointermove", move); over.removeEventListener("pointerup", up); over.removeEventListener("pointercancel", cancel); over.removeEventListener("wheel", wheel); document.removeEventListener("keydown", key); };
}
