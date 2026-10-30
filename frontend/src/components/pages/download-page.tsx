"use client";

import { useI18n } from "@/store/i18n-store";
import { getSiteDefinition } from "@/lib/hivemind-site-data.mjs";
import { isPublishedDownload, releaseDownloads } from "@/lib/downloads.mjs";
import { PageSection, Surface } from "./page-primitives";

function StepList({ title, steps }: { title: string; steps: string[] }) {
  return (
    <Surface>
      <h2 className="text-lg font-semibold">{title}</h2>
      <ol className="mt-4 space-y-3 text-sm leading-relaxed text-muted-foreground">
        {steps.map((step, index) => (
          <li key={step} className="flex gap-3">
            <span className="font-mono-tech text-xs text-honey">{String(index + 1).padStart(2, "0")}</span>
            <span>{step}</span>
          </li>
        ))}
      </ol>
    </Surface>
  );
}

export function DownloadPage() {
  const { locale } = useI18n();
  const site = getSiteDefinition(locale);
  const copy = site.sections.download;

  return (
    <PageSection eyebrow={locale === "zh" ? "下載" : "Download"} title={copy.title} body={copy.intro}>
      <div className="mt-10 grid min-w-0 grid-cols-1 gap-4 lg:grid-cols-2">
        {copy.packages.map((pkg) => {
          const release = releaseDownloads[pkg.id as keyof typeof releaseDownloads];
          const published = isPublishedDownload(release);
          return (
            <Surface key={pkg.id}>
              <h2 className="text-lg font-semibold">{pkg.name}</h2>
              <p className="mt-2 text-sm leading-relaxed text-muted-foreground">{pkg.summary}</p>
              {published ? (
                <div className="mt-6 space-y-3 text-sm">
                  <p>
                    {copy.versionLabel}: {release.version}
                  </p>
                  <p className="break-all font-mono-tech text-xs text-muted-foreground">
                    {copy.hashLabel}: {release.sha256}
                  </p>
                  <a
                    href={release.url}
                    rel="noopener noreferrer"
                    className="inline-flex h-11 items-center justify-center rounded-md bg-primary px-5 text-sm font-medium text-primary-foreground hover:bg-primary/90"
                  >
                    {copy.downloadLabel}
                  </a>
                </div>
              ) : (
                <p className="mt-6 text-sm text-muted-foreground">{copy.unavailable}</p>
              )}
            </Surface>
          );
        })}
      </div>

      <div className="mt-10 grid min-w-0 grid-cols-1 gap-4 lg:grid-cols-2">
        <StepList title={copy.masterStepsTitle} steps={copy.masterSteps} />
        <StepList title={copy.workerStepsTitle} steps={copy.workerSteps} />
      </div>

      <div className="mt-10 grid min-w-0 grid-cols-1 gap-4 lg:grid-cols-2">
        <Surface>
          <h2 className="text-lg font-semibold">{copy.requirementsTitle}</h2>
          <ul className="mt-4 space-y-2 text-sm leading-relaxed text-muted-foreground">
            {copy.requirements.map((item) => (
              <li key={item}>{item}</li>
            ))}
          </ul>
        </Surface>
        <Surface>
          <h2 className="text-lg font-semibold">{copy.verifyTitle}</h2>
          <p className="mt-3 text-sm leading-relaxed text-muted-foreground">{copy.verifyBody}</p>
          <pre className="mt-4 overflow-x-auto rounded-xl border border-border/60 bg-background p-4 text-xs leading-relaxed">
            <code className="font-mono-tech">{copy.verifyCommand}</code>
          </pre>
        </Surface>
      </div>

      <div className="mt-10 grid min-w-0 grid-cols-1 gap-4 lg:grid-cols-2">
        <Surface>
          <h2 className="text-lg font-semibold">{copy.warningTitle}</h2>
          <p className="mt-3 text-sm leading-relaxed text-muted-foreground">{copy.warningBody}</p>
        </Surface>
        <Surface>
          <h2 className="text-lg font-semibold">{copy.advancedTitle}</h2>
          <p className="mt-3 text-sm leading-relaxed text-muted-foreground">{copy.advancedBody}</p>
        </Surface>
      </div>
    </PageSection>
  );
}
