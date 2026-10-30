"use client";

import { useEffect } from "react";
import { useI18n } from "@/store/i18n-store";

export default function Error({
  error,
  reset,
}: {
  error: Error & { digest?: string };
  reset: () => void;
}) {
  const { locale } = useI18n();
  const zh = locale === "zh";

  useEffect(() => {
    console.error("App error boundary:", error);
  }, [error]);

  return (
    <div className="flex min-h-screen items-center justify-center bg-background p-8">
      <div className="mx-auto max-w-md text-center">
        <h2 className="text-2xl font-bold text-destructive">
          {zh ? "頁面載入失敗" : "This page could not load"}
        </h2>
        <p className="mt-3 text-muted-foreground">
          {zh
            ? "這個頁面暫時無法載入。請再試一次，或稍後再回來。"
            : "The page could not be loaded. Try again, or come back later."}
        </p>
        <button
          onClick={reset}
          className="mt-6 rounded-lg bg-primary px-5 py-2.5 text-sm font-semibold text-primary-foreground hover:bg-primary/90"
        >
          {zh ? "重試" : "Try again"}
        </button>
      </div>
    </div>
  );
}
