import type { SystemActivity } from "../../contracts/systemPage";
import type { NetworkView } from "../../v2/screens/activity/NetworkScreen";
import { buildCharts, formatRate, historySpan, HISTORY_SLOTS, liveNote, liveState, type Loadable } from "./systemView";

/** Activity's Network tab: download and upload now, the highest in the chart, and the chart. */
export function buildNetworkView(input: { activity: Loadable<SystemActivity>; history: readonly SystemActivity[]; paused: boolean }): NetworkView {
  const chart = buildCharts(input.history).network;
  const [down, up] = chart.series;
  const peak = (values: readonly (number | null)[]) => {
    const known = values.filter((entry): entry is number => entry !== null);
    return known.length === 0 ? "—" : formatRate(Math.max(...known));
  };
  return {
    live: liveState(input.activity, input.paused),
    note: liveNote(input.activity),
    down: down.now,
    up: up.now,
    peakDown: peak(down.values),
    peakUp: peak(up.values),
    chart,
    slots: HISTORY_SLOTS,
    span: historySpan(input.history),
  };
}
