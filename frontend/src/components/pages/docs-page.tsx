"use client";

import { useI18n } from "@/store/i18n-store";
import { getSiteDefinition } from "@/lib/hivemind-site-data.mjs";
import { Surface } from "./page-primitives";

type Copy = { zh: string; en: string };

const t = (locale: string, copy: Copy) => (locale === "zh" ? copy.zh : copy.en);

const SECTIONS: { id: string; label: Copy }[] = [
  { id: "quickstart", label: { zh: "快速上手", en: "Quick start" } },
  { id: "api", label: { zh: "API 參考", en: "API reference" } },
  { id: "fields", label: { zh: "任務欄位", en: "Task fields" } },
  { id: "language", label: { zh: "語言參考", en: "Language reference" } },
  { id: "example", label: { zh: "完整範例", en: "Worked example" } },
  { id: "limits", label: { zh: "限制", en: "Limits" } },
  { id: "billing", label: { zh: "計費", en: "Billing" } },
  { id: "failures", label: { zh: "失敗代碼", en: "Failure codes" } },
];

function CodeBlock({ children }: { children: string }) {
  return (
    <pre className="mt-4 overflow-x-auto rounded-xl border border-border/60 bg-background/60 p-4 text-xs leading-relaxed">
      <code className="font-mono-tech">{children}</code>
    </pre>
  );
}

function DocSection({
  id,
  title,
  body,
  children,
}: {
  id: string;
  title: string;
  body?: string;
  children?: React.ReactNode;
}) {
  return (
    <section id={id} className="scroll-mt-28 border-t border-border/60 pt-12 first:border-t-0 first:pt-0">
      <h2 className="text-2xl font-semibold tracking-tight sm:text-3xl">{title}</h2>
      {body ? <p className="mt-3 text-pretty leading-relaxed text-muted-foreground">{body}</p> : null}
      {children}
    </section>
  );
}

function Pill({ children }: { children: React.ReactNode }) {
  return (
    <span className="rounded-md bg-honey/10 px-2 py-0.5 font-mono-tech text-xs text-honey ring-1 ring-honey/20">
      {children}
    </span>
  );
}

