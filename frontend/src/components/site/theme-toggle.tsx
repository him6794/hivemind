"use client";

import { useTheme } from "next-themes";
import { Moon, Sun } from "lucide-react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

export function ThemeToggle({ className }: { className?: string }) {
  const { theme, setTheme } = useTheme();
  const isDark = theme === "dark";

  return (
    <Button
      type="button"
      variant="ghost"
      size="icon"
      aria-label="Toggle theme"
      aria-pressed={isDark}
      onClick={() => setTheme(isDark ? "light" : "dark")}
      className={cn("relative size-11 text-muted-foreground", className)}
    >
      <Sun aria-hidden="true" className={cn("size-4 transition-all duration-200", isDark ? "scale-0 -rotate-90 opacity-0" : "scale-100 opacity-100")} />
      <Moon aria-hidden="true" className={cn("absolute size-4 transition-all duration-200", isDark ? "scale-100 opacity-100" : "scale-0 rotate-90 opacity-0")} />
    </Button>
  );
}
