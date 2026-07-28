"use client";

import { useI18n } from "@/store/i18n-store";
import { getSiteDefinition } from "@/lib/hivemind-site-data.mjs";
import { PageSection, Surface } from "./page-primitives";

export function DocsPage() {
  const { locale } = useI18n();
  const site = getSiteDefinition(locale);

  return (
    <PageSection
      eyebrow={locale === "zh" ? "部署與整合" : "Deployment and Integration"}
      title={locale === "zh" ? "先看清楚該部署哪一個節點，再開始整合。" : "Start by deploying the right node, then integrate against the right surface."}
      body={locale === "zh"
        ? "官方網站的文件頁只描述帳號中心端點與部署交接原則。真正的工作提交與節點管理，請在你部署的 Master 或 Worker 節點上進行。"
        : "The official docs page focuses on account-center endpoints and deployment handoff. Real task submission and worker management belong on the Master or Worker nodes you deploy."}
    >
      <div className="mt-10 overflow-hidden rounded-2xl border border-border/60 bg-card/40">
        <div className="grid grid-cols-[120px_1fr_1.1fr] gap-4 border-b border-border/60 px-6 py-4 text-xs uppercase tracking-wide text-muted-foreground">
          <div>Method</div>
          <div>Path</div>
          <div>{locale === "zh" ? "用途" : "Use"}</div>
        </div>
        {site.sections.docs.map((entry) => (
          <div key={`${entry.method}-${entry.path}`} className="grid grid-cols-[120px_1fr_1.1fr] gap-4 border-b border-border/40 px-6 py-4 text-sm last:border-b-0">
            <div className="font-mono-tech">{entry.method}</div>
            <div className="font-mono-tech text-muted-foreground">{entry.path}</div>
            <div>{entry.note}</div>
          </div>
        ))}
      </div>

      <div className="mt-8 grid gap-4 lg:grid-cols-2">
        <Surface>
          <h3 className="text-lg font-semibold">{locale === "zh" ? "部署 Master 節點" : "Deploy a Master node"}</h3>
          <p className="mt-2 text-sm leading-relaxed text-muted-foreground">
            {locale === "zh"
              ? "當你要提交工作、追蹤進度或控制任務時，請在自己的環境中部署 Master UI 與 HTTP API。"
              : "When you need task submission, progress tracking, or task control, deploy the Master UI and HTTP API in your own environment."}
          </p>
        </Surface>
        <Surface>
          <h3 className="text-lg font-semibold">{locale === "zh" ? "部署 Worker 節點" : "Deploy a Worker node"}</h3>
          <p className="mt-2 text-sm leading-relaxed text-muted-foreground">
            {locale === "zh"
              ? "當你要提供算力、設定資源價格或管理本地執行時，請在自己的環境中部署 Worker UI 與 HTTP API。"
              : "When you need to provide compute, configure pricing, or manage local execution, deploy the Worker UI and HTTP API in your own environment."}
          </p>
        </Surface>
      </div>
    </PageSection>
  );
}
