"use client";

import { useI18n } from "@/store/i18n-store";
import { getSiteDefinition } from "@/lib/hivemind-site-data.mjs";
import { Surface } from "./page-primitives";

type Step = { step: string; title: string; body: string };
type Named = { form: string; note: string };
type Field = { name: string; type: string; required: string; note: string };
type ApiRow = { method: string; path: string; note: string };

// Section ids and titles for the reference pages. The page's "On this page"
// list and the headings below both read from here, so they cannot disagree.
export function referenceSections(name: string, locale: string): { id: string; text: string }[] {
  const zh = locale === "zh";
  const docs = getSiteDefinition(locale).sections.docs;
  switch (name) {
    case "language":
      return [
        { id: "statements", text: zh ? "陳述式" : "Statements" },
        { id: "expressions", text: zh ? "運算式" : "Expressions" },
        { id: "builtins", text: zh ? "內建函式" : "Builtins" },
        { id: "unavailable", text: zh ? "CPU hcode 不提供" : "Not available in CPU hcode" },
        { id: "rules", text: zh ? "規則" : "Rules" },
      ];
    case "settings":
      return [{ id: "fields", text: zh ? "欄位" : "Fields" }];
    case "billing":
      return [
        { id: "formula", text: zh ? "計算公式" : "The formula" },
        { id: "numbers", text: zh ? "重要數字" : "Key numbers" },
        { id: "cases", text: zh ? "兩個範例" : "Two worked cases" },
        { id: "functions", text: zh ? "函式級計價" : "Function-level pricing" },
        { id: "receipts", text: zh ? "收據範例" : "Receipt examples" },
        { id: "rules", text: zh ? "計費規則" : "Rules for every charge" },
        { id: "settlement", text: docs.settlement.title as string },
      ];
    case "api":
      return [
        { id: "account", text: docs.api.groups[0].title as string },
        { id: "tasks", text: docs.api.groups[1].title as string },
        { id: "example", text: zh ? "送出工作的範例" : "Example request" },
      ];
    case "platforms":
      return [
        { id: "support", text: zh ? "平台支援狀態" : "Platform support" },
        { id: "versions", text: docs.versions.runtimeTitle as string },
      ];
    default:
      return [];
  }
}

function sectionTitle(name: string, locale: string, id: string) {
  return referenceSections(name, locale).find((section) => section.id === id)?.text ?? id;
}

function Heading({ id, children }: { id: string; children: React.ReactNode }) {
  return (
    <h2 id={id} className="mt-14 scroll-mt-28 text-2xl font-semibold tracking-tight first:mt-0 sm:text-3xl">
      {children}
    </h2>
  );
}

function Table({ children }: { children: React.ReactNode }) {
  return <div className="mt-6 overflow-hidden rounded-xl border border-border bg-card">{children}</div>;
}

function TableRow({ className, children }: { className?: string; children: React.ReactNode }) {
  return (
    <div className={`grid gap-2 border-b border-border/40 px-5 py-4 text-sm last:border-b-0 ${className ?? ""}`}>
      {children}
    </div>
  );
}

function Pre({ children }: { children: string }) {
  return (
    <pre className="mt-6 overflow-x-auto rounded-xl border border-border/60 bg-background p-4 text-xs leading-relaxed">
      <code className="font-mono-tech">{children}</code>
    </pre>
  );
}

function FlowView({ docs }: { docs: { flow: Step[] } }) {
  return (
    <ol className="mt-6 grid gap-4 md:grid-cols-2">
      {docs.flow.map((item) => (
        <li key={item.step}>
          <Surface className="h-full">
            <div className="font-mono-tech text-xs text-honey">{item.step}</div>
            <h3 className="mt-2 text-base font-semibold">{item.title}</h3>
            <p className="mt-2 text-sm leading-relaxed text-muted-foreground">{item.body}</p>
          </Surface>
        </li>
      ))}
    </ol>
  );
}

function TermsView({ docs }: { docs: { terms: { term: string; body: string }[] } }) {
  return (
    <dl className="mt-6 divide-y overflow-hidden rounded-xl border border-border bg-card">
      {docs.terms.map((item) => (
        <div key={item.term} className="grid gap-1 px-5 py-4 sm:grid-cols-[220px_minmax(0,1fr)] sm:gap-6">
          <dt className="text-sm font-medium">{item.term}</dt>
          <dd className="text-sm leading-relaxed text-muted-foreground">{item.body}</dd>
        </div>
      ))}
    </dl>
  );
}

