import React, { useEffect, useState } from 'react';
import SectionEyebrow from '../components/SectionEyebrow';
import { colors, font, glassStrong, pageWrap, primaryButton, secondaryButton } from '../theme';
import { getSection } from '../i18n';

export default function HomePage({ lang, t, navigate }) {
  const home = getSection(lang, 'home');
  const [typed, setTyped] = useState('');

  useEffect(() => {
    let i = 0;
    setTyped('');
    const text = home.heroTitle;
    const id = setInterval(() => {
      i += 1;
      setTyped(text.slice(0, i));
      if (i >= text.length) clearInterval(id);
    }, 34);
    return () => clearInterval(id);
  }, [home.heroTitle]);

  const trust = home.trust || ['BARE METAL', 'TRUSTLESS', 'REALTIME', 'GLOBAL'];

  return (
    <>
      <section style={{ position: 'relative', padding: '108px 24px 72px', overflow: 'hidden' }}>
        <div style={{ ...pageWrap, display: 'flex', flexDirection: 'column', alignItems: 'center', textAlign: 'center' }}>
          <div
            style={{
              display: 'inline-flex',
              alignItems: 'center',
              justifyContent: 'center',
              gap: 8,
              padding: '6px 12px',
              borderRadius: 999,
              background: 'rgba(6,182,212,0.1)',
              border: '1px solid rgba(34,211,238,0.2)',
              color: '#22d3ee',
              fontSize: 10,
              fontWeight: 700,
              letterSpacing: '0.16em',
              textTransform: 'uppercase',
              fontFamily: font.display,
              marginBottom: 28,
            }}
          >
            {home.badge || home.bannerLabel}
          </div>

          <h1
            style={{
              margin: '0 auto 24px',
              fontFamily: font.serif,
              fontWeight: 500,
              fontSize: 'clamp(44px, 8vw, 88px)',
              lineHeight: 1.05,
              letterSpacing: '-0.04em',
              maxWidth: 980,
              width: '100%',
              textWrap: 'balance',
              textAlign: 'center',
              minHeight: '1.15em',
            }}
          >
            {typed}
            <span className="hm-cursor" style={{ color: '#67e8f9' }}>|</span>
          </h1>

          <p
            style={{
              margin: '0 auto 34px',
              maxWidth: 680,
              color: 'rgba(226,232,240,0.72)',
              fontSize: 17,
              lineHeight: 1.85,
              textAlign: 'center',
            }}
          >
            {home.subtitle}
          </p>

          <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap', justifyContent: 'center', marginBottom: 36 }}>
            <button type="button" className="hm-btn" style={{ ...primaryButton, padding: '16px 28px', fontSize: 13 }} onClick={() => navigate('/download')}>
              {home.primaryCta}
            </button>
            <button type="button" className="hm-btn hm-btn-secondary" style={{ ...secondaryButton, padding: '16px 28px', fontSize: 13 }} onClick={() => navigate('/account')}>
              {home.secondaryCta}
            </button>
          </div>

          <div
            style={{
              display: 'flex',
              gap: 18,
              flexWrap: 'wrap',
              justifyContent: 'center',
              color: 'rgba(226,232,240,0.42)',
              fontFamily: font.display,
              fontSize: 11,
              letterSpacing: '0.16em',
              fontWeight: 600,
            }}
          >
            {trust.map((item) => (
              <span key={item}>{item}</span>
            ))}
          </div>
        </div>
      </section>

      <section style={{ background: colors.white, color: colors.ink, padding: '112px 24px' }}>
        <div style={pageWrap}>
          <div className="hm-hero-grid" style={{ display: 'grid', gridTemplateColumns: '1.05fr 0.95fr', gap: 64, alignItems: 'start' }}>
            <div>
              <SectionEyebrow>{home.whatLabel}</SectionEyebrow>
              <h2
                style={{
                  margin: '22px 0 18px',
                  fontFamily: font.serif,
                  fontWeight: 500,
                  fontSize: 'clamp(40px, 5.5vw, 64px)',
                  lineHeight: 0.98,
                  letterSpacing: '-0.04em',
                  maxWidth: 560,
                }}
              >
                {home.whatTitle}
              </h2>
              <p style={{ margin: 0, color: colors.muted, fontSize: 16, lineHeight: 1.85, maxWidth: 520 }}>{home.whatBody}</p>
            </div>

            <div style={{ display: 'grid', gap: 16 }}>
              {home.whatPoints.map((item, index) => {
                const dark = index % 2 === 0;
                return (
                  <article
                    key={item.title}
                    style={{
                      background: dark ? colors.dark : colors.white,
                      color: dark ? colors.white : colors.ink,
                      borderRadius: 28,
                      border: dark ? '1px solid rgba(255,255,255,0.08)' : '1px solid rgba(15,23,42,0.08)',
                      padding: 28,
                      boxShadow: dark ? '0 18px 40px rgba(15,23,42,0.18)' : '0 12px 28px rgba(15,23,42,0.04)',
                    }}
                  >
                    <div
                      style={{
                        fontFamily: font.display,
                        fontSize: 11,
                        letterSpacing: '0.14em',
                        textTransform: 'uppercase',
                        color: dark ? '#67e8f9' : '#0e7490',
                        marginBottom: 12,
                        fontWeight: 700,
                      }}
                    >
                      0{index + 1}
                    </div>
                    <h3 style={{ margin: '0 0 10px', fontFamily: font.serif, fontSize: 28, letterSpacing: '-0.02em', lineHeight: 1.15 }}>
                      {item.title}
                    </h3>
                    <p style={{ margin: 0, color: dark ? 'rgba(226,232,240,0.72)' : colors.muted, lineHeight: 1.75, fontSize: 14 }}>
                      {item.body}
                    </p>
                  </article>
                );
              })}
            </div>
          </div>
        </div>
      </section>

      <section style={{ background: colors.panel, color: colors.ink, padding: '104px 24px' }}>
        <div style={{ ...pageWrap, textAlign: 'center', marginBottom: 40, display: 'flex', flexDirection: 'column', alignItems: 'center' }}>
          <SectionEyebrow>{home.rolesLabel || home.whatLabel}</SectionEyebrow>
          <h2 style={{ ...sectionTitle, marginTop: 18, marginBottom: 10, textAlign: 'center' }}>{home.rolesTitle || home.requestorTitle}</h2>
          <p style={{ margin: '0 auto', maxWidth: 560, color: colors.muted, lineHeight: 1.8, textAlign: 'center' }}>{home.rolesBody || home.bannerBody}</p>
        </div>
        <div style={pageWrap}>
          <div className="hm-dual-grid" style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 20 }}>
            <article style={roleCard}>
              <div style={roleHead}>
                <div style={roleTag}>{home.requestorKicker}</div>
                <h3 style={roleTitle}>{home.requestorTitle}</h3>
              </div>
              <p style={roleBody}>{home.requestorBody}</p>
              <button type="button" className="hm-btn" style={{ ...primaryButton, alignSelf: 'center' }} onClick={() => navigate('/download')}>
                {home.requestorCta}
              </button>
            </article>
            <article style={roleCard}>
              <div style={roleHead}>
                <div style={roleTag}>{home.providerKicker}</div>
                <h3 style={roleTitle}>{home.providerTitle}</h3>
              </div>
              <p style={roleBody}>{home.providerBody}</p>
              <button type="button" className="hm-btn" style={{ ...primaryButton, alignSelf: 'center' }} onClick={() => navigate('/download')}>
                {home.providerCta}
              </button>
            </article>
          </div>
        </div>
      </section>

      <section style={{ background: colors.dark, color: colors.white, padding: '88px 24px' }}>
        <div style={pageWrap}>
          <div
            className="hm-hero-grid"
            style={{
              ...glassStrong,
              borderRadius: 40,
              padding: '48px 40px',
              display: 'grid',
              gridTemplateColumns: '1.4fr auto',
              gap: 28,
              alignItems: 'center',
            }}
          >
            <div>
              <div style={{ fontFamily: font.display, fontSize: 11, letterSpacing: '0.16em', textTransform: 'uppercase', color: '#67e8f9', fontWeight: 700 }}>
                CPT
              </div>
              <h3 style={{ margin: '14px 0 14px', fontFamily: font.serif, fontSize: 'clamp(30px, 4vw, 42px)', letterSpacing: '-0.03em', lineHeight: 1.1 }}>
                {home.valueTitle}
              </h3>
              <p style={{ margin: 0, color: 'rgba(226,232,240,0.7)', lineHeight: 1.8, maxWidth: 700, fontSize: 16 }}>{home.valueBody}</p>
            </div>
            <button type="button" className="hm-btn" style={{ ...primaryButton, padding: '16px 28px' }} onClick={() => navigate('/account')}>
              {home.valueCta}
            </button>
          </div>
        </div>
      </section>

      <section style={{ background: colors.white, color: colors.ink, padding: '112px 24px' }}>
        <div style={{ maxWidth: 900, margin: '0 auto', textAlign: 'center', display: 'flex', flexDirection: 'column', alignItems: 'center' }}>
          <h2 style={{ ...sectionTitle, marginBottom: 18, fontSize: 'clamp(40px, 5.5vw, 64px)', lineHeight: 0.98, textAlign: 'center' }}>{home.finalTitle}</h2>
          <p style={{ margin: '0 auto 30px', color: colors.muted, fontSize: 17, lineHeight: 1.85, maxWidth: 620, textAlign: 'center' }}>{home.finalBody}</p>
          <div style={{ display: 'flex', gap: 14, justifyContent: 'center', flexWrap: 'wrap' }}>
            <button type="button" className="hm-btn" style={{ ...primaryButton, padding: '16px 30px' }} onClick={() => navigate('/download')}>
              {home.finalCta}
            </button>
            <button
              type="button"
              className="hm-btn"
              style={{
                ...secondaryButton,
                padding: '16px 30px',
                color: colors.ink,
                background: 'transparent',
                border: '1px solid transparent',
                boxShadow: 'none',
              }}
              onClick={() => navigate('/faq')}
            >
              {t('nav.faq')}
            </button>
          </div>
        </div>
      </section>
    </>
  );
}

