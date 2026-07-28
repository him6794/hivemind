"use client";

import { Languages } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useI18n, type Locale } from "@/store/i18n-store";
import { cn } from "@/lib/utils";

const options: Array<{ value: Locale; label: string; short: string }> = [
  { value: "zh", label: "中文", short: "ZH" },
  { value: "en", label: "English", short: "EN" },
];

export function LocaleToggle({ className }: { className?: string }) {
  const { locale, setLocale } = useI18n();
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const onPointerDown = (event: MouseEvent) => {
      if (ref.current && !ref.current.contains(event.target as Node)) {
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", onPointerDown);
    return () => document.removeEventListener("mousedown", onPointerDown);
  }, []);

  return (
    <div ref={ref} className={cn("relative", className)}>
      <button
        type="button"
        aria-label="Switch language"
        onClick={() => setOpen((value) => !value)}
        className="inline-flex size-9 items-center justify-center rounded-lg text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
      >
        <Languages className="size-4" />
      </button>
      {open ? (
        <div className="absolute right-0 top-full z-50 mt-2 w-40 overflow-hidden rounded-xl border border-border/60 bg-card/95 p-1 shadow-2xl backdrop-blur-2xl">
          {options.map((option) => (
            <button
              key={option.value}
              type="button"
              onClick={() => {
                setLocale(option.value);
                setOpen(false);
              }}
              className={cn(
                "flex w-full items-center justify-between rounded-lg px-3 py-2 text-left text-sm transition-colors",
                locale === option.value
                  ? "bg-honey/10 text-honey"
                  : "text-muted-foreground hover:bg-accent hover:text-foreground"
              )}
            >
              <span>{option.label}</span>
              <span className="font-mono-tech text-xs">{option.short}</span>
            </button>
          ))}
        </div>
      ) : null}
    </div>
  );
}
