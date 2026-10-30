"use client";

import { useState } from "react";
import { Check, Copy, Info, TriangleAlert } from "lucide-react";
import { useAppStore } from "@/store/app-store";
import { useI18n } from "@/store/i18n-store";
import { getSiteDefinition } from "@/lib/hivemind-site-data.mjs";
import { EXAMPLES } from "@/lib/docs/examples.mjs";
import { DocsReference } from "./docs-reference";
import { DEMOS } from "./docs-demos";

export type DocsBlockData =
  | { type: "p"; text: string }
  | { type: "h2"; id: string; text: string }
  | { type: "list"; items: string[] }
  | { type: "steps"; items: { title: string; body: string; code?: { lang: string; code: string } }[] }
  | { type: "code"; lang: string; code: string }
  | { type: "tabs"; tabs: { label: string; lang: string; code: string }[] }
  | { type: "example"; id: string }
  | { type: "table"; head: string[]; rows: string[][] }
  | { type: "callout"; tone: "note" | "warning"; text: string }
  | { type: "links"; slugs: string[] }
  | { type: "reference"; name: string }
  | { type: "demo"; id: string };

// Backticks mark inline code in the page text.
export function renderInline(text: string) {
  return text.split("`").map((part, index) =>
    index % 2 === 1 ? (
      <code key={index} className="rounded bg-muted px-1 py-0.5 font-mono-tech text-[0.85em] text-foreground">
        {part}
      </code>
    ) : (
      <span key={index}>{part}</span>
    )
  );
}

export function CodeBlock({ code, lang, label }: { code: string; lang: string; label?: string }) {
  const { locale } = useI18n();
  const zh = locale === "zh";
  const [copied, setCopied] = useState(false);

  async function copy() {
    try {
      await navigator.clipboard.writeText(code);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1600);
    } catch {
      // The browser refused clipboard access. The code stays selectable.
    }
  }

  return (
    <div className="mt-4 overflow-hidden rounded-xl border border-border/60 bg-background">
      <div className="flex items-center justify-between gap-3 border-b border-border/60 px-4 py-2 text-xs text-muted-foreground">
        <span className="font-mono-tech uppercase tracking-wide">{label ?? lang}</span>
        <button
          type="button"
          onClick={copy}
          className="inline-flex items-center gap-1.5 rounded-md px-2 py-1 transition-colors hover:bg-accent hover:text-foreground"
        >
          {copied ? <Check aria-hidden="true" className="size-3.5" /> : <Copy aria-hidden="true" className="size-3.5" />}
          {copied ? (zh ? "已複製" : "Copied") : zh ? "複製" : "Copy"}
        </button>
      </div>
      <pre className="overflow-x-auto p-4 text-xs leading-relaxed">
        <code className="font-mono-tech">{code}</code>
      </pre>
    </div>
  );
}

export function CodeTabs({ tabs }: { tabs: { label: string; lang: string; code: string }[] }) {
  const [active, setActive] = useState(0);
  const current = tabs[active] ?? tabs[0];

  return (
    <div className="mt-4">
      <div role="tablist" className="inline-flex gap-1 rounded-lg bg-muted p-1">
        {tabs.map((tab, index) => (
          <button
            key={tab.label}
            type="button"
            role="tab"
            aria-selected={index === active}
            onClick={() => setActive(index)}
            className={`rounded-md px-3 py-1 text-xs font-medium transition-colors ${
              index === active ? "bg-background text-foreground shadow-sm" : "text-muted-foreground hover:text-foreground"
            }`}
          >
            {tab.label}
          </button>
        ))}
      </div>
      <div role="tabpanel">
        <CodeBlock code={current.code} lang={current.lang} />
      </div>
    </div>
  );
}

export function ExampleBlock({ id }: { id: string }) {
  const { locale } = useI18n();
  const zh = locale === "zh";
  const example = EXAMPLES.find((entry) => entry.id === id);
  if (!example) return null;

  return (
    <figure className="mt-6">
      <CodeBlock code={example.source} lang="hcode" label={zh ? "程式" : "Program"} />
      <CodeBlock code={example.input} lang="json" label={zh ? "輸入" : "Input"} />
      <dl className="mt-4 grid gap-4 rounded-xl border border-border bg-card p-5 text-sm sm:grid-cols-2">
        <div>
          <dt className="text-xs uppercase tracking-wide text-muted-foreground">{zh ? "結果" : "Result"}</dt>
          <dd className="mt-1 font-mono-tech break-all">{example.result}</dd>
        </div>
        <div>
          <dt className="text-xs uppercase tracking-wide text-muted-foreground">
            {zh ? "用量單位（一個副本）" : "Usage units (one replica)"}
          </dt>
          <dd className="mt-1 font-mono-tech">{example.usage}</dd>
        </div>
        {example.output ? (
          <div className="sm:col-span-2">
            <dt className="text-xs uppercase tracking-wide text-muted-foreground">{zh ? "印出的輸出" : "Printed output"}</dt>
            <dd className="mt-1">
              <pre className="overflow-x-auto rounded-lg bg-background p-3 font-mono-tech text-xs">{example.output}</pre>
            </dd>
          </div>
        ) : null}
      </dl>
    </figure>
  );
}