export function DocsPage() {
  const { locale } = useI18n();
  const site = getSiteDefinition(locale);
  const docs = site.sections.docs;

  return (
    <div className="mx-auto max-w-7xl px-4 py-16 sm:px-6 sm:py-24">
      <div className="max-w-3xl">
        <div className="font-mono-tech text-xs uppercase tracking-[0.24em] text-honey">
          {t(locale, { zh: "文件", en: "Documentation" })}
        </div>
        <h1 className="mt-3 text-balance text-3xl font-semibold tracking-tight sm:text-5xl">
          {t(locale, {
            zh: "從建立帳號到跑完第一份工作。",
            en: "From an account to your first finished job.",
          })}
        </h1>
        <p className="mt-4 text-pretty leading-relaxed text-muted-foreground">
          {t(locale, {
            zh: "以下的路由、欄位、限制與失敗代碼，都對應目前執行中的系統行為。任務 API 位於你自己部署的 Master 節點，不在本網站。",
            en: "Every route, field, limit, and failure code below matches how the running system behaves today. The task API lives on the Master node you deploy, not on this website.",
          })}
        </p>
      </div>

      <div className="mt-14 grid gap-12 lg:grid-cols-[200px_1fr] lg:gap-16">
        <nav aria-label={t(locale, { zh: "文件目錄", en: "On this page" })} className="lg:sticky lg:top-28 lg:self-start">
          <div className="mb-3 text-xs uppercase tracking-wide text-muted-foreground">
            {t(locale, { zh: "本頁內容", en: "On this page" })}
          </div>
          <ul className="flex flex-wrap gap-x-4 gap-y-2 lg:flex-col lg:gap-2">
            {SECTIONS.map((section) => (
              <li key={section.id}>
                <a
                  href={`#${section.id}`}
                  className="text-sm text-muted-foreground transition-colors hover:text-honey"
                >
                  {t(locale, section.label)}
                </a>
              </li>
            ))}
          </ul>
        </nav>

        <div className="flex min-w-0 flex-col gap-12">
          <DocSection
            id="quickstart"
            title={t(locale, { zh: "快速上手", en: "Quick start" })}
          >
            <div className="mt-6 grid gap-4 sm:grid-cols-2">
              {docs.quickstart.map((item: { step: string; title: string; body: string }) => (
                <Surface key={item.step}>
                  <div className="font-mono-tech text-xs text-honey">STEP {item.step}</div>
                  <h3 className="mt-2 text-base font-semibold">{item.title}</h3>
                  <p className="mt-2 text-sm leading-relaxed text-muted-foreground">{item.body}</p>
                </Surface>
              ))}
            </div>
          </DocSection>

          <DocSection id="api" title={t(locale, { zh: "API 參考", en: "API reference" })}>
            {docs.groups.map((group: {
              id: string;
              title: string;
              note: string;
              rows: { method: string; path: string; note: string }[];
            }) => (
              <div key={group.id} className="mt-8">
                <h3 className="text-lg font-semibold">{group.title}</h3>
                <p className="mt-2 text-sm leading-relaxed text-muted-foreground">{group.note}</p>
                <div className="mt-4 overflow-hidden rounded-2xl border border-border/60 bg-card/40">
                  {group.rows.map((row) => (
                    <div
                      key={`${row.method}-${row.path}`}
                      className="grid gap-2 border-b border-border/40 px-5 py-4 text-sm last:border-b-0 sm:grid-cols-[72px_minmax(0,1fr)] sm:gap-4 lg:grid-cols-[72px_240px_minmax(0,1fr)]"
                    >
                      <div className="font-mono-tech text-xs text-honey">{row.method}</div>
                      <div className="font-mono-tech text-xs break-all">{row.path}</div>
                      <div className="text-sm text-muted-foreground">{row.note}</div>
                    </div>
                  ))}
                </div>
              </div>
            ))}
          </DocSection>

          <DocSection
            id="fields"
            title={t(locale, { zh: "任務欄位", en: "Task fields" })}
            body={t(locale, {
              zh: "POST /api/tasks 的請求主體。標示必填的欄位若缺少或不合法，會在送出當下被拒絕。",
              en: "The request body for POST /api/tasks. A required field that is missing or invalid is rejected at submission time.",
            })}
          >
            <div className="mt-6 overflow-hidden rounded-2xl border border-border/60 bg-card/40">
              {docs.taskFields.map((field: { name: string; type: string; required: string; note: string }) => (
                <div
                  key={field.name}
                  className="grid gap-2 border-b border-border/40 px-5 py-4 last:border-b-0 lg:grid-cols-[180px_minmax(0,1fr)] lg:gap-4"
                >
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="font-mono-tech text-sm">{field.name}</span>
                    <Pill>{field.type}</Pill>
                  </div>
                  <div className="text-sm text-muted-foreground">
                    <span className="mr-2 text-xs uppercase tracking-wide text-foreground/70">{field.required}</span>
                    {field.note}
                  </div>
                </div>
              ))}
            </div>
          </DocSection>

          <DocSection
            id="language"
            title={t(locale, { zh: "語言參考", en: "Language reference" })}
            body={docs.language.intro}
          >
            <div className="mt-6 grid gap-4 lg:grid-cols-2">
              <Surface>
                <h3 className="text-base font-semibold">{t(locale, { zh: "陳述式", en: "Statements" })}</h3>
                <ul className="mt-3 space-y-1.5">
                  {docs.language.statements.map((line: string) => (
                    <li key={line} className="font-mono-tech text-xs text-muted-foreground">{line}</li>
                  ))}
                </ul>
              </Surface>
              <Surface>
                <h3 className="text-base font-semibold">{t(locale, { zh: "運算式", en: "Expressions" })}</h3>
                <ul className="mt-3 space-y-1.5">
                  {docs.language.expressions.map((line: string) => (
                    <li key={line} className="font-mono-tech text-xs text-muted-foreground">{line}</li>
                  ))}
                </ul>
              </Surface>
              <Surface>
                <h3 className="text-base font-semibold">{t(locale, { zh: "內建函式", en: "Builtins" })}</h3>
                <ul className="mt-3 space-y-2">
                  {docs.language.builtins.map((builtin: { sig: string; note: string }) => (
                    <li key={builtin.sig} className="text-sm">
                      <span className="font-mono-tech text-xs text-honey">{builtin.sig}</span>
                      <span className="ml-2 text-muted-foreground">{builtin.note}</span>
                    </li>
                  ))}
                </ul>
              </Surface>
              <Surface>
                <h3 className="text-base font-semibold">{t(locale, { zh: "規則", en: "Rules" })}</h3>
                <ul className="mt-3 space-y-2">
                  {docs.language.rules.map((rule: string) => (
                    <li key={rule} className="text-sm leading-relaxed text-muted-foreground">{rule}</li>
                  ))}
                </ul>
              </Surface>
            </div>

            <Surface className="mt-4 border-honey/25 bg-honey/[0.04]">
              <h3 className="text-base font-semibold">
                {t(locale, { zh: "v0 不支援", en: "Not available in v0" })}
              </h3>
              <div className="mt-3 flex flex-wrap gap-2">
                {docs.language.forbidden.map((item: string) => (
                  <span
                    key={item}
                    className="rounded-md border border-border/60 bg-background/50 px-2.5 py-1 text-xs text-muted-foreground"
                  >
                    {item}
                  </span>
                ))}
              </div>
            </Surface>
          </DocSection>

          <DocSection
            id="example"
            title={t(locale, { zh: "完整範例", en: "Worked example" })}
            body={t(locale, {
              zh: "一個讀取 JSON 輸入、以使用者函式計分、再加總的工作。",
              en: "A job that reads its JSON input, scores each row with a user function, and totals the result.",
            })}
          >
            <div className="mt-2">
              <div className="mt-6 text-sm font-medium">{t(locale, { zh: "task_source", en: "task_source" })}</div>
              <CodeBlock>{docs.language.example}</CodeBlock>

              <div className="mt-6 text-sm font-medium">
                {t(locale, { zh: "JSON 輸入（torrent 欄位）", en: "JSON input (torrent field)" })}
              </div>
              <CodeBlock>{docs.language.exampleInput}</CodeBlock>
              <p className="mt-3 text-sm text-muted-foreground">{docs.language.exampleNote}</p>

              <div className="mt-6 text-sm font-medium">
                {t(locale, { zh: "送出（在你的 Master 節點上）", en: "Submit it (against your Master node)" })}
              </div>
              <CodeBlock>{docs.language.submitExample}</CodeBlock>
            </div>
          </DocSection>

          <DocSection
            id="limits"
            title={t(locale, { zh: "限制", en: "Limits" })}
            body={t(locale, {
              zh: "這些是目前的實際上限。超過上限的工作會被拒絕或中止，而不是靜默截斷。",
              en: "These are the ceilings in force today. A job that crosses one is rejected or stopped, never silently truncated.",
            })}
          >
            <div className="mt-6 overflow-hidden rounded-2xl border border-border/60 bg-card/40">
              {docs.limits.map((limit: { name: string; value: string; note: string }) => (
                <div
                  key={limit.name}
                  className="grid gap-1 border-b border-border/40 px-5 py-4 last:border-b-0 lg:grid-cols-[minmax(0,1fr)_200px_minmax(0,1fr)] lg:gap-4"
                >
                  <div className="text-sm font-medium">{limit.name}</div>
                  <div className="font-mono-tech text-sm text-honey">{limit.value}</div>
                  <div className="text-sm text-muted-foreground">{limit.note}</div>
                </div>
              ))}
            </div>
          </DocSection>

          <DocSection id="billing" title={docs.billing.title} body={docs.billing.body}>
            <CodeBlock>{docs.billing.formula}</CodeBlock>
            <div className="mt-4 grid gap-4 sm:grid-cols-2">
              {docs.billing.rows.map((row: { name: string; value: string }) => (
                <Surface key={row.name} className="flex items-center justify-between">
                  <span className="text-sm text-muted-foreground">{row.name}</span>
                  <span className="font-mono-tech text-lg text-honey">{row.value}</span>
                </Surface>
              ))}
            </div>

            <div className="mt-8">
              <h3 className="text-lg font-semibold">
                {t(locale, { zh: "函式級計價", en: "Function-level pricing" })}
              </h3>
              <p className="mt-2 text-sm leading-relaxed text-muted-foreground">
                {t(locale, {
                  zh: "以下是每個可呼叫函式的固定成本；引數、函式本體與迴圈內實際執行的運算，會再逐項加入 usage units。",
                  en: "These are the fixed costs for each callable function form. Argument evaluation, function bodies, and loop work add their own usage units on top.",
                })}
              </p>
              <div className="mt-4 overflow-hidden rounded-2xl border border-border/60 bg-card/40">
                {docs.billing.functionRows.map((row: {
                  id: string;
                  name: string;
                  price: string;
                  note: string;
                }) => (
                  <div
                    key={row.id}
                    className="grid gap-2 border-b border-border/40 px-5 py-4 last:border-b-0 lg:grid-cols-[220px_240px_minmax(0,1fr)] lg:gap-4"
                  >
                    <div className="font-mono-tech text-xs text-honey">{row.name}</div>
                    <div className="text-sm font-medium">{row.price}</div>
                    <div className="text-sm leading-relaxed text-muted-foreground">{row.note}</div>
                  </div>
                ))}
              </div>
            </div>

            <div className="mt-8">
              <h3 className="text-lg font-semibold">
                {t(locale, { zh: "回執帳單範例", en: "Receipt-backed examples" })}
              </h3>
              <p className="mt-2 text-sm leading-relaxed text-muted-foreground">
                {t(locale, {
                  zh: "以下數字來自執行回執；每個範例都把 usage unit 與 1 CPT 基本呼叫費分開列出。",
                  en: "Each example is backed by an execution receipt and separates usage units from the 1 CPT base invocation charge.",
                })}
              </p>
              <div className="mt-4 grid gap-4 lg:grid-cols-2">
                {docs.billing.examples.map((example: {
                  id: string;
                  title: string;
                  program: string;
                  receiptUsageUnits: number;
                  totalCpt: number;
                  breakdown: string;
                }) => (
                  <Surface key={example.id} className="flex flex-col gap-3">
                    <div className="flex items-start justify-between gap-4">
                      <h4 className="text-base font-semibold">{example.title}</h4>
                      <span className="shrink-0 font-mono-tech text-sm text-honey">{example.totalCpt} CPT</span>
                    </div>
                    <CodeBlock>{example.program}</CodeBlock>
                    <div className="grid gap-2 text-sm sm:grid-cols-2">
                      <div className="rounded-xl bg-background/50 p-3">
                        <div className="text-xs uppercase tracking-wide text-muted-foreground">usage units</div>
                        <div className="mt-1 font-mono-tech text-lg text-honey">{example.receiptUsageUnits}</div>
                      </div>
                      <div className="rounded-xl bg-background/50 p-3">
                        <div className="text-xs uppercase tracking-wide text-muted-foreground">total CPT</div>
                        <div className="mt-1 font-mono-tech text-lg text-honey">{example.totalCpt}</div>
                      </div>
                    </div>
                    <p className="text-sm leading-relaxed text-muted-foreground">{example.breakdown}</p>
                  </Surface>
                ))}
              </div>
            </div>

            <div className="mt-8">
              <h3 className="text-lg font-semibold">
                {t(locale, { zh: "平台支援矩陣", en: "Platform support matrix" })}
              </h3>
              <div className="mt-4 overflow-hidden rounded-2xl border border-border/60 bg-card/40">
                {docs.billing.platforms.map((platform: {
                  id: string;
                  name: string;
                  status: string;
                  proof: string;
                }) => (
                  <div
                    key={platform.id}
                    className="grid gap-2 border-b border-border/40 px-5 py-4 last:border-b-0 lg:grid-cols-[180px_140px_minmax(0,1fr)] lg:gap-4"
                  >
                    <div className="text-sm font-medium">{platform.name}</div>
                    <div className="font-mono-tech text-xs text-honey">{platform.status}</div>
                    <div className="text-sm leading-relaxed text-muted-foreground">{platform.proof}</div>
                  </div>
                ))}
              </div>
            </div>

            <ul className="mt-4 space-y-2">
              {docs.billing.notes.map((note: string) => (
                <li key={note} className="text-sm leading-relaxed text-muted-foreground">{note}</li>
              ))}
            </ul>

            <Surface className="mt-6 border-honey/25 bg-honey/[0.04]">
              <h3 className="text-base font-semibold">{docs.settlement.title}</h3>
              <p className="mt-2 text-sm leading-relaxed text-muted-foreground">{docs.settlement.body}</p>
            </Surface>
          </DocSection>

          <DocSection
            id="failures"
            title={t(locale, { zh: "失敗代碼", en: "Failure codes" })}
            body={t(locale, {
              zh: "每一次失敗的執行都會在回執中帶一個代碼。",
              en: "Every failed execution carries one of these codes in its receipt.",
            })}
          >
            <div className="mt-6 overflow-hidden rounded-2xl border border-border/60 bg-card/40">
              {docs.failures.map((failure: { code: string; note: string }) => (
                <div
                  key={failure.code}
                  className="grid gap-1 border-b border-border/40 px-5 py-3.5 last:border-b-0 lg:grid-cols-[220px_minmax(0,1fr)] lg:gap-4"
                >
                  <div className="font-mono-tech text-xs text-honey">{failure.code}</div>
                  <div className="text-sm text-muted-foreground">{failure.note}</div>
                </div>
              ))}
            </div>
          </DocSection>
        </div>
      </div>
    </div>
  );
}
