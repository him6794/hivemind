import React, { useEffect, useState } from 'react';
import SectionEyebrow from '../components/SectionEyebrow';
import { colors, font, glassStrong } from '../theme';
import { getSection } from '../i18n';

export default function FeaturesPage({ lang }) {
  const features = getSection(lang, 'features');
  const [active, setActive] = useState(features.sections[0]?.id || 'identity');

  useEffect(() => {
    const nodes = features.sections
      .map((section) => document.getElementById(`feature-${section.id}`))
      .filter(Boolean);

    if (!nodes.length) return undefined;

    const observer = new IntersectionObserver(
      (entries) => {
        const visible = entries
          .filter((entry) => entry.isIntersecting)
          .sort((a, b) => b.intersectionRatio - a.intersectionRatio);
        if (visible[0]?.target?.id) {
          setActive(visible[0].target.id.replace('feature-', ''));
        }
      },
      { rootMargin: '-30% 0px -45% 0px', threshold: [0.2, 0.4, 0.6] },
    );

    nodes.forEach((node) => observer.observe(node));
    return () => observer.disconnect();
  }, [features.sections]);

  return (
    <>
      <section style={{ background: colors.surface, color: colors.ink, padding: '56px 18px 80px' }}>
        <div style={{ maxWidth: 1180, margin: '0 auto' }}>
          <SectionEyebrow>{features.title}</SectionEyebrow>
          <h1
            style={{
              margin: '16px 0 12px',
              fontFamily: font.serif,
              fontWeight: 600,
              fontSize: 'clamp(34px, 4.8vw, 56px)',
              lineHeight: 1.1,
              letterSpacing: '-0.03em',
              maxWidth: 780,
            }}
          >
            {features.title}
          </h1>
          <p style={{ margin: '0 0 36px', maxWidth: 720, color: colors.muted, lineHeight: 1.8 }}>{features.body}</p>

          <div className="hm-feature-grid" style={{ display: 'grid', gridTemplateColumns: '0.28fr 0.72fr', gap: 28, alignItems: 'start' }}>
            <aside
              className="hm-feature-sidebar"
              style={{
                position: 'sticky',
                top: 96,
                alignSelf: 'start',
              }}
            >
              <nav style={{ display: 'grid', gap: 8 }}>
                {features.sections.map((section) => {
                  const isActive = active === section.id;
                  return (
                    <a
                      key={section.id}
                      href={`#feature-${section.id}`}
                      onClick={() => setActive(section.id)}
                      style={{
                        display: 'flex',
                        alignItems: 'center',
                        gap: 12,
                        padding: '10px 4px',
                        color: isActive ? colors.ink : colors.muted,
                        fontWeight: isActive ? 700 : 500,
                        transition: 'color 500ms ease, opacity 500ms ease',
                        textDecoration: 'none',
                      }}
                    >
                      <span
                        style={{
                          width: 6,
                          height: 6,
                          borderRadius: 999,
                          background: isActive ? colors.ink : 'transparent',
                          transition: 'background 500ms ease',
                        }}
                      />
                      {section.label}
                    </a>
                  );
                })}
              </nav>
            </aside>

            <div style={{ display: 'grid', gap: 28 }}>
              {features.sections.map((section, index) => {
                const reverse = index % 2 === 1;
                return (
                  <article
                    key={section.id}
                    id={`feature-${section.id}`}
                    style={{
                      display: 'grid',
                      gridTemplateColumns: '1fr 1fr',
                      gap: 18,
                      alignItems: 'center',
                      background: colors.white,
                      borderRadius: 18,
                      border: '1px solid rgba(15,23,42,0.08)',
                      padding: 22,
                      boxShadow: '0 18px 40px rgba(15,23,42,0.04)',
                    }}
                    className="hm-hero-grid"
                  >
                    <div style={{ order: reverse ? 2 : 1 }}>
                      <div style={{ fontFamily: font.display, color: colors.indigo, fontSize: 12, letterSpacing: '0.12em', textTransform: 'uppercase', fontWeight: 700 }}>
                        {section.label}
                      </div>
                      <h2 style={{ margin: '10px 0 10px', fontFamily: font.serif, fontSize: 28, lineHeight: 1.15, letterSpacing: '-0.02em' }}>
                        {section.title}
                      </h2>
                      <p style={{ margin: 0, color: colors.muted, lineHeight: 1.75, fontSize: 14 }}>{section.body}</p>
                    </div>
                    <div style={{ order: reverse ? 1 : 2 }}>
                      <pre
                        style={{
                          margin: 0,
                          padding: 18,
                          borderRadius: 14,
                          background: colors.dark,
                          color: '#e2e8f0',
                          fontSize: 12,
                          lineHeight: 1.7,
                          overflow: 'auto',
                          fontFamily: 'ui-monospace, SFMono-Regular, Menlo, monospace',
                          border: '1px solid rgba(255,255,255,0.08)',
                        }}
                      >
                        {String(section.visual).replace(/\\n/g, '\n')}
                      </pre>
                    </div>
                  </article>
                );
              })}
            </div>
          </div>
        </div>
      </section>

      <section style={{ background: colors.dark, color: colors.white, padding: '72px 18px 88px' }}>
        <div style={{ maxWidth: 1180, margin: '0 auto' }}>
          <SectionEyebrow dark>{features.quotesTitle}</SectionEyebrow>
          <h2
            style={{
              margin: '16px 0 28px',
              fontFamily: font.serif,
              fontWeight: 600,
              fontSize: 'clamp(30px, 4vw, 46px)',
              lineHeight: 1.12,
              letterSpacing: '-0.03em',
            }}
          >
            {features.quotesTitle}
          </h2>
          <div
            style={{
              columns: '280px 3',
              columnGap: 16,
            }}
          >
            {features.quotes.map((quote) => (
              <article
                key={quote.handle}
                style={{
                  ...glassStrong,
                  borderRadius: 18,
                  padding: 18,
                  marginBottom: 16,
                  breakInside: 'avoid',
                  display: 'grid',
                  gap: 12,
                }}
              >
                <p style={{ margin: 0, color: '#e2e8f0', fontSize: 14, lineHeight: 1.75 }}>{quote.quote}</p>
                <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
                  <div
                    style={{
                      width: 36,
                      height: 36,
                      borderRadius: 999,
                      background: 'linear-gradient(145deg, rgba(99,102,241,0.8), rgba(168,85,247,0.7))',
                    }}
                  />
                  <div>
                    <div style={{ fontWeight: 600 }}>{quote.name}</div>
                    <div style={{ color: 'rgba(226,232,240,0.6)', fontSize: 12 }}>{quote.handle}</div>
                  </div>
                </div>
              </article>
            ))}
          </div>
        </div>
      </section>
    </>
  );
}
