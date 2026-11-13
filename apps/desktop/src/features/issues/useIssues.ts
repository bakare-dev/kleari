import { useCallback, useEffect, useRef, useState } from "react";
import { parseIssues, type Issue } from "../../contracts/issues";
import { call } from "../../lib/storageIpc";

/** The issues now, read when the Dashboard opens and on request. `null` until read. */
export function useIssues(load: () => Promise<Issue[]> = () => call("get_issues", {}, parseIssues)) {
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
