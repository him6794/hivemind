"use client";

import { Check } from "lucide-react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";

export type ReceiptRow = { label: string; value: string; emphasis?: boolean };

export function Receipt({ title, note, rows }: { title: string; note: string; rows: ReceiptRow[] }) {
  return (
    <Card className="shadow-none">
      <CardHeader>
        <CardTitle className="text-base">{title}</CardTitle>
      </CardHeader>
      <CardContent>
        <dl className="grid gap-3 text-sm">
          {rows.map((row) => (
            <div
              key={row.label}
              className={row.emphasis ? "flex items-baseline justify-between gap-4 border-t pt-3 font-medium" : "flex items-baseline justify-between gap-4"}
            >
              <dt className="text-muted-foreground">{row.label}</dt>
              <dd className="font-mono-tech text-right">{row.value}</dd>
            </div>
          ))}
        </dl>
        <p className="mt-5 text-xs leading-relaxed text-muted-foreground">{note}</p>
      </CardContent>
    </Card>
  );
}

export function Bullets({ items }: { items: string[] }) {
  return (
    <ul className="space-y-3 text-sm leading-relaxed text-muted-foreground">
      {items.map((item) => (
        <li key={item} className="flex gap-3">
          <Check aria-hidden="true" className="mt-0.5 size-4 shrink-0 text-honey" />
          <span>{item}</span>
        </li>
      ))}
    </ul>
  );
}

export function StepList({ steps }: { steps: { step: string; title: string; body: string }[] }) {
  return (
    <ol className="grid gap-6">
      {steps.map((step) => (
        <li key={step.step} className="flex gap-5">
          <span className="font-mono-tech text-sm text-honey">{step.step}</span>
          <div>
            <h2 className="font-medium">{step.title}</h2>
            <p className="mt-1 text-sm leading-relaxed text-muted-foreground">{step.body}</p>
          </div>
        </li>
      ))}
    </ol>
  );
}
