import { useCallback, useEffect, useRef, useState } from "react";
import { parseIssues, type Issue } from "../../contracts/issues";
import { call } from "../../lib/storageIpc";

/**
 * Module-scoped so its identity is stable across renders. An inline default would be a new
 * function each render, changing `refresh` and re-running the effect: a get_issues loop.
 */
const loadIssues = (): Promise<Issue[]> => call("get_issues", {}, parseIssues);

/**
 * The issues now, read when the Dashboard opens and on request. `null` until read.
 * A custom `load` must be stable (module-scoped or memoised), or the read repeats every render.
 */
export function useIssues(load: () => Promise<Issue[]> = loadIssues) {
  const [issues, setIssues] = useState<Issue[] | null>(null);
  const [failed, setFailed] = useState(false);
  const mounted = useRef(true);

  const refresh = useCallback(() => {
    load().then(
      (next) => {
        if (mounted.current) {
          setIssues(next);
          setFailed(false);
        }
      },
      () => {
        if (mounted.current) {
          setFailed(true);
        }
      },
    );
  }, [load]);

  useEffect(() => {
    mounted.current = true;
    refresh();
    return () => {
      mounted.current = false;
    };
  }, [refresh]);

  return { issues, failed, refresh };
}
