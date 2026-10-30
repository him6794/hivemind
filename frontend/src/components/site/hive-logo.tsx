"use client";

import { cn } from "@/lib/utils";

export function HiveLogo({
  className,
  withText = false,
}: {
  className?: string;
  withText?: boolean;
}) {
  return (
    <div className={cn("flex items-center gap-2.5", className)}>
      <span className="relative inline-flex size-8 items-center justify-center">
        {/* The mark is decorative: the wordmark next to it, or the button's label, names the link. */}
        <img src="/hivemind-mark.png" alt="" width={256} height={256} className="size-8 object-contain" />
      </span>
      {withText && (
        <span className="text-[17px] font-semibold tracking-tight">
          Hive<span className="text-honey">Mind</span>
        </span>
      )}
    </div>
  );
}
