import React, { useEffect, useState } from 'react';
import SectionEyebrow from '../components/SectionEyebrow';
import VibeInput from '../components/VibeInput';
import { colors, font, glassSoft, glassStrong, primaryButton, secondaryButton } from '../theme';
import { getSection } from '../i18n';

export default function HomePage({ lang, t, navigate, masterUi, workerUi }) {
  const home = getSection(lang, 'home');
  const [typed, setTyped] = useState('');
  const [prompt, setPrompt] = useState('');

  useEffect(() => {
    let i = 0;
    setTyped('');
    const text = home.title;
    const id = setInterval(() => {
      i += 1;
      setTyped(text.slice(0, i));
      if (i >= text.length) clearInterval(id);
    }, 24);
    return () => clearInterval(id);
  }, [home.title]);

  return (
    <>
      <section
        style={{
          minHeight: '110vh',
          padding: '48px 18px 120px',
          display: 'flex',
          flexDirection: 'column',
          justifyContent: 'center',
          position: 'relative',
        }}
      >
        <div style={{ maxWidth: 980, margin: '0 auto', width: '100%', textAlign: 'center' }}>
          <div style={{ display: 'inline-flex', alignItems: 'center', gap: 8, marginBottom: 18 }}>
            <span
              style={{
                padding: '4px 10px',
                borderRadius: 999,
                background: 'rgba(99,102,241,0.2)',
                border: '1px solid rgba(165,180,252,0.35)',
                color: '#c7d2fe',
                fontSize: 11,
                fontWeight: 700,
                letterSpacing: '0.12em',
                textTransform: 'uppercase',
                fontFamily: font.display,
              }}
            >
              {home.badge}
            </span>
            <span style={{ color: 'rgba(226,232,240,0.72)', fontSize: 13 }}>{home.badgeText}</span>
          </div>

          <h1
            style={{
              margin: '0 auto 18px',
              maxWidth: 920,
              fontFamily: font.serif,
              fontWeight: 600,
              fontSize: 'clamp(40px, 6.2vw, 72px)',
              lineHeight: 1.1,
              letterSpacing: '-0.03em',
              minHeight: '2.4em',
            }}
          >
            {typed}
            <span className="hm-cursor" style={{ color: '#a5b4fc' }}>|</span>
          </h1>

          <p style={{ margin: '0 auto', maxWidth: 720, color: 'rgba(226,232,240,0.78)', fontSize: 16, lineHeight: 1.8 }}>
            {home.subtitle}
          </p>

          <div style={{ display: 'flex', justifyContent: 'center', gap: 10, flexWrap: 'wrap', marginTop: 24 }}>
            <button type="button" className="hm-btn" style={primaryButton} onClick={() => navigate('/account')}>
              {t('common.enter')}
            </button>
            <button type="button" className="hm-btn" style={secondaryButton} onClick={() => navigate('/vpn')}>
              {t('common.learnVpn')}
            </button>
          </div>

          <VibeInput
            value={prompt}
            onChange={(e) => setPrompt(e.target.value)}
            placeholder={home.promptPlaceholder}
            hint={home.promptHint}
          />
        </div>

        <div
          style={{
            position: 'absolute',
            left: '50%',
            bottom: 28,
            transform: 'translateX(-50%)',
            width: 'min(920px, calc(100% - 36px))',
            ...glassStrong,
            borderRadius: 20,
            padding: 18,
          }}
        >
          <div
            className="hm-hero-grid"
            style={{
              display: 'grid',
              gridTemplateColumns: '1fr 1fr',
              gap: 18,
              alignItems: 'center',
            }}
          >
            <div>
              <div style={{ fontSize: 12, letterSpacing: '0.12em', textTransform: 'uppercase', color: 'rgba(226,232,240,0.6)', fontFamily: font.display }}>
                {home.frameworks}
              </div>
              <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap', marginTop: 12 }}>
                {home.frameworksList.map((item) => (
                  <span key={item} style={{ ...glassSoft, borderRadius: 999, padding: '8px 12px', fontSize: 13 }}>
                    {item}
                  </span>
                ))}
              </div>
            </div>
            <div>
              <div style={{ fontSize: 12, letterSpacing: '0.12em', textTransform: 'uppercase', color: 'rgba(226,232,240,0.6)', fontFamily: font.display }}>
                {home.integrations}
              </div>
              <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap', marginTop: 12 }}>
                {home.integrationsList.map((item) => (
                  <span
                    key={item}
                    style={{
                      ...glassSoft,
                      borderRadius: 999,
                      padding: '8px 12px',
                      fontSize: 13,
                      color: 'rgba(226,232,240,0.78)',
                      transition: 'color 500ms ease, border-color 500ms ease',
                    }}
                  >
                    {item}
                  </span>
                ))}
              </div>
            </div>
          </div>
        </div>
      </section>

      <section style={{ background: colors.surface, color: colors.ink, padding: '80px 18px' }}>
        <div style={{ maxWidth: 980, margin: '0 auto', textAlign: 'center' }}>
          <SectionEyebrow>{home.badgeText}</SectionEyebrow>
          <h2
            style={{
              margin: '16px 0 12px',
              fontFamily: font.serif,
              fontWeight: 600,
              fontSize: 'clamp(30px, 4vw, 48px)',
              lineHeight: 1.12,
              letterSpacing: '-0.03em',
            }}
          >
            {home.ctaTitle}
          </h2>
          <p style={{ margin: '0 auto 24px', maxWidth: 680, color: colors.muted, lineHeight: 1.8 }}>{home.ctaBody}</p>
          <div style={{ display: 'flex', justifyContent: 'center', gap: 10, flexWrap: 'wrap' }}>
            <a href={masterUi} target="_blank" rel="noreferrer" className="hm-btn" style={primaryButton}>
              {t('common.openMaster')}
            </a>
            <a href={workerUi} target="_blank" rel="noreferrer" className="hm-btn" style={{ ...secondaryButton, color: colors.ink, border: '1px solid rgba(15,23,42,0.12)', background: colors.white }}>
              {t('common.openWorker')}
            </a>
            <button type="button" className="hm-btn" style={{ ...secondaryButton, color: colors.ink, border: '1px solid rgba(15,23,42,0.12)', background: colors.white }} onClick={() => navigate('/features')}>
              {t('nav.features')}
            </button>
          </div>
        </div>
      </section>
    </>
  );
}