function QuickstartView({ docs }: { docs: { quickstart: Step[] } }) {
  return (
    <ol className="mt-6 space-y-8">
      {docs.quickstart.map((item) => (
        <li key={item.step} className="grid gap-3 sm:grid-cols-[56px_minmax(0,1fr)] sm:gap-6">
          <div className="font-mono-tech text-sm text-honey">{item.step}</div>
          <div className="min-w-0">
            <h3 className="text-base font-semibold">{item.title}</h3>
            <p className="mt-2 text-sm leading-relaxed text-muted-foreground">{item.body}</p>
          </div>
        </li>
      ))}
    </ol>
  );
}

function LanguageView({ docs, locale }: { docs: any; locale: string }) {
  const name = "language";
  return (
    <>
      <p className="mt-4 leading-relaxed text-muted-foreground">{docs.language.intro}</p>
      <Heading id="statements">{sectionTitle(name, locale, "statements")}</Heading>
      <Table>
        {docs.language.statements.map((item: Named) => (
          <TableRow key={item.form} className="lg:grid-cols-[minmax(0,1fr)_minmax(0,1.3fr)] lg:gap-6">
            <code className="font-mono-tech break-all text-xs text-honey">{item.form}</code>
            <div className="leading-relaxed text-muted-foreground">{item.note}</div>
          </TableRow>
        ))}
      </Table>
      <Heading id="expressions">{sectionTitle(name, locale, "expressions")}</Heading>
      <Table>
        {docs.language.expressions.map((item: Named) => (
          <TableRow key={item.form} className="lg:grid-cols-[minmax(0,1fr)_minmax(0,1.3fr)] lg:gap-6">
            <code className="font-mono-tech break-all text-xs text-honey">{item.form}</code>
            <div className="leading-relaxed text-muted-foreground">{item.note}</div>
          </TableRow>
        ))}
      </Table>
      <Heading id="builtins">{sectionTitle(name, locale, "builtins")}</Heading>
      <Table>
        {docs.language.builtins.map((item: { sig: string; note: string }) => (
          <TableRow key={item.sig} className="lg:grid-cols-[minmax(0,1fr)_minmax(0,1.3fr)] lg:gap-6">
            <code className="font-mono-tech break-all text-xs text-honey">{item.sig}</code>
            <div className="leading-relaxed text-muted-foreground">{item.note}</div>
          </TableRow>
        ))}
      </Table>
      <Heading id="unavailable">{sectionTitle(name, locale, "unavailable")}</Heading>
      <Surface className="mt-6">
        <ul className="space-y-2 text-sm leading-relaxed text-muted-foreground">
          {docs.language.unavailable.map((item: string) => (
            <li key={item}>{item}</li>
          ))}
        </ul>
      </Surface>
      <Heading id="rules">{sectionTitle(name, locale, "rules")}</Heading>
      <Surface className="mt-6">
        <ul className="space-y-2 text-sm leading-relaxed text-muted-foreground">
          {docs.language.rules.map((item: string) => (
            <li key={item}>{item}</li>
          ))}
        </ul>
      </Surface>
    </>
  );
}

function SettingsView({ docs, locale }: { docs: any; locale: string }) {
  return (
    <>
      <p className="mt-4 leading-relaxed text-muted-foreground">{docs.settings.intro}</p>
      <Heading id="fields">{sectionTitle("settings", locale, "fields")}</Heading>
      <Table>
        {docs.settings.fields.map((field: Field) => (
          <TableRow key={field.name} className="lg:grid-cols-[220px_minmax(0,1fr)] lg:gap-4">
            <div className="flex flex-wrap items-center gap-2">
              <code className="font-mono-tech text-sm">{field.name}</code>
              <span className="rounded-md bg-honey/10 px-2 py-0.5 font-mono-tech text-xs text-honey ring-1 ring-honey/20">
                {field.type}
              </span>
            </div>
            <div className="leading-relaxed text-muted-foreground">
              <span className="mr-2 text-xs uppercase tracking-wide text-foreground/70">{field.required}</span>
              {field.note}
            </div>
          </TableRow>
        ))}
      </Table>
    </>
  );
}

