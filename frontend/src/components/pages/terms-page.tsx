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
      eyebrow={locale === "zh" ? "目前的規則" : "Current rules"}
      title={
        locale === "zh"
          ? "使用規範與服務限制"
          : "Usage rules and service limits"
      }
      body={terms.summary}
    >
      <Surface className="mt-10 border-honey/30 bg-honey/[0.05]">
        <div className="font-mono-tech text-xs uppercase tracking-[0.2em] text-honey">
          {locale === "zh" ? "請先閱讀" : "Read this first"}
        </div>
        <p className="mt-3 text-pretty leading-relaxed">
          {locale === "zh"
            ? "CPT 是工作用的內部額度，不是貨幣，也不能兌換或提領。分享電腦所得到的額度只能用於你自己的工作，不是收入。"
            : "CPT is an internal credit for tasks. It is not money, and it cannot be converted or withdrawn. Credit earned by sharing a computer can be used only for your own tasks. It is not income."}
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
          ? "這一頁說明服務目前的運作方式與限制，會隨服務變動而更新。它不是法律契約，也不取代你與提供服務的人之間的任何安排。"
          : "This page describes how the service works today and where its limits are. It is updated as the service changes. It is not a legal agreement and does not replace any arrangement between you and the people who run the service."}
      </p>
    </PageSection>
  );
}
