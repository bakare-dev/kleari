export interface Poller {
  /** Loads immediately unless a load is already in flight, then restarts the interval. */
  refresh(): void;
  /** Cancels the pending timer; results of an in-flight load are discarded. */
  stop(): void;
}

export interface PollerOptions<T> {
  load: () => Promise<T>;
  onSuccess: (value: T) => void;
  onError: (error: unknown) => void;
  intervalMs: number;
}

/**
 * Shares one pending load between callers. Pollers are recreated on remount or visibility
 * changes; wrapping their loader once keeps a restart from issuing an overlapping request.
 */
export function singleFlight<T>(load: () => Promise<T>): () => Promise<T> {
  let pending: Promise<T> | null = null;
  return () => {
    if (pending === null) {
      pending = load().finally(() => {
        pending = null;
      });
    }
    return pending;
  };
}

/**
 * Single-flight polling: the next load is scheduled only after the previous one settles,
 * so slow responses never overlap and at most one timer exists at a time.
 */
export function startPolling<T>({ load, onSuccess, onError, intervalMs }: PollerOptions<T>): Poller {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let inFlight = false;
  let stopped = false;

  function clearTimer() {
    if (timer !== undefined) {
      clearTimeout(timer);
      timer = undefined;
    }
  }

  async function run() {
    if (stopped || inFlight) {
      return;
    }
    clearTimer();
    inFlight = true;
    try {
      const value = await load();
      if (!stopped) {
        onSuccess(value);
      }
    } catch (error) {
      if (!stopped) {
        onError(error);
      }
    } finally {
      inFlight = false;
      if (!stopped) {
        timer = setTimeout(() => void run(), intervalMs);
      }
    }
  }

  void run();

  return {
    refresh: () => void run(),
    stop: () => {
      stopped = true;
      clearTimer();
    },
  };
}
