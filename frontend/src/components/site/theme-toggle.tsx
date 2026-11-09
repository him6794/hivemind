"use client";

import { useEffect, useState } from "react";
import { useTheme } from "next-themes";
import { Moon, Sun } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useI18n } from "@/store/i18n-store";
import { cn } from "@/lib/utils";

export function ThemeToggle({ className }: { className?: string }) {
  const { theme, setTheme } = useTheme();
  const { locale } = useI18n();
  // The saved theme is known only in the browser. Until the page has mounted, the toggle shows the light state,
  // the same as the server render, so the first client render matches the HTML and no hydration error appears.
  const [mounted, setMounted] = useState(false);
  useEffect(() => {
    setMounted(true);
  }, []);
  const isDark = mounted && theme === "dark";

  return (
    <Button
      type="button"
      variant="ghost"
      size="icon"
      aria-label={locale === "zh" ? "切換主題" : "Toggle theme"}
      aria-pressed={isDark}
      onClick={() => setTheme(isDark ? "light" : "dark")}
      className={cn("relative size-11 text-muted-foreground", className)}
    >
      <Sun aria-hidden="true" className={cn("size-4 transition-all duration-200", isDark ? "scale-0 -rotate-90 opacity-0" : "scale-100 opacity-100")} />
      <Moon aria-hidden="true" className={cn("absolute size-4 transition-all duration-200", isDark ? "scale-100 opacity-100" : "scale-0 rotate-90 opacity-0")} />
    </Button>
  );
}
