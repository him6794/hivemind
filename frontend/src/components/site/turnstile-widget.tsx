"use client";

import { forwardRef, useEffect, useImperativeHandle, useRef, useState } from "react";
import { TURNSTILE_SCRIPT_URL, turnstileSiteKey } from "@/lib/turnstile-policy.mjs";
import { useI18n } from "@/store/i18n-store";

type TurnstileApi = {
  render: (container: HTMLElement, options: Record<string, unknown>) => string;
  reset: (widgetId?: string) => void;
  remove: (widgetId?: string) => void;
};

declare global {
  interface Window {
    turnstile?: TurnstileApi;
  }
}

export type TurnstileHandle = {
  // Clears the token and resets the challenge. Call it after a failed submission, because
  // a token can be used only once.
  reset: () => void;
};

type TurnstileWidgetProps = {
  // Receives the token when the challenge passes, and "" when it expires or fails.
  onToken: (token: string) => void;
};

let scriptRequest: Promise<void> | null = null;

// Loads the Turnstile script once for the page. A failed load is retried on the next mount.
function loadTurnstile(): Promise<void> {
  if (window.turnstile) return Promise.resolve();
  if (!scriptRequest) {
    scriptRequest = new Promise<void>((resolve, reject) => {
      const script = document.createElement("script");
      script.src = TURNSTILE_SCRIPT_URL;
      script.async = true;
      script.onload = () => resolve();
      script.onerror = () => {
        script.remove();
        scriptRequest = null;
        reject(new Error("turnstile script failed to load"));
      };
      document.head.appendChild(script);
    });
  }
  return scriptRequest;
}

export const TurnstileWidget = forwardRef<TurnstileHandle, TurnstileWidgetProps>(function TurnstileWidget(
  { onToken },
  ref
) {
  const { locale } = useI18n();
  const zh = locale === "zh";
  const siteKey = turnstileSiteKey(process.env.NEXT_PUBLIC_TURNSTILE_SITE_KEY);
  const containerRef = useRef<HTMLDivElement>(null);
  const widgetId = useRef<string | null>(null);
  const onTokenRef = useRef(onToken);
  const [loadFailed, setLoadFailed] = useState(false);

  useEffect(() => {
    onTokenRef.current = onToken;
  }, [onToken]);

  useEffect(() => {
    let cancelled = false;
    const container = containerRef.current;

    loadTurnstile()
      .then(() => {
        if (cancelled || !container || !window.turnstile) return;
        // The normal widget is 300px wide. A card on a 320px screen is narrower than that, so the compact one fits there.
        const size = container.clientWidth < 300 ? "compact" : "normal";
        widgetId.current = window.turnstile.render(container, {
          sitekey: siteKey,
          size,
          callback: (token: string) => onTokenRef.current(token),
          "expired-callback": () => onTokenRef.current(""),
          "error-callback": () => onTokenRef.current(""),
        });
      })
      .catch(() => {
        if (!cancelled) setLoadFailed(true);
      });

    return () => {
      cancelled = true;
      if (widgetId.current !== null && window.turnstile) {
        window.turnstile.remove(widgetId.current);
      }
      widgetId.current = null;
    };
  }, [siteKey]);

  useImperativeHandle(
    ref,
    () => ({
      reset() {
        onTokenRef.current("");
        if (widgetId.current !== null && window.turnstile) {
          window.turnstile.reset(widgetId.current);
        }
      },
    }),
    []
  );

  return (
    <div className="space-y-2">
      <div ref={containerRef} data-sitekey={siteKey} className="min-h-[65px]" />
      {loadFailed ? (
        <p role="alert" className="text-sm text-destructive">
          {zh
            ? "人機驗證元件無法載入，請檢查網路連線或瀏覽器的內容阻擋設定後再試。"
            : "The verification widget could not load. Check your connection or content blocker, then try again."}
        </p>
      ) : null}
    </div>
  );
});
