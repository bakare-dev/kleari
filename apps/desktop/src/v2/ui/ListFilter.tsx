import { useCallback, useState } from "react";
import type { FileCategory } from "../../contracts/storage";
import { SearchField, Select, type Tone } from "./components";
import "./listFilter.css";

/** How a list is ordered. */
export type SortKey = "largest" | "smallest" | "newest" | "oldest" | "name";

/** Every list that has a filter bar; each remembers its own sort and type. */
export type ListId =
  | "review"
  | "trash"
  | "files.largeOld"
  | "files.duplicates"
  | "files.similar"
  | "files.downloads"
  | "files.ai"
  | "apps"
  | "apps.leftovers"
  | "apps.installers"
  | "developer.toolchains"
  | "developer.projects";

export interface FilterState {
  sort: SortKey;
  /** A type chip's value, or `ALL_TYPES`. */
  type: string;
  query: string;
}

export const ALL_TYPES = "all";

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

export interface TypeChip {
  value: string;
  label: string;
  tone?: Tone;
}

/** Type chips for lists of files and folders: a file's kind, or a folder. */
export const fileKindChips: readonly TypeChip[] = [
  { value: "folder", label: "Folders", tone: "blue" },
  { value: "video", label: "Videos", tone: "pink" },
  { value: "image", label: "Images", tone: "violet" },
  { value: "audio", label: "Audio", tone: "indigo" },
  { value: "archive", label: "Archives", tone: "orange" },
  { value: "diskImage", label: "Disk images", tone: "yellow" },
  { value: "document", label: "Documents", tone: "teal" },
  { value: "code", label: "Code", tone: "green" },
  { value: "other", label: "Other", tone: "gray" },
];

/** The file-kind chip an item belongs to. */
export function fileKind(item: { folder: boolean; fileCategory: FileCategory | null }): string {
  return item.folder ? "folder" : (item.fileCategory ?? "other");
}

const sortKeys: readonly SortKey[] = ["largest", "smallest", "newest", "oldest", "name"];
const sortLabels: Record<SortKey, string> = { largest: "Largest first", smallest: "Smallest first", newest: "Newest first", oldest: "Oldest first", name: "Name" };

const storageKey = (list: ListId) => `kleari.listFilter.${list}`;
/** Searches typed this session, per list: kept while Kleari runs, never stored. */
const queries = new Map<ListId, string>();

function remembered(list: ListId, fallback: SortKey): Pick<FilterState, "sort" | "type"> {
  try {
    const raw = window.localStorage.getItem(storageKey(list));
    const saved: unknown = raw === null ? null : JSON.parse(raw);
    if (typeof saved === "object" && saved !== null) {
      const { sort, type } = saved as Record<string, unknown>;
      return {
        sort: sortKeys.find((key) => key === sort) ?? fallback,
        type: typeof type === "string" && type.length > 0 && type.length <= 64 ? type : ALL_TYPES,
      };
    }
  } catch {
    // Deliberate: storage can be unavailable or hold something unreadable; the defaults apply.
  }
  return { sort: fallback, type: ALL_TYPES };
}

function remember(list: ListId, state: FilterState): void {
  try {
    window.localStorage.setItem(storageKey(list), JSON.stringify({ sort: state.sort, type: state.type }));
  } catch {
    // Deliberate: without storage the choice lasts until the page closes, which is harmless.
  }
}

/** A list's filter: sort and type remembered on this Mac per list, search for the session.
 * `initialQuery`, when not empty, starts the search (a search handed over from elsewhere). */
