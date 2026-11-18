import type { ListId } from "../../v2/ui/ListFilter";
import type { IconName } from "../../v2/ui/icons";
import type { Tone } from "../../v2/ui/components";
import type { V2Route } from "../../v2/shell/routes";

// The Dashboard's search: what Kleari already holds this session, grouped by module. Pure and
// free of runtime imports, so it runs under `node --test` (scripts/dashboard-search.test.mjs).

/** Where a result opens: a page, its tab (the hash detail) and the list whose search it fills. */
export interface SearchTarget {
  route: V2Route;
  detail?: string;
  list?: ListId;
}

export interface SearchItem {
  id: string;
  title: string;
  sub: string;
  /** What the query is matched against besides the title (a location, keywords). */
  text?: string;
  /** Opens here instead of the module's own target (another tab of the same page). */
  target?: SearchTarget;
  icon?: IconName;
  tone?: Tone;
}

export interface SearchSource {
  id: string;
  label: string;
  icon: IconName;
  tone: Tone;
  target: SearchTarget;
  /** `null` when the module hasn't read anything yet this session: `pending` says how to. */
  items: readonly SearchItem[] | null;
  pending?: string;
}

export interface SearchGroup {
  source: SearchSource;
  /** The best matches, at most the limit asked for. */
  matches: SearchItem[];
  total: number;
  /** Where "Show all" opens: the best match's tab, so its list shows every match. */
  showAll: SearchTarget;
}

export interface SearchResult {
  needle: string;
  groups: SearchGroup[];
  total: number;
  /** Modules with nothing read yet, so nothing of theirs could be searched. */
  pending: SearchSource[];
}

/** The text a query is matched against: the same as a list's name search (title, then text). */
function haystack(item: SearchItem): string {
  return `${item.title} ${item.text ?? ""}`.toLowerCase();
}

/** Lower is better: a title that starts with the query, then one that contains it, then the rest. */
function rank(item: SearchItem, needle: string): number {
  const title = item.title.toLowerCase();
  return title.startsWith(needle) ? 0 : title.includes(needle) ? 1 : 2;
}

/**
 * Every module's matches for `query`, in the order the sources are given, each group's best
 * `perGroup` first (ties keep the source's own order). An empty query finds nothing, so
 * clearing the search brings the Dashboard back as it was.
 */
export function searchEverything(query: string, sources: readonly SearchSource[], perGroup = 5): SearchResult {
  const needle = query.trim().toLowerCase();
  if (needle === "") {
    return { needle, groups: [], total: 0, pending: [] };
  }
  const groups: SearchGroup[] = [];
  const pending: SearchSource[] = [];
  let total = 0;
  for (const source of sources) {
    if (source.items === null) {
      if (source.pending) {
        pending.push(source);
      }
      continue;
    }
    const found = source.items
      .map((item, index) => ({ item, index, rank: rank(item, needle) }))
      .filter((entry) => haystack(entry.item).includes(needle))
      .sort((a, b) => a.rank - b.rank || a.index - b.index);
    if (found.length === 0) {
      continue;
    }
    const best = found[0].item;
    groups.push({
      source,
      matches: found.slice(0, perGroup).map((entry) => entry.item),
      total: found.length,
      showAll: best.target ?? source.target,
    });
    total += found.length;
  }
  return { needle, groups, total, pending };
}
