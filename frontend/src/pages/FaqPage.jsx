import React from 'react';
import FaqItem from '../components/FaqItem';
import SectionEyebrow from '../components/SectionEyebrow';
import { colors, font, pageWrap } from '../theme';
import { getSection } from '../i18n';

export default function FaqPage({ lang }) {
  const faq = getSection(lang, 'faq');

  return (
    <section style={{ background: colors.white, color: colors.ink, padding: '96px 24px 120px', minHeight: '70vh' }}>
      <div style={{ ...pageWrap, maxWidth: 820 }}>
        <div style={{ marginBottom: 36, textAlign: 'center', display: 'flex', flexDirection: 'column', alignItems: 'center' }}>
          <SectionEyebrow>{faq.kicker || faq.title}</SectionEyebrow>
          <h1
            style={{
              margin: '22px 0 18px',
              fontFamily: font.serif,
              fontWeight: 500,
              fontSize: 'clamp(44px, 6vw, 68px)',
              lineHeight: 0.98,
              letterSpacing: '-0.04em',
              textAlign: 'center',
            }}
          >
            {faq.title}
          </h1>
          <p style={{ margin: 0, color: colors.muted, lineHeight: 1.85, fontSize: 16, maxWidth: 640, textAlign: 'center' }}>{faq.body}</p>
        </div>
        <div style={{ display: 'grid', gap: 14 }}>
          {faq.items.map((item) => (
            <FaqItem key={item.q} question={item.q} answer={item.a} />
          ))}
        </div>
      </div>
    </section>
  );
}