const sectionTitle = {
  margin: 0,
  fontFamily: font.serif,
  fontWeight: 500,
  fontSize: 'clamp(34px, 4.6vw, 48px)',
  lineHeight: 1.08,
  letterSpacing: '-0.03em',
};

const roleCard = {
  background: colors.white,
  borderRadius: 28,
  border: '1px solid rgba(15,23,42,0.08)',
  padding: 36,
  display: 'grid',
  gap: 14,
  boxShadow: '0 18px 40px rgba(15,23,42,0.04)',
  textAlign: 'center',
  justifyItems: 'center',
  alignContent: 'start',
};

const roleHead = {
  display: 'grid',
  gap: 8,
  justifyItems: 'center',
};

const roleTag = {
  display: 'inline-flex',
  alignItems: 'center',
  justifyContent: 'center',
  width: 'fit-content',
  padding: '5px 10px',
  borderRadius: 999,
  background: 'rgba(6,182,212,0.1)',
  color: '#0e7490',
  fontSize: 11,
  fontWeight: 700,
  letterSpacing: '0.04em',
  textTransform: 'none',
  fontFamily: font.display,
  whiteSpace: 'nowrap',
  lineHeight: 1.1,
  margin: 0,
};

const roleTitle = {
  margin: 0,
  fontFamily: font.serif,
  fontSize: 34,
  letterSpacing: '-0.03em',
  lineHeight: 1.12,
  fontWeight: 500,
  textAlign: 'center',
};

const roleBody = {
  margin: 0,
  color: colors.muted,
  lineHeight: 1.8,
  fontSize: 15,
  textAlign: 'center',
  maxWidth: 420,
};
