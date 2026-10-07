export async function discoverService(endpoints: readonly string[], preferredEndpoint: string | null, probe: (endpoint: string, signal: AbortSignal) => Promise<boolean>, signal?: AbortSignal): Promise<string | null> {
  const ordered = [...new Set(endpoints)];
  if (preferredEndpoint && ordered.includes(preferredEndpoint) && ordered[0] !== preferredEndpoint) {
    const configured = ordered.shift();
    ordered.splice(ordered.indexOf(preferredEndpoint), 1);
    ordered.unshift(...(configured ? [configured] : []), preferredEndpoint);
  }
  const controller = new AbortController();
  const combined = signal ? AbortSignal.any([signal, controller.signal]) : controller.signal;
  return new Promise((resolve) => {
    let settled = false;
    const results: Array<boolean | undefined> = ordered.map(() => undefined);
    const finish = (endpoint: string | null) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      combined.removeEventListener("abort", abort);
      controller.abort();
      resolve(endpoint);
    };
    const evaluate = () => {
      for (let index = 0; index < ordered.length; index += 1) {
        if (results[index] === undefined) return;
        if (results[index]) { finish(ordered[index]); return; }
      }
      finish(null);
    };
    const abort = () => finish(null);
    const timer = setTimeout(() => { for (let index = 0; index < results.length; index += 1) results[index] ??= false; evaluate(); }, 2500);
    combined.addEventListener("abort", abort, { once: true });
    if (combined.aborted) { finish(null); return; }
    if (!ordered.length) { finish(null); return; }
    ordered.forEach((endpoint, index) => {
      void Promise.resolve().then(() => probe(endpoint, combined)).catch(() => false).then((online) => {
        if (settled) return;
        results[index] = online;
        evaluate();
      });
    });
  });
}