function StepsBlock({ items }: { items: { title: string; body: string; code?: { lang: string; code: string } }[] }) {
  return (
    <ol className="mt-6 space-y-8">
      {items.map((item, index) => (
        <li key={item.title} className="relative pl-12">
          <span
            aria-hidden="true"
            className="absolute left-0 top-0 flex size-8 items-center justify-center rounded-full border border-border bg-background font-mono-tech text-xs"
          >
            {index + 1}
          </span>
          {index < items.length - 1 ? (
            <span aria-hidden="true" className="absolute left-4 top-10 h-[calc(100%-2rem)] w-px bg-border" />
          ) : null}
          <h3 className="text-base font-semibold">{item.title}</h3>
          <p className="mt-2 text-sm leading-relaxed text-muted-foreground">{renderInline(item.body)}</p>
          {item.code ? <CodeBlock code={item.code.code} lang={item.code.lang} /> : null}
        </li>
      ))}
    </ol>
  );
}

function TableBlock({ head, rows }: { head: string[]; rows: string[][] }) {
  return (
    <div className="mt-6 overflow-x-auto rounded-xl border border-border bg-card">
      <table className="w-full text-left text-sm">
        <thead className="bg-muted/50">
          <tr>
            {head.map((cell) => (
              <th key={cell} scope="col" className="px-4 py-2.5 font-medium">
                {cell}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => (
            <tr key={row.join("|")} className="border-t border-border/60">
              {row.map((cell, index) => (
                <td key={`${index}-${cell}`} className="px-4 py-2.5 align-top leading-relaxed text-muted-foreground">
                  {renderInline(cell)}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function CalloutBlock({ tone, text }: { tone: "note" | "warning"; text: string }) {
  const warning = tone === "warning";
  return (
    <div
      role="note"
      className={`mt-6 flex gap-3 rounded-xl border p-4 text-sm leading-relaxed ${
        warning ? "border-amber-500/40 bg-amber-500/5" : "border-honey/30 bg-honey/5"
      }`}
    >
      {warning ? (
        <TriangleAlert aria-hidden="true" className="mt-0.5 size-4 shrink-0 text-amber-600" />
      ) : (
        <Info aria-hidden="true" className="mt-0.5 size-4 shrink-0 text-honey" />
      )}
      <p>{renderInline(text)}</p>
    </div>
  );
}

function LinksBlock({ slugs }: { slugs: string[] }) {
  const { locale } = useI18n();
  const navigate = useAppStore((state) => state.navigate);
  const pages = getSiteDefinition(locale).sections.docs.pages as { slug: string; title: string; description: string }[];
  const linked = slugs.map((slug) => pages.find((page) => page.slug === slug)).filter(Boolean) as {
    slug: string;
    title: string;
    description: string;
  }[];

  return (
    <div className="mt-10 grid gap-3 sm:grid-cols-2">
      {linked.map((page) => (
        <button
          key={page.slug}
          type="button"
          onClick={() => navigate("docs", page.slug)}
          className="group rounded-xl border border-border bg-card p-4 text-left transition-colors hover:bg-accent"
        >
          <span className="flex items-center justify-between gap-2 text-sm font-medium">
            {page.title}
            <span aria-hidden="true" className="text-muted-foreground transition-transform group-hover:translate-x-0.5">
              →
            </span>
          </span>
          <span className="mt-1 line-clamp-2 block text-xs leading-relaxed text-muted-foreground">
            {renderInline(page.description)}
          </span>
        </button>
      ))}
    </div>
  );
}

// A live example from the component pages. The preview is the official component,
// and the code shown below it is the snippet to copy.
function DemoBlock({ id }: { id: string }) {
  const { locale } = useI18n();
  const zh = locale === "zh";
  const demo = DEMOS[id];
  if (!demo) return null;
  const { Preview } = demo;

  return (
    <>
      <figure className="mt-6 overflow-hidden rounded-xl border border-border bg-card">
        <figcaption className="border-b border-border/60 px-4 py-2 text-xs uppercase tracking-wide text-muted-foreground">
          {zh ? demo.title.zh : demo.title.en}
        </figcaption>
        <div className="flex min-h-[200px] items-center overflow-x-auto p-6 [justify-content:safe_center] sm:p-10">
          <Preview />
        </div>
      </figure>
      <CodeBlock code={demo.code} lang="tsx" label={zh ? "程式碼" : "Code"} />
    </>
  );
}

export function DocsBlock({ block }: { block: DocsBlockData }) {
  switch (block.type) {
    case "p":
      return <p className="mt-4 leading-relaxed text-muted-foreground">{renderInline(block.text)}</p>;
    case "h2":
      return (
        <h2 id={block.id} className="mt-14 scroll-mt-28 text-2xl font-semibold tracking-tight first:mt-0 sm:text-3xl">
          {block.text}
        </h2>
      );
    case "list":
      return (
        <ul className="mt-4 space-y-2.5">
          {block.items.map((item) => (
            <li key={item} className="flex gap-3 text-sm leading-relaxed text-muted-foreground">
              <span aria-hidden="true" className="mt-2 size-1.5 shrink-0 rounded-full bg-honey" />
              <span>{renderInline(item)}</span>
            </li>
          ))}
        </ul>
      );
    case "steps":
      return <StepsBlock items={block.items} />;
    case "code":
      return <CodeBlock code={block.code} lang={block.lang} />;
    case "tabs":
      return <CodeTabs tabs={block.tabs} />;
    case "example":
      return <ExampleBlock id={block.id} />;
    case "table":
      return <TableBlock head={block.head} rows={block.rows} />;
    case "callout":
      return <CalloutBlock tone={block.tone} text={block.text} />;
    case "links":
      return <LinksBlock slugs={block.slugs} />;
    case "reference":
      return <DocsReference name={block.name} />;
    case "demo":
      return <DemoBlock id={block.id} />;
    default:
      return null;
  }
}
