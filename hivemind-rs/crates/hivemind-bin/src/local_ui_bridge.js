(() => {
  const origin = __HIVEMIND_ORIGIN__;
  const nonce = __HIVEMIND_NONCE__;
  if (location.origin !== origin) return;
  let sequence = 0;
  let frontendReady = false;
  const pending = new Map();

  window.addEventListener('hivemind:host-response', ({ detail }) => {
    const request = pending.get(detail?.id);
    if (!request) return;
    pending.delete(detail.id);
    clearTimeout(request.timer);
    if (detail.ok) request.resolve();
    else request.reject(new Error('Window action failed'));
  });

  function send(action) {
    const id = ++sequence;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        pending.delete(id);
        reject(new Error('Window action timed out'));
      }, 4000);
      pending.set(id, { resolve, reject, timer });
      window.chrome.webview.postMessage(JSON.stringify({ channel: 'hivemind-window-v1', nonce, action, id }));
    });
  }

  function fallback() {
    if (frontendReady || document.getElementById('hivemind-close-fallback')) return;
    if (!document.body) {
      document.addEventListener('DOMContentLoaded', fallback, { once: true });
      return;
    }
    const dialog = document.createElement('dialog');
    dialog.id = 'hivemind-close-fallback';
    dialog.setAttribute('aria-labelledby', 'hivemind-close-title');
    dialog.style.cssText = 'position:fixed;inset:auto;top:50%;left:50%;margin:0;transform:translate(-50%,-50%);box-sizing:border-box;border:1px solid var(--border,#ddd);border-radius:10px;padding:24px;max-width:calc(100vw - 32px);max-height:calc(100dvh - 32px);overflow:auto;width:420px;background:var(--background,#fff);color:var(--foreground,#171717);font:14px system-ui;box-shadow:0 20px 80px #0003';
    const title = document.createElement('h2');
    title.id = 'hivemind-close-title';
    title.textContent = 'Keep client running?';
    title.style.cssText = 'font-size:18px;margin:0 0 12px';
    const description = document.createElement('p');
    description.textContent = 'Keep the service in the background, or quit this client.';
    const actions = document.createElement('div');
    actions.style.cssText = 'display:flex;gap:8px;flex-wrap:wrap;margin-top:20px';
    let busy = false;
    async function resolve(action) {
      if (busy) return;
      busy = true;
      for (const item of actions.children) item.disabled = true;
      const animations = [];
      try {
        if (action !== 'cancel' && !window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
          for (const element of [document.body, dialog]) {
            const animation = element.animate?.([{ opacity: 1 }, { opacity: 0 }], {
              duration: 180, easing: 'ease-out', fill: 'forwards',
            });
            if (animation) animations.push(animation);
          }
          await Promise.all(animations.map((animation) => animation.finished));
        }
        await send(action);
        dialog.close();
        dialog.remove();
      } catch {
        description.textContent = 'Could not close the window. Try again.';
      } finally {
        for (const animation of animations) animation.cancel();
        for (const item of actions.children) item.disabled = false;
        busy = false;
      }
    }
    for (const [action, label] of [['cancel', 'Cancel'], ['quit', 'Quit'], ['background', 'Keep in background']]) {
      const button = document.createElement('button');
      button.type = 'button';
      button.textContent = label;
      button.style.cssText = 'min-height:44px;padding:8px 14px;border:1px solid var(--border,#ddd);border-radius:6px;background:var(--secondary,#f5f5f5);color:inherit;cursor:pointer';
      button.onclick = () => resolve(action);
      actions.append(button);
    }
    dialog.append(title, description, actions);
    dialog.addEventListener('cancel', (event) => {
      event.preventDefault();
      void resolve('cancel');
    });
    document.body.append(dialog);
    dialog.showModal();
    if (!window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
      dialog.animate?.([
        { opacity: 0, transform: 'translate(-50%,-50%) scale(0.98)' },
        { opacity: 1, transform: 'translate(-50%,-50%) scale(1)' },
      ], { duration: 180, easing: 'ease-out' });
    }
  }

  window.addEventListener('hivemind:close-requested', fallback);
  Object.defineProperty(window, '__HIVEMIND_DESKTOP__', {
    value: Object.freeze({
      ready() {
        frontendReady = true;
        document.getElementById('hivemind-close-fallback')?.remove();
        return send('ready');
      },
      requestClose: () => send('request'),
      resolveClose(action) {
        if (!['cancel', 'background', 'quit'].includes(action)) return Promise.reject(new Error('Invalid close action'));
        return send(action);
      },
    }),
    configurable: false,
    writable: false,
  });
})();
