"use client";

import { ReactNode } from "react";
import { cn } from "@/lib/utils";
import { Card } from "@/components/ui/card";

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
    <section className={cn("mx-auto max-w-7xl px-4 pb-16 pt-28 sm:px-6 sm:pb-24 sm:pt-32", className)}>
      <div className="max-w-3xl">
        <div className="font-mono-tech text-xs uppercase tracking-[0.24em] text-honey">{eyebrow}</div>
        <h1 className="mt-3 text-balance text-3xl font-semibold tracking-tight sm:text-5xl">{title}</h1>
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
    <Card className={cn("p-6", className)}>
      {children}
    </Card>
  );
}
