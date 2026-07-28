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
        <svg
          viewBox="0 0 40 40"
          className="size-8"
          fill="none"
          xmlns="http://www.w3.org/2000/svg"
          aria-hidden="true"
        >
          <defs>
            <linearGradient id="hive-g" x1="0" y1="0" x2="40" y2="40">
              <stop offset="0%" stopColor="oklch(0.99 0 0)" />
              <stop offset="55%" stopColor="oklch(0.82 0 0)" />
              <stop offset="100%" stopColor="oklch(0.5 0 0)" />
            </linearGradient>
          </defs>
          {/* Hexagon hive node */}
          <path
            d="M20 2 L34 10 L34 30 L20 38 L6 30 L6 10 Z"
            stroke="url(#hive-g)"
            strokeWidth="1.6"
            opacity="0.9"
          />
          <path
            d="M20 9 L28 13.5 L28 22.5 L20 27 L12 22.5 L12 13.5 Z"
            stroke="url(#hive-g)"
            strokeWidth="1.4"
            opacity="0.55"
          />
          <path
            d="M20 15 L24 17.3 L24 22.7 L20 25 L16 22.7 L16 17.3 Z"
            fill="url(#hive-g)"
            opacity="0.95"
          />
          <circle cx="20" cy="20" r="2.2" fill="oklch(0.11 0 0)" />
        </svg>
        <span className="absolute inset-0 -z-10 rounded-full bg-honey/30 blur-lg" />
      </span>
      {withText && (
        <span className="text-[17px] font-semibold tracking-tight">
          Hive<span className="text-honey">Mind</span>
        </span>
      )}
    </div>
  );
}