function BillingView({ docs, locale }: { docs: any; locale: string }) {
  const zh = locale === "zh";
  const name = "billing";
  return (
    <>
      <p className="mt-4 leading-relaxed text-muted-foreground">{docs.billing.body}</p>
      <Heading id="formula">{sectionTitle(name, locale, "formula")}</Heading>
      <Pre>{docs.billing.formula}</Pre>
      <Heading id="numbers">{sectionTitle(name, locale, "numbers")}</Heading>
      <Table>
        {docs.billing.rows.map((row: { name: string; value: string }) => (
          <TableRow key={row.name} className="lg:grid-cols-[220px_minmax(0,1fr)] lg:gap-4">
            <div className="font-medium">{row.name}</div>
            <div className="leading-relaxed text-muted-foreground">{row.value}</div>
          </TableRow>
        ))}
      </Table>
      <Heading id="cases">{sectionTitle(name, locale, "cases")}</Heading>
      <div className="mt-6 grid min-w-0 grid-cols-1 gap-4 lg:grid-cols-2">
        {docs.billing.cases.map((item: { id: string; title: string; lines: string[] }) => (
          <Surface key={item.id}>
            <h3 className="text-base font-semibold">{item.title}</h3>
            <ul className="mt-3 space-y-2 text-sm leading-relaxed text-muted-foreground">
              {item.lines.map((line) => (
                <li key={line}>{line}</li>
              ))}
            </ul>
          </Surface>
        ))}
      </div>
      <Heading id="functions">{sectionTitle(name, locale, "functions")}</Heading>
      <p className="mt-4 text-sm leading-relaxed text-muted-foreground">
        {zh
          ? "每種可呼叫的函式形式都有固定成本。引數計算、函式本體與迴圈內的工作，會另外加上 usage unit。"
          : "Each callable function form has a fixed cost. Argument evaluation, function bodies, and loop work add their own usage units."}
      </p>
      <Table>
        {docs.billing.functionRows.map((row: { id: string; name: string; price: string; note: string }) => (
          <TableRow key={row.id} className="lg:grid-cols-[220px_240px_minmax(0,1fr)] lg:gap-4">
            <div className="font-mono-tech text-xs text-honey">{row.name}</div>
            <div className="font-medium">{row.price}</div>
            <div className="leading-relaxed text-muted-foreground">{row.note}</div>
          </TableRow>
        ))}
      </Table>
      <Heading id="receipts">{sectionTitle(name, locale, "receipts")}</Heading>
      <p className="mt-4 text-sm leading-relaxed text-muted-foreground">
        {zh
          ? "每個範例都是單一副本的 receipt。工作的扣款會合計所有有效副本的用量，不是某一個 receipt 上的數字。"
          : "Each example is the receipt of one replica. A task is charged for the valid usage of all replicas together, not for the number on one receipt."}
      </p>
      <div className="mt-6 grid min-w-0 grid-cols-1 gap-4 lg:grid-cols-2">
        {docs.billing.examples.map((example: { id: string; title: string; program: string; receiptUsageUnits: number; breakdown: string }) => (
          <Surface key={example.id} className="flex min-w-0 flex-col gap-3">
            <div className="flex items-start justify-between gap-4">
              <h3 className="text-base font-semibold">{example.title}</h3>
              <span className="shrink-0 font-mono-tech text-sm text-honey">
                {example.receiptUsageUnits} {zh ? "個 usage unit" : "usage units"}
              </span>
            </div>
            <pre className="overflow-x-auto rounded-lg border border-border/60 bg-background p-3 text-xs leading-relaxed">
              <code className="font-mono-tech">{example.program}</code>
            </pre>
            <p className="text-sm leading-relaxed text-muted-foreground">{example.breakdown}</p>
          </Surface>
        ))}
      </div>
      <Heading id="rules">{sectionTitle(name, locale, "rules")}</Heading>
      <ul className="mt-6 space-y-3">
        {docs.billing.notes.map((note: string) => (
          <li key={note} className="flex gap-3 text-sm leading-relaxed text-muted-foreground">
            <span aria-hidden="true" className="mt-2 size-1.5 shrink-0 rounded-full bg-honey" />
            <span>{note}</span>
          </li>
        ))}
      </ul>
      <Heading id="settlement">{sectionTitle(name, locale, "settlement")}</Heading>
      <Surface className="mt-6 border-honey/25 bg-honey/[0.04]">
        <p className="text-sm leading-relaxed text-muted-foreground">{docs.settlement.body}</p>
      </Surface>
    </>
  );
}

function LimitsView({ docs }: { docs: any }) {
  return (
    <Table>
      {docs.limits.map((limit: { id: string; name: string; value: string; note: string }) => (
        <TableRow key={limit.id} className="lg:grid-cols-[minmax(0,1fr)_220px_minmax(0,1.3fr)] lg:gap-4">
          <div className="font-medium">{limit.name}</div>
          <div className="font-mono-tech text-sm text-honey">{limit.value}</div>
          <div className="leading-relaxed text-muted-foreground">{limit.note}</div>
        </TableRow>
      ))}
    </Table>
  );
}

