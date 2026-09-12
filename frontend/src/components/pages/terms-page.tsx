"use client";

import { useI18n } from "@/store/i18n-store";
import { getSiteDefinition } from "@/lib/hivemind-site-data.mjs";
import { PageSection, Surface } from "./page-primitives";

export function TermsPage() {
  const { locale } = useI18n();
  const site = getSiteDefinition(locale);
  const terms = site.sections.terms;

  return (
    <PageSection
      eyebrow={locale === "zh" ? "使用規範" : "Usage rules"}
      title={
        locale === "zh"
          ? "這套系統現在還不是一個公開市集。"
          : "This is not an open marketplace yet."
      }
      body={terms.summary}
    >
      <Surface className="mt-10 border-honey/30 bg-honey/[0.05]">
        <div className="font-mono-tech text-xs uppercase tracking-[0.2em] text-honey">
          {locale === "zh" ? "請先讀這一段" : "Read this first"}
        </div>
        <p className="mt-3 text-pretty leading-relaxed">
          {locale === "zh"
            ? "CPT 是工作額度，不是貨幣，也沒有兌換或提領管道。分享電腦得到的是可以用來執行自己工作的額度，不是收入。"
            : "CPT is a work credit, not money. There is no conversion or withdrawal path. Sharing a computer earns credit for your own tasks, not income."}
        </p>
      </Surface>

      <div className="mt-6 grid gap-4 lg:grid-cols-2">
        {terms.groups.map((group: { title: string; items: string[] }) => (
          <Surface key={group.title}>
            <h3 className="text-lg font-semibold">{group.title}</h3>
            <ul className="mt-4 space-y-2.5">
              {group.items.map((item) => (
                <li key={item} className="flex gap-2.5 text-sm leading-relaxed text-muted-foreground">
                  <span aria-hidden="true" className="mt-2 size-1 shrink-0 rounded-full bg-honey" />
                  <span>{item}</span>
                </li>
              ))}
            </ul>
          </Surface>
        ))}
      </div>

      <p className="mt-8 max-w-3xl text-sm leading-relaxed text-muted-foreground">
        {locale === "zh"
          ? "這一頁描述系統目前的行為與界線，會隨系統改變而更新。它不是法律契約，也不取代你與提供服務的人之間的任何協議。"
          : "This page describes how the system behaves and where its edges are today. It is not a legal agreement and does not replace any arrangement you have with the people running the service."}
      </p>
    </PageSection>
  );
}
