"use client";

import { useEffect, useMemo, useState } from "react";
import { ArrowLeft, ArrowRight, Menu } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { useAppStore } from "@/store/app-store";
import { useI18n } from "@/store/i18n-store";
import { getSiteDefinition } from "@/lib/hivemind-site-data.mjs";
import { DocsBlock, renderInline, type DocsBlockData } from "./docs-blocks";
import { referenceSections } from "./docs-reference";

type DocsPageData = { slug: string; group: string; title: string; description: string; blocks: DocsBlockData[] };
type DocsGroup = { id: string; title: string };
type TocItem = { id: string; text: string };

function tocFor(page: DocsPageData, locale: string): TocItem[] {
  return page.blocks.flatMap((block) => {
    if (block.type === "h2") return [{ id: block.id, text: block.text }];
    if (block.type === "reference") return referenceSections(block.name, locale);
    return [];
  });
}

function jumpTo(id: string) {
  document.getElementById(id)?.scrollIntoView({ behavior: "smooth", block: "start" });
}

// Marks the section nearest the top of the viewport. Sections are read from the
// rendered page, so the list must be rebuilt whenever the page changes.
function useActiveSection(ids: string[]) {
  const [active, setActive] = useState<string | undefined>(undefined);
  const key = ids.join("|");

  useEffect(() => {
    const elements = key
      .split("|")
      .filter(Boolean)
      .map((id) => document.getElementById(id))
      .filter((element): element is HTMLElement => element !== null);
    if (elements.length === 0) {
      setActive(undefined);
      return;
    }
    setActive(elements[0].id);
    const observer = new IntersectionObserver(
      (entries) => {
        const visible = entries
          .filter((entry) => entry.isIntersecting)
          .sort((a, b) => a.boundingClientRect.top - b.boundingClientRect.top);
        if (visible.length > 0) setActive(visible[0].target.id);
      },
      { rootMargin: "-120px 0px -60% 0px" }
    );
    elements.forEach((element) => observer.observe(element));
    return () => observer.disconnect();
  }, [key]);

  return active;
}

function DocsNav({
  groups,
  pages,
  activeSlug,
  label,
  onSelect,
}: {
  groups: DocsGroup[];
  pages: DocsPageData[];
  activeSlug: string;
  label: string;
  onSelect: (slug: string) => void;
}) {
  return (
    <nav aria-label={label} className="space-y-6">
      {groups.map((group) => (
        <div key={group.id}>
          <p className="mb-2 px-2 text-xs font-semibold uppercase tracking-wider text-muted-foreground">{group.title}</p>
          <ul className="space-y-0.5">
            {pages
              .filter((page) => page.group === group.id)
              .map((page) => {
                const current = page.slug === activeSlug;
                return (
                  <li key={page.slug}>
                    <button
                      type="button"
                      onClick={() => onSelect(page.slug)}
                      aria-current={current ? "page" : undefined}
                      className={`w-full rounded-md px-2 py-1.5 text-left text-sm transition-colors ${
                        current
                          ? "bg-accent font-medium text-foreground"
                          : "text-muted-foreground hover:bg-accent/60 hover:text-foreground"
                      }`}
                    >
                      {page.title}
                    </button>
                  </li>
                );
              })}
          </ul>
        </div>
      ))}
    </nav>
  );
}

