"use client";

import { ArrowRight } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useAppStore } from "@/store/app-store";
import { useI18n } from "@/store/i18n-store";
import { getSiteDefinition } from "@/lib/hivemind-site-data.mjs";
import { Bullets, StepList } from "./site-blocks";
import { PageSection, Surface } from "./page-primitives";

export function SharePage() {
  const navigate = useAppStore((state) => state.navigate);
  const { locale } = useI18n();
  const zh = locale === "zh";
  const site = getSiteDefinition(locale);
  const copy = site.sections.share;

  return (
    <PageSection eyebrow={zh ? "電腦擁有者" : "For computer owners"} title={copy.title} body={copy.body}>
      <div className="mt-12 grid gap-12 lg:grid-cols-2 lg:items-start">
        <div>
          <StepList steps={copy.steps} />
          <div className="mt-10 flex flex-wrap gap-3">
            <Button size="lg" className="h-11" onClick={() => navigate("download")}>
              {zh ? "下載 Worker" : "Download Worker"}
              <ArrowRight aria-hidden="true" />
            </Button>
            <Button size="lg" variant="outline" className="h-11" onClick={() => navigate("security")}>
              {site.sections.trust.linkLabel}
            </Button>
          </div>
        </div>
        <div className="grid gap-4">
          <Surface>
            <h2 className="text-base font-semibold">{copy.panelTitle}</h2>
            <div className="mt-4">
              <Bullets items={copy.panelItems} />
            </div>
          </Surface>
          <Surface>
            <h2 className="text-base font-semibold">{site.sections.download.requirementsTitle}</h2>
            <div className="mt-4">
              <Bullets items={site.sections.download.requirements} />
            </div>
          </Surface>
        </div>
      </div>
    </PageSection>
  );
}
