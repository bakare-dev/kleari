// The list filter's rules, without React: what a filter shows and in what order. ListFilter.tsx
// re-exports these; `pnpm test:ui` tests them (scripts/list-filter.test.mjs).

/** How a list is ordered. */
export type SortKey = "largest" | "smallest" | "newest" | "oldest" | "name";

export interface FilterState {
  sort: SortKey;
  /** A type chip's value, or `ALL_TYPES`. */
  type: string;
  query: string;
}

export const ALL_TYPES = "all";

export const sortKeys: readonly SortKey[] = ["largest", "smallest", "newest", "oldest", "name"];

/** How to read a list's items. Without `date`, the newest and oldest sorts aren't offered. */
export interface ListFilterSpec<T> {
  bytes: (item: T) => number;
  name: (item: T) => string;
  /** More text the search matches besides the name (a location or path). */
  text?: (item: T) => string;
  date?: (item: T) => number | null;
  /** The value of the type chip the item belongs to. */
  type?: (item: T) => string;
}

/** The chip in effect: a remembered type no item of the whole list has any more shows everything. */
export function typeInEffect<T>(items: readonly T[], state: FilterState, spec: ListFilterSpec<T>): string {
  const typeOf = spec.type;
  if (state.type === ALL_TYPES || typeOf === undefined) {
    return ALL_TYPES;
  }
  return items.some((item) => typeOf(item) === state.type) ? state.type : ALL_TYPES;
}

/** The sort in effect: by date only for a list that has dates. */
export function sortInEffect<T>(state: FilterState, spec: ListFilterSpec<T>): SortKey {
  return spec.date === undefined && (state.sort === "newest" || state.sort === "oldest") ? "largest" : state.sort;
}

/**
 * The items that match, in the chosen order. `whole` is the list the filter bar counts (by
 * default `items`): for a list shown in parts, such as Files' groups, each part is filtered
 * against the whole list's chips, so a part with none of the chosen type shows nothing rather
 * than everything.
 */
export function applyFilter<T>(items: readonly T[], state: FilterState, spec: ListFilterSpec<T>, whole: readonly T[] = items): T[] {
  const type = typeInEffect(whole, state, spec);
  const needle = state.query.trim().toLowerCase();
  const shown = items.filter(
    (item) =>
      (type === ALL_TYPES || spec.type?.(item) === type) &&
      (needle === "" || `${spec.name(item)} ${spec.text?.(item) ?? ""}`.toLowerCase().includes(needle)),
  );
  const date = (item: T) => spec.date?.(item) ?? null;
  const compare: Record<SortKey, (a: T, b: T) => number> = {
    largest: (a, b) => spec.bytes(b) - spec.bytes(a),
    smallest: (a, b) => spec.bytes(a) - spec.bytes(b),
    // Items without a date go last either way.
    newest: (a, b) => (date(b) ?? -Infinity) - (date(a) ?? -Infinity),
    oldest: (a, b) => (date(a) ?? Infinity) - (date(b) ?? Infinity),
    name: (a, b) => spec.name(a).localeCompare(spec.name(b), undefined, { numeric: true, sensitivity: "base" }),
  };
  return shown.sort(compare[sortInEffect(state, spec)]);
}