function FailuresView({ docs, locale }: { docs: any; locale: string }) {
  const zh = locale === "zh";
  return (
    <>
      <Table>
        {docs.failures.map((failure: { code: string; note: string }) => (
          <TableRow key={failure.code} className="lg:grid-cols-[240px_minmax(0,1fr)] lg:gap-4">
            <div className="font-mono-tech text-xs text-honey">{failure.code}</div>
            <div className="leading-relaxed text-muted-foreground">{failure.note}</div>
          </TableRow>
        ))}
      </Table>
      <p className="mt-6 text-sm leading-relaxed text-muted-foreground">
        {zh
          ? "未達 quorum 的工作沒有被接受的結果，也不會扣款。"
          : "A task that does not reach a quorum has no accepted result and is not charged."}
      </p>
    </>
  );
}

function ApiView({ docs, locale }: { docs: any; locale: string }) {
  const name = "api";
  return (
    <>
      <p className="mt-4 leading-relaxed text-muted-foreground">{docs.api.intro}</p>
      {docs.api.groups.map((group: { id: string; title: string; note: string; rows: ApiRow[] }) => (
        <div key={group.id}>
          <Heading id={group.id}>{sectionTitle(name, locale, group.id)}</Heading>
          <p className="mt-4 text-sm leading-relaxed text-muted-foreground">{group.note}</p>
          <Table>
            {group.rows.map((row) => (
              <TableRow
                key={`${row.method}-${row.path}`}
                className="sm:grid-cols-[72px_minmax(0,1fr)] lg:grid-cols-[72px_280px_minmax(0,1fr)] lg:gap-4"
              >
                <div className="font-mono-tech text-xs text-honey">{row.method}</div>
                <div className="font-mono-tech break-all text-xs">{row.path}</div>
                <div className="leading-relaxed text-muted-foreground">{row.note}</div>
              </TableRow>
            ))}
          </Table>
        </div>
      ))}
      <p className="mt-6 text-sm leading-relaxed text-muted-foreground">{docs.api.footnote}</p>
      <Heading id="example">{sectionTitle(name, locale, "example")}</Heading>
      <p className="mt-4 text-sm leading-relaxed text-muted-foreground">{docs.api.submitNote}</p>
      <Pre>{docs.api.submitExample}</Pre>
    </>
  );
}

function PlatformsView({ docs, locale }: { docs: any; locale: string }) {
  const name = "platforms";
  return (
    <>
      <p className="mt-4 leading-relaxed text-muted-foreground">{docs.versions.platformsBody}</p>
      <Heading id="support">{sectionTitle(name, locale, "support")}</Heading>
      <Table>
        {docs.billing.platforms.map((platform: { id: string; name: string; status: string; check: string }) => (
          <TableRow key={platform.id} className="lg:grid-cols-[180px_140px_minmax(0,1fr)] lg:gap-4">
            <div className="font-medium">{platform.name}</div>
            <div className="font-mono-tech text-xs text-honey">{platform.status}</div>
            <div className="leading-relaxed text-muted-foreground">{platform.check}</div>
          </TableRow>
        ))}
      </Table>
      <Heading id="versions">{sectionTitle(name, locale, "versions")}</Heading>
      <p className="mt-6 leading-relaxed text-muted-foreground">{docs.versions.runtimeBody}</p>
      <p className="mt-4 leading-relaxed text-muted-foreground">{docs.versions.gpuBody}</p>
    </>
  );
}

export function DocsReference({ name }: { name: string }) {
  const { locale } = useI18n();
  const docs = getSiteDefinition(locale).sections.docs as any;
  switch (name) {
    case "flow":
      return <FlowView docs={docs} />;
    case "terms":
      return <TermsView docs={docs} />;
    case "quickstart":
      return <QuickstartView docs={docs} />;
    case "language":
      return <LanguageView docs={docs} locale={locale} />;
    case "settings":
      return <SettingsView docs={docs} locale={locale} />;
    case "billing":
      return <BillingView docs={docs} locale={locale} />;
    case "limits":
      return <LimitsView docs={docs} />;
    case "failures":
      return <FailuresView docs={docs} locale={locale} />;
    case "api":
      return <ApiView docs={docs} locale={locale} />;
    case "platforms":
      return <PlatformsView docs={docs} locale={locale} />;
    default:
      return null;
  }
}
