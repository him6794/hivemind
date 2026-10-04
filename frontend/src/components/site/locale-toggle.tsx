"use client";

import { Languages, Check } from "lucide-react";
import * as DropdownMenu from "@radix-ui/react-dropdown-menu";
import { Button } from "@/components/ui/button";
import { useI18n, type Locale } from "@/store/i18n-store";

const options: Array<{ value: Locale; label: string }> = [
  { value: "zh", label: "中文" },
  { value: "en", label: "English" },
];

export function LocaleToggle({ className }: { className?: string }) {
  const { locale, setLocale } = useI18n();
  return (
    <DropdownMenu.Root>
      <DropdownMenu.Trigger asChild>
        <Button type="button" variant="ghost" size="icon" aria-label="Switch language" className={`size-11 text-muted-foreground ${className || ""}`}><Languages aria-hidden="true" className="size-4" /></Button>
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content onCloseAutoFocus={(event) => { if (document.querySelector('[role="dialog"]')) event.preventDefault(); }} align="end" sideOffset={6} className="z-[60] min-w-36 rounded-md border bg-popover p-1 text-popover-foreground shadow-md outline-none data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=open]:fade-in-0 data-[state=closed]:fade-out-0 data-[state=open]:zoom-in-95 data-[state=closed]:zoom-out-95">
          <DropdownMenu.RadioGroup value={locale} onValueChange={(value) => { if (value === "zh" || value === "en") setLocale(value); }}>
            {options.map((option) => (
              <DropdownMenu.RadioItem key={option.value} value={option.value} className="relative flex min-h-11 cursor-default select-none items-center rounded-sm py-2 pl-8 pr-3 text-sm outline-none focus:bg-accent focus:text-accent-foreground">
                <DropdownMenu.ItemIndicator className="absolute left-2"><Check aria-hidden="true" className="size-4" /></DropdownMenu.ItemIndicator>
                {option.label}
              </DropdownMenu.RadioItem>
            ))}
          </DropdownMenu.RadioGroup>
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}