export function useListFilter(list: ListId, fallback: SortKey = "largest", initialQuery = ""): [FilterState, (next: Partial<FilterState>) => void] {
  const start = (id: ListId, query: string): FilterState => ({ ...remembered(id, fallback), query: query || (queries.get(id) ?? "") });
  const [held, setHeld] = useState<{ list: ListId; state: FilterState }>(() => ({ list, state: start(list, initialQuery) }));
  const update = useCallback(
    (next: Partial<FilterState>) => {
      setHeld((current) => {
        const merged = { ...current.state, ...next };
        queries.set(current.list, merged.query);
        if (merged.sort !== current.state.sort || merged.type !== current.state.type) {
          remember(current.list, merged);
        }
        return { list: current.list, state: merged };
      });
    },
    [],
  );
  // Another list shown in the same place (a Files tool, a Developer view): its own filter.
  if (held.list !== list) {
    const next = { list, state: start(list, "") };
    setHeld(next);
    return [next.state, update];
  }
  return [held.state, update];
}

/** The chip in effect: a remembered type no item has any more shows everything. */
function typeInEffect<T>(items: readonly T[], state: FilterState, spec: ListFilterSpec<T>): string {
  const typeOf = spec.type;
  if (state.type === ALL_TYPES || typeOf === undefined) {
    return ALL_TYPES;
  }
  return items.some((item) => typeOf(item) === state.type) ? state.type : ALL_TYPES;
}

/** The sort in effect: by date only for a list that has dates. */
function sortInEffect<T>(state: FilterState, spec: ListFilterSpec<T>): SortKey {
  return spec.date === undefined && (state.sort === "newest" || state.sort === "oldest") ? "largest" : state.sort;
}

/** The items that match, in the chosen order. */
export function applyFilter<T>(items: readonly T[], state: FilterState, spec: ListFilterSpec<T>): T[] {
  const type = typeInEffect(items, state, spec);
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

interface FilterBarProps<T> {
  label: string;
  state: FilterState;
  onChange: (next: Partial<FilterState>) => void;
  /** Every item of the list, for the chips' counts. */
  items: readonly T[];
  spec: ListFilterSpec<T>;
  /** In the order shown; a chip no item belongs to is left out. */
  chips?: readonly TypeChip[];
  allLabel?: string;
  searchWidth?: number;
  className?: string;
}

/** Sort, type chips and a name search, the same on every list. */
export function FilterBar<T>({ label, state, onChange, items, spec, chips = [], allLabel = "All", searchWidth = 180, className }: FilterBarProps<T>) {
  const type = typeInEffect(items, state, spec);
  const counts = new Map<string, number>();
  if (spec.type) {
    for (const item of items) {
      const value = spec.type(item);
      counts.set(value, (counts.get(value) ?? 0) + 1);
    }
  }
  const present = chips.filter((chip) => (counts.get(chip.value) ?? 0) > 0);
  const sorts = sortKeys.filter((key) => spec.date !== undefined || (key !== "newest" && key !== "oldest"));
  return (
    <div role="group" aria-label={label} className={`lf-bar${className ? ` ${className}` : ""}`}>
      {present.length > 1 && (
        <div role="group" aria-label="Type" className="lf-chips">
          <button type="button" aria-pressed={type === ALL_TYPES} className={`lf-chip lf-chip-all${type === ALL_TYPES ? " on" : ""}`} onClick={() => onChange({ type: ALL_TYPES })}>
            <span className="v2-dot lf-dot-all" aria-hidden="true" />
            <span>{allLabel}</span>
            <span className="lf-chip-count">{items.length}</span>
          </button>
          {present.map((chip) => (
            <button
              key={chip.value}
              type="button"
              aria-pressed={type === chip.value}
              className={`lf-chip tone-${chip.tone ?? "slate"}${type === chip.value ? " on" : ""}`}
              onClick={() => onChange({ type: chip.value })}
            >
              <span className="v2-dot" aria-hidden="true" />
              <span>{chip.label}</span>
              <span className="lf-chip-count">{counts.get(chip.value)}</span>
            </button>
          ))}
        </div>
      )}
      <div className="lf-tools">
        <Select label="Sort" value={sortInEffect(state, spec)} options={sorts.map((value) => ({ value, label: sortLabels[value] }))} onChange={(sort) => onChange({ sort })} />
        <SearchField label="Search by name" value={state.query} onChange={(query) => onChange({ query })} width={searchWidth} />
      </div>
    </div>
  );
}
