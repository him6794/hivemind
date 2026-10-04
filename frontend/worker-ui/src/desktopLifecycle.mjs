export function getDesktopBridge(host = globalThis.window) {
  const bridge = host?.__HIVEMIND_DESKTOP__;
  return bridge
    && typeof bridge.ready === 'function'
    && typeof bridge.requestClose === 'function'
    && typeof bridge.resolveClose === 'function'
    ? bridge
    : null;
}

export function motionDelay(host = globalThis.window) {
  return host?.matchMedia?.('(prefers-reduced-motion: reduce)').matches ? 0 : 180;
}

function withDeadline(operation, timeoutMs) {
  let timer;
  const deadline = new Promise((_, reject) => {
    timer = setTimeout(() => reject(new Error('Window action timed out')), timeoutMs);
  });
  return Promise.race([operation, deadline]).finally(() => clearTimeout(timer));
}

export function createCloseController({ bridge, setVisibility, delay = 180, timeoutMs = 5000, wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms)) }) {
  let busy = false;
  return {
    get busy() { return busy; },
    async resolve(action) {
      if (!['cancel', 'background', 'quit'].includes(action)) throw new Error('Invalid close action');
      if (busy) return false;
      busy = true;
      try {
        const host = bridge();
        if (!host) throw new Error('Desktop window controls are unavailable');
        if (action !== 'cancel') {
          setVisibility('closing');
          await wait(delay);
        }
        await withDeadline(Promise.resolve().then(() => host.resolveClose(action)), timeoutMs);
        return true;
      } finally {
        setVisibility('visible');
        busy = false;
      }
    },
  };
}
