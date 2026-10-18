(() => {
  const origin = __HIVEMIND_ORIGIN__;
  const nonce = __HIVEMIND_NONCE__;
  if (location.origin !== origin) return;
  const page = crypto.randomUUID().replaceAll('-', '');
  const timeoutMs = 4000;
  let sequence = 0;
  let frontendReady = false;
  let writeVersion = 0;
  let warnedVersion = -1;
  let latestWrite = null;
  let quitting = false;
  const pending = new Map();
  const localePending = new Map();
  const writes = new Set();

  function nextId() {
    do { sequence = sequence % 0xffffffff + 1; } while (pending.has(sequence) || localePending.has(sequence));
    return sequence;
  }

  window.addEventListener('hivemind:locale-response', ({ detail }) => {
    if (detail?.page !== page) return;
    const request = localePending.get(detail.id);
    if (!request) return;
    localePending.delete(detail.id);
    clearTimeout(request.timer);
    if (detail.ok === false) { request.reject(new Error('Language preference request failed')); return; }
    const validLocale = request.action === 'read'
      ? detail.locale === null || detail.locale === 'en' || detail.locale === 'zh'
      : detail.locale === request.locale;
    if (detail.ok !== true || !validLocale || Object.keys(detail).some((key) => !['page', 'id', 'ok', 'locale'].includes(key))) {
      request.reject(new Error('Invalid language preference response'));
      return;
    }
    request.resolve(detail.locale);
  });

  function sendLocale(action, locale) {
    const id = nextId();
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        localePending.delete(id);
        reject(new Error('Language preference request timed out'));
      }, timeoutMs);
      localePending.set(id, { action, locale, resolve, reject, timer });
      try {
        window.chrome.webview.postMessage(JSON.stringify({
          channel: 'hivemind-locale-v1', nonce, page, action, id,
          ...(action === 'write' ? { locale } : {}),
        }));
      } catch (error) {
        localePending.delete(id);
        clearTimeout(timer);
        reject(error);
      }
    });
  }

  function writeLocale(locale) {
    if (!['en', 'zh'].includes(locale)) return Promise.reject(new Error('Invalid language preference'));
    if (quitting) return Promise.reject(new Error('The client is quitting'));
    writeVersion += 1;
    warnedVersion = -1;
    const operation = sendLocale('write', locale).then(() => {});
    const record = { outcome: operation.then(() => true, () => false) };
    latestWrite = record;
    writes.add(record);
    record.outcome.then((ok) => { record.ok = ok; writes.delete(record); });
    return operation;
  }

  function unsavedLanguage() {
    return Object.assign(new Error('Language may not have been saved. Press Quit again to exit.'), { code: 'locale-not-saved' });
  }

  async function waitForWrites() {
    if (warnedVersion === writeVersion) { quitting = true; return; }
    let timer;
    let active = true;
    try {
      await Promise.race([
        (async () => {
          // A selection made while waiting also belongs to this Quit attempt.
          while (active && writes.size) await Promise.all([...writes].map((record) => record.outcome));
          if (!active) return;
          if (latestWrite?.ok === false) throw unsavedLanguage();
          quitting = true;
        })(),
        new Promise((_, reject) => { timer = setTimeout(() => reject(unsavedLanguage()), timeoutMs); }),
      ]);
    } catch (error) {
      quitting = false;
      warnedVersion = writeVersion;
      throw error;
    } finally { active = false; clearTimeout(timer); }
  }

  async function resolveClose(action) {
    if (!['cancel', 'background', 'quit'].includes(action)) throw new Error('Invalid close action');
    if (action === 'quit') {
      await waitForWrites();
      quitting = true;
    }
    try { return await send(action); }
    catch (error) { quitting = false; throw error; }
  }

  window.addEventListener('hivemind:host-response', ({ detail }) => {
    const request = pending.get(detail?.id);
    if (!request) return;
    pending.delete(detail.id);
    clearTimeout(request.timer);
    if (detail.ok) request.resolve();
    else request.reject(new Error('Window action failed'));
  });

  function send(action) {
    const id = nextId();
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        pending.delete(id);
        reject(new Error('Window action timed out'));
      }, timeoutMs);
      pending.set(id, { resolve, reject, timer });
      try {
        window.chrome.webview.postMessage(JSON.stringify({ channel: 'hivemind-window-v1', nonce, action, id }));
      } catch (error) {
        pending.delete(id);
        clearTimeout(timer);
        reject(error);
      }
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
        await resolveClose(action);
        dialog.close();
        dialog.remove();
      } catch (error) {
        description.textContent = error.code === 'locale-not-saved' ? error.message : 'Could not close the window. Try again.';
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
      resolveClose,
      readLocale: () => sendLocale('read'),
      writeLocale,
    }),
    configurable: false,
    writable: false,
  });
})();
