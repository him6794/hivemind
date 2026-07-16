import React, { useEffect, useState } from 'react';
import SectionEyebrow from '../components/SectionEyebrow';
import { colors, font, glassStrong, pageWrap } from '../theme';
import { getSection } from '../i18n';

export default function FeaturesPage({ lang }) {
  const features = getSection(lang, 'features');
  const [active, setActive] = useState(features.sections[0]?.id || 'gateway');

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
        if (visible[0]?.target?.id) setActive(visible[0].target.id.replace('feature-', ''));
      },
      { rootMargin: '-28% 0px -48% 0px', threshold: [0.2, 0.45, 0.7] },
    );
    nodes.forEach((node) => observer.observe(node));
    return () => observer.disconnect();
  }, [features.sections]);

  return (
    <>
      <section style={{ background: colors.surface, color: colors.ink, padding: '96px 24px 112px' }}>
        <div style={pageWrap}>
          <div style={{ maxWidth: 760, marginBottom: 48 }}>
            <SectionEyebrow>{features.kicker || features.title}</SectionEyebrow>
            <h1
              style={{
                margin: '22px 0 18px',
                fontFamily: font.serif,
                fontWeight: 500,
                fontSize: 'clamp(44px, 6vw, 68px)',
                lineHeight: 0.98,
                letterSpacing: '-0.04em',
              }}
            >
              {features.title}
            </h1>
            <p style={{ margin: 0, maxWidth: 640, color: colors.muted, lineHeight: 1.85, fontSize: 16 }}>{features.body}</p>
          </div>

          <div className="hm-feature-grid" style={{ display: 'grid', gridTemplateColumns: '0.24fr 0.76fr', gap: 48, alignItems: 'start' }}>
            <aside className="hm-feature-sidebar" style={{ position: 'sticky', top: 108 }}>
              <nav style={{ display: 'grid', gap: 4 }}>
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
                        gap: 14,
                        padding: '12px 4px',
                        color: isActive ? colors.ink : colors.muted,
                        fontWeight: isActive ? 700 : 500,
                        transition: 'color 500ms ease',
                        fontSize: 14,
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

            <div style={{ display: 'grid', gap: 20 }}>
              {features.sections.map((section) => (
                <article
                  key={section.id}
                  id={`feature-${section.id}`}
                  style={{
                    background: colors.white,
                    borderRadius: 28,
                    border: '1px solid rgba(15,23,42,0.08)',
                    padding: 36,
                    boxShadow: '0 18px 40px rgba(15,23,42,0.04)',
                  }}
                >
                  <div
                    style={{
                      fontFamily: font.display,
                      color: '#0e7490',
                      fontSize: 11,
                      letterSpacing: '0.14em',
                      textTransform: 'uppercase',
                      fontWeight: 700,
                    }}
                  >
                    {section.label}
                  </div>
                  <h2
                    style={{
                      margin: '14px 0 12px',
                      fontFamily: font.serif,
                      fontWeight: 500,
                      fontSize: 'clamp(28px, 3.4vw, 36px)',
                      lineHeight: 1.12,
                      letterSpacing: '-0.03em',
                    }}
                  >
                    {section.title}
                  </h2>
                  <p style={{ margin: '0 0 20px', color: colors.muted, lineHeight: 1.85, fontSize: 15, maxWidth: 720 }}>
                    {section.body}
                  </p>
                  <div style={{ display: 'flex', gap: 10, flexWrap: 'wrap' }}>
                    {section.points.map((point) => (
                      <span
                        key={point}
                        style={{
                          padding: '9px 14px',
                          borderRadius: 999,
                          background: 'rgba(6,182,212,0.08)',
                          color: '#0f766e',
                          fontSize: 13,
                          fontWeight: 500,
                        }}
                      >
                        {point}
                      </span>
                    ))}
                  </div>
                </article>
              ))}
            </div>
          </div>
        </div>
      </section>

      <section style={{ background: colors.dark, color: colors.white, padding: '104px 24px 112px' }}>
        <div style={pageWrap}>
          <div style={{ maxWidth: 720, marginBottom: 40 }}>
            <SectionEyebrow dark>{features.storiesLabel || features.storiesTitle}</SectionEyebrow>
            <h2
              style={{
                margin: '22px 0 0',
                fontFamily: font.serif,
                fontWeight: 500,
                fontSize: 'clamp(36px, 5vw, 56px)',
                lineHeight: 0.98,
                letterSpacing: '-0.04em',
              }}
            >
              {features.storiesTitle}
            </h2>
          </div>
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(3, minmax(0, 1fr))', gap: 18 }} className="hm-dual-grid">
            {features.stories.map((story) => (
              <article
                key={story.name}
                style={{ ...glassStrong, borderRadius: 28, padding: 28, display: 'grid', gap: 18, minHeight: 240 }}
              >
                <p style={{ margin: 0, color: 'rgba(226,232,240,0.82)', fontSize: 15, lineHeight: 1.8 }}>{story.quote}</p>
                <div>
                  <div style={{ fontFamily: font.serif, fontWeight: 600, fontSize: 18, letterSpacing: '-0.02em' }}>{story.name}</div>
                  <div
                    style={{
                      color: 'rgba(226,232,240,0.52)',
                      fontSize: 12,
                      marginTop: 4,
                      fontFamily: font.display,
                      letterSpacing: '0.1em',
                      textTransform: 'uppercase',
                    }}
                  >
                    {story.role}
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
