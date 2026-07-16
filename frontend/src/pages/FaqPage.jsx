import React from 'react';
import FaqItem from '../components/FaqItem';
import SectionEyebrow from '../components/SectionEyebrow';
import { colors, font } from '../theme';
import { getSection } from '../i18n';

export default function FaqPage({ lang }) {
  const faq = getSection(lang, 'faq');

  return (
    <section style={{ background: colors.white, color: colors.ink, padding: '56px 18px 90px', minHeight: '70vh' }}>
      <div style={{ maxWidth: 820, margin: '0 auto' }}>
        <SectionEyebrow>{faq.title}</SectionEyebrow>
        <h1
          style={{
            margin: '16px 0 12px',
            fontFamily: font.serif,
            fontWeight: 600,
            fontSize: 'clamp(34px, 4.8vw, 52px)',
            lineHeight: 1.1,
            letterSpacing: '-0.03em',
          }}
        >
          {faq.title}
        </h1>
        <p style={{ margin: '0 0 28px', color: colors.muted, lineHeight: 1.8 }}>{faq.body}</p>
        <div style={{ display: 'grid', gap: 12 }}>
          {faq.items.map((item) => (
            <FaqItem key={item.q} question={item.q} answer={item.a} />
          ))}
        </div>
      </div>
    </section>
  );
}