export function DocsPage() {
  const { locale } = useI18n();
  const zh = locale === "zh";
  const docs = getSiteDefinition(locale).sections.docs as { groups: DocsGroup[]; pages: DocsPageData[] };
  const slug = useAppStore((state) => state.docsSlug);
  const navigate = useAppStore((state) => state.navigate);
  const [menuOpen, setMenuOpen] = useState(false);

  const index = Math.max(0, docs.pages.findIndex((entry) => entry.slug === slug));
  const page = docs.pages[index] ?? docs.pages[0];
  const previous = docs.pages[index - 1];
  const next = docs.pages[index + 1];
  const group = docs.groups.find((entry) => entry.id === page.group);
  const toc = useMemo(() => tocFor(page, locale), [page, locale]);
  const active = useActiveSection(toc.map((item) => item.id));
  const navLabel = zh ? "文件頁面" : "Documentation pages";

  const select = (target: string) => {
    setMenuOpen(false);
    navigate("docs", target);
  };

  return (
    <div className="mx-auto w-full max-w-7xl px-4 pb-16 pt-28 sm:px-6 sm:pb-24 sm:pt-32">
      <div className="lg:grid lg:grid-cols-[210px_minmax(0,1fr)] lg:gap-10 xl:grid-cols-[210px_minmax(0,1fr)_190px]">
        <aside className="hidden lg:sticky lg:top-28 lg:block lg:max-h-[calc(100vh-8rem)] lg:self-start lg:overflow-y-auto">
          <DocsNav groups={docs.groups} pages={docs.pages} activeSlug={page.slug} label={navLabel} onSelect={select} />
        </aside>

        <div className="min-w-0">
          <div className="mb-6 flex justify-end lg:hidden">
            <Button variant="outline" size="sm" onClick={() => setMenuOpen(true)}>
              <Menu aria-hidden="true" className="size-4" />
              {zh ? "所有頁面" : "All pages"}
            </Button>
          </div>

          <article className="max-w-3xl">
            <header>
              <p className="font-mono-tech text-xs uppercase tracking-[0.24em] text-honey">{group?.title}</p>
              <h1 className="mt-3 text-balance text-3xl font-semibold tracking-tight sm:text-4xl">{page.title}</h1>
              <p className="mt-4 text-pretty text-lg leading-relaxed text-muted-foreground">{renderInline(page.description)}</p>
              <div className="mt-6 flex flex-wrap items-center gap-2">
                {previous ? (
                  <Button variant="outline" size="sm" onClick={() => select(previous.slug)}>
                    <ArrowLeft aria-hidden="true" className="size-4" />
                    {previous.title}
                  </Button>
                ) : null}
                {next ? (
                  <Button variant="outline" size="sm" onClick={() => select(next.slug)}>
                    {next.title}
                    <ArrowRight aria-hidden="true" className="size-4" />
                  </Button>
                ) : null}
              </div>
            </header>

            <div className="mt-10">
              {page.blocks.map((block, blockIndex) => (
                <DocsBlock key={`${page.slug}-${blockIndex}`} block={block} />
              ))}
            </div>
          </article>

          <nav
            aria-label={zh ? "上一頁與下一頁" : "Previous and next pages"}
            className="mt-16 grid max-w-3xl gap-3 border-t border-border/60 pt-8 sm:grid-cols-2"
          >
            {previous ? (
              <button
                type="button"
                onClick={() => select(previous.slug)}
                className="rounded-xl border border-border bg-card p-4 text-left transition-colors hover:bg-accent"
              >
                <span className="text-xs uppercase tracking-wide text-muted-foreground">{zh ? "上一頁" : "Previous"}</span>
                <span className="mt-1 block text-sm font-medium">{previous.title}</span>
              </button>
            ) : (
              <span />
            )}
            {next ? (
              <button
                type="button"
                onClick={() => select(next.slug)}
                className="rounded-xl border border-border bg-card p-4 text-right transition-colors hover:bg-accent sm:col-start-2"
              >
                <span className="text-xs uppercase tracking-wide text-muted-foreground">{zh ? "下一頁" : "Next"}</span>
                <span className="mt-1 block text-sm font-medium">{next.title}</span>
              </button>
            ) : null}
          </nav>
        </div>

        <aside className="hidden xl:sticky xl:top-28 xl:block xl:self-start">
          {toc.length > 0 ? (
            <div>
              <p className="mb-3 text-xs font-semibold uppercase tracking-wider text-muted-foreground">
                {zh ? "本頁內容" : "On this page"}
              </p>
              <ul className="space-y-1.5 border-l border-border/60">
                {toc.map((item) => (
                  <li key={item.id}>
                    <button
                      type="button"
                      onClick={() => jumpTo(item.id)}
                      className={`-ml-px block border-l py-0.5 pl-3 text-left text-sm transition-colors ${
                        active === item.id
                          ? "border-honey text-foreground"
                          : "border-transparent text-muted-foreground hover:text-foreground"
                      }`}
                    >
                      {item.text}
                    </button>
                  </li>
                ))}
              </ul>
            </div>
          ) : null}
        </aside>
      </div>

      <Dialog open={menuOpen} onOpenChange={setMenuOpen}>
        <DialogContent className="max-h-[80dvh] max-w-sm overflow-y-auto">
          <DialogTitle>{zh ? "所有頁面" : "All pages"}</DialogTitle>
          <DialogDescription className="sr-only">{zh ? "文件頁面清單" : "List of documentation pages"}</DialogDescription>
          <div className="mt-4">
            <DocsNav groups={docs.groups} pages={docs.pages} activeSlug={page.slug} label={navLabel} onSelect={select} />
          </div>
        </DialogContent>
      </Dialog>
    </div>
  );
}
