import { Component, type ReactNode } from "react";

type Props = { children: ReactNode; scope?: string };
type Position = { element: Element; top: number }[];

/** Capture immediately before DOM changes, not when a background request starts. */
export class PreserveReadingPosition extends Component<Props, {}, Position | null> {
  getSnapshotBeforeUpdate(previous: Props): Position | null {
    if (previous.scope !== this.props.scope || window.scrollY === 0) return null;
    const dashboard = document.querySelector(".dashboard");
    if (!dashboard) return null;
    const rect = dashboard.getBoundingClientRect();
    const x = Math.min(window.innerWidth - 1, Math.max(1, rect.left + rect.width / 2));
    const anchors = new Set<Element>();
    for (const y of [24, 80, 160, window.innerHeight / 2, window.innerHeight - 24]) {
      const element = document.elementFromPoint(x, y);
      if (!element || !dashboard.contains(element)) continue;
      // Prefer the actual text being read; use nearby rows if that text expires.
      anchors.add(element.closest("pre, summary, tr, p, h2, h3, li") ?? element);
      const row = element.closest(".call-card, .activity-list > li");
      if (row) {
        anchors.add(row);
        if (row.nextElementSibling) anchors.add(row.nextElementSibling);
      }
    }
    return [...anchors].map(element => ({ element, top: element.getBoundingClientRect().top }));
  }

  componentDidUpdate(_previous: Props, _state: {}, positions: Position | null) {
    const anchor = positions?.find(({ element }) => element.isConnected && element.getClientRects().length > 0);
    if (!anchor) return;
    const delta = anchor.element.getBoundingClientRect().top - anchor.top;
    if (Math.abs(delta) >= 1) {
      window.scrollBy({ top: delta, behavior: "instant" });
    }
  }

  render() { return this.props.children; }
}
