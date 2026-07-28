"use client";

import { ReactNode } from "react";
import { cn } from "@/lib/utils";

export function PageSection({
  eyebrow,
  title,
  body,
  children,
  className,
}: {
  eyebrow: string;
  title: string;
  body?: string;
  children?: ReactNode;
  className?: string;
}) {
  return (
    <section className={cn("mx-auto max-w-7xl px-4 py-16 sm:px-6 sm:py-24", className)}>
      <div className="max-w-3xl">
        <div className="font-mono-tech text-xs uppercase tracking-[0.24em] text-honey">{eyebrow}</div>
        <h2 className="mt-3 text-balance text-3xl font-semibold tracking-tight sm:text-5xl">{title}</h2>
        {body ? <p className="mt-4 text-pretty text-muted-foreground">{body}</p> : null}
      </div>
      {children}
    </section>
  );
}

export function Surface({
  children,
  className,
}: {
  children: ReactNode;
  className?: string;
}) {
  return (
    <div className={cn("rounded-2xl border border-border/60 bg-card/40 p-6 glass", className)}>
      {children}
    </div>
  );
}

export function KeyValue({
  label,
  value,
}: {
  label: string;
  value: ReactNode;
}) {
  return (
    <div>
      <div className="text-xs uppercase tracking-wide text-muted-foreground">{label}</div>
      <div className="mt-1 text-sm font-medium">{value}</div>
    </div>
  );
}
