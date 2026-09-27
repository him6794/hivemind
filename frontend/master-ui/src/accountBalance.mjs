export function parseCptBalance(payload = {}) {
  const raw = payload?.balance ?? payload?.cpt_balance;
  if (raw === undefined || raw === null || raw === '') {
    throw new Error('Account service returned an invalid balance.');
  }

  const balance = Number(raw);
  if (!Number.isFinite(balance)) {
    throw new Error('Account service returned an invalid balance.');
  }
  return balance;
}

export function createBalanceRefreshController({
  loadBalance,
  getCurrentToken,
  setBalance,
  setError,
}) {
  let inFlight = null;
  let generation = 0;

  function refresh(authToken) {
    if (!authToken) return Promise.resolve(false);
    if (inFlight) return inFlight;

    const requestGeneration = generation;
    const request = Promise.resolve().then(async () => {
      try {
        const balance = await loadBalance(authToken);
        if (generation !== requestGeneration || getCurrentToken() !== authToken) return false;
        setBalance(balance);
        setError('');
        return true;
      } catch (error) {
        if (generation === requestGeneration && getCurrentToken() === authToken) {
          setError(error?.message || 'Failed to load account balance');
        }
        return false;
      }
    });
    let trackedRequest;
    trackedRequest = request.finally(() => {
      if (inFlight === trackedRequest) inFlight = null;
    });
    inFlight = trackedRequest;
    return trackedRequest;
  }

  async function refreshAfter(authToken) {
    const pending = inFlight;
    if (pending) await pending;
    if (authToken && authToken === getCurrentToken()) await refresh(authToken);
  }

  function clear() {
    generation += 1;
    inFlight = null;
    setBalance(null);
    setError('');
  }

  return { refresh, refreshAfter, clear };
}
