/** Coalesce refresh bursts; discard a response if newer data changed mid-flight. */
export const createRefreshScheduler = <T>(options: {
  load: () => Promise<T>;
  apply: (value: T) => void;
  onError: (error: unknown) => void;
  delayMs?: number;
}) => {
  let revision = 0;
  let inFlight = false;
  let disposed = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const schedule = () => {
    if (disposed || inFlight || timer) return;
    timer = setTimeout(() => { timer = undefined; void run(); }, options.delayMs ?? 120);
  };
  const run = async () => {
    const startedRevision = revision;
    inFlight = true;
    try {
      const value = await options.load();
      if (!disposed && revision === startedRevision) options.apply(value);
    } catch (error) {
      if (!disposed && revision === startedRevision) options.onError(error);
    } finally {
      inFlight = false;
      if (!disposed && revision !== startedRevision) schedule();
    }
  };
  return {
    request() { revision += 1; schedule(); },
    dispose() { disposed = true; if (timer) clearTimeout(timer); timer = undefined; },
  };
};
