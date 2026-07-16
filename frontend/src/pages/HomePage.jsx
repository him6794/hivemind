import React, { useEffect, useState } from 'react';
import SectionEyebrow from '../components/SectionEyebrow';
import { colors, font, glassSoft, glassStrong, primaryButton, secondaryButton } from '../theme';
import { getSection } from '../i18n';

export default function HomePage({ lang, t, navigate, masterUi, workerUi }) {
  const home = getSection(lang, 'home');
  const [lineIndex, setLineIndex] = useState(0);
  const [typed, setTyped] = useState('');

  useEffect(() => {
    const full = home.heroLines.join(' ');
    let i = 0;
    setTyped('');
    setLineIndex(0);
    const id = setInterval(() => {
      i += 1;
      const next = full.slice(0, i);
      setTyped(next);
      // rough active line tracking for visual emphasis
      const parts = home.heroLines;
      let acc = 0;
      for (let idx = 0; idx < parts.length; idx += 1) {
        acc += parts[idx].length + (idx < parts.length - 1 ? 1 : 0);
        if (i <= acc) {
          setLineIndex(idx);
          break;
        }
      }
      if (i >= full.length) clearInterval(id);
    }, 42);
    return () => clearInterval(id);
  }, [home.heroLines]);

  return (
    <>
      <section
        style={{
          minHeight: '100vh',
          padding: '48px 20px 36px',
          display: 'flex',
          alignItems: 'center',
          position: 'relative',
        }}
      >
        <div
          className="hm-hero-grid"
          style={{
            maxWidth: 1200,
            margin: '0 auto',
            width: '100%',
            display: 'grid',
            gridTemplateColumns: '1.15fr 0.85fr',
            gap: 28,
            alignItems: 'center',
          }}
        >
          <div>
            <div style={{ display: 'flex', flexDirection: 'column', gap: 4, marginBottom: 18 }}>
              {home.heroLines.map((line, idx) => {
                const full = home.heroLines.join(' ');
                const start = home.heroLines.slice(0, idx).join(' ').length + (idx ? 1 : 0);
                const end = start + line.length;
                const visible = typed.slice(start, Math.min(end, typed.length));
                const active = lineIndex === idx;
                return (
                  <div
                    key={line}
                    style={{
                      fontFamily: font.serif,
                      fontWeight: 600,
                      fontSize: 'clamp(48px, 8vw, 88px)',
                      lineHeight: 1.02,
                      letterSpacing: '-0.04em',
                      color: active || visible ? colors.white : 'rgba(255,255,255,0.28)',
                      minHeight: '1.05em',
                    }}
                  >
                    {visible || (idx === 0 ? '' : '')}
                    {idx === lineIndex ? <span className="hm-cursor" style={{ color: '#a5b4fc' }}>|</span> : null}
                  </div>
                );
              })}
            </div>

            <p style={{ margin: '0 0 28px', maxWidth: 640, color: 'rgba(226,232,240,0.78)', fontSize: 17, lineHeight: 1.8 }}>
              {home.subtitle}
            </p>

            <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap' }}>
              <button type="button" className="hm-btn" style={primaryButton} onClick={() => navigate('/account')}>
                {home.primaryCta}
              </button>
              <button type="button" className="hm-btn" style={secondaryButton} onClick={() => window.open(workerUi, '_blank', 'noreferrer')}>
                {home.secondaryCta}
              </button>
            </div>
          </div>

          <div style={{ ...glassStrong, borderRadius: 28, padding: 18, minHeight: 360, position: 'relative', overflow: 'hidden' }}>
            <div
              aria-hidden
              style={{
                position: 'absolute',
                inset: 0,
                background:
                  'radial-gradient(circle at 30% 25%, rgba(99,102,241,0.35), transparent 28%), radial-gradient(circle at 75% 70%, rgba(236,72,153,0.18), transparent 30%), linear-gradient(160deg, rgba(15,23,42,0.1), rgba(15,23,42,0.65))',
              }}
            />
            <div style={{ position: 'relative', height: '100%', display: 'grid', alignContent: 'space-between', minHeight: 320 }}>
              <div>
                <SectionEyebrow dark>Hivemind</SectionEyebrow>
                <div style={{ marginTop: 18, fontFamily: font.display, fontSize: 14, letterSpacing: '0.14em', textTransform: 'uppercase', color: 'rgba(226,232,240,0.6)' }}>
                  Create · Compute · Earn
                </div>
              </div>
              <div style={{ display: 'grid', gap: 10 }}>
                {[t('common.openMaster'), t('common.openWorker'), t('common.enter')].map((label, i) => (
                  <div key={label} style={{ ...glassSoft, borderRadius: 14, padding: '12px 14px', display: 'flex', justifyContent: 'space-between', gap: 12 }}>
                    <span style={{ color: 'rgba(226,232,240,0.8)', fontSize: 13 }}>{label}</span>
                    <span style={{ color: '#c7d2fe', fontFamily: font.display, fontSize: 12 }}>0{i + 1}</span>
                  </div>
                ))}
              </div>
            </div>
          </div>
        </div>
      </section>

      <section style={{ padding: '0 20px 28px' }}>
        <div
          style={{
            maxWidth: 1200,
            margin: '0 auto',
            ...glassSoft,
            borderRadius: 20,
            padding: '18px 20px',
            display: 'grid',
            gridTemplateColumns: '1.2fr 1fr auto',
            gap: 16,
            alignItems: 'center',
          }}
          className="hm-hero-grid"
        >
          <div>
            <div style={{ fontSize: 12, letterSpacing: '0.12em', textTransform: 'uppercase', color: '#a5b4fc', fontFamily: font.display }}>
              {home.newsLabel}
            </div>
            <div style={{ marginTop: 8, fontFamily: font.serif, fontSize: 22, fontWeight: 600, letterSpacing: '-0.02em' }}>
              {home.newsTitle}
            </div>
          </div>
          <p style={{ margin: 0, color: 'rgba(226,232,240,0.7)', lineHeight: 1.7, fontSize: 14 }}>{home.newsBody}</p>
          <button type="button" className="hm-btn" style={primaryButton} onClick={() => navigate('/account')}>
            {home.newsCta}
          </button>
        </div>
      </section>

      <section style={{ background: colors.surface, color: colors.ink, padding: '80px 20px' }}>
        <div style={{ maxWidth: 1200, margin: '0 auto' }}>
          <SectionEyebrow>{home.whatLabel}</SectionEyebrow>
          <div className="hm-hero-grid" style={{ display: 'grid', gridTemplateColumns: '1.1fr 0.9fr', gap: 28, marginTop: 18, alignItems: 'start' }}>
            <div>
              <h2
                style={{
                  margin: '0 0 16px',
                  fontFamily: font.serif,
                  fontWeight: 600,
                  fontSize: 'clamp(34px, 4.5vw, 52px)',
                  lineHeight: 1.12,
                  letterSpacing: '-0.03em',
                }}
              >
                {home.whatTitle}
              </h2>
              <p style={{ margin: 0, color: colors.muted, fontSize: 16, lineHeight: 1.85, maxWidth: 640 }}>{home.whatBody}</p>
              <button
                type="button"
                className="hm-btn"
                onClick={() => navigate('/features')}
                style={{ ...primaryButton, marginTop: 24, boxShadow: '0 10px 28px rgba(99,102,241,0.25)' }}
              >
                {home.whatCta}
              </button>
            </div>
            <div className="hm-stats-grid" style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
              {home.stats.map((item) => (
                <div
                  key={item.value}
                  style={{
                    background: colors.white,
                    borderRadius: 18,
                    border: '1px solid rgba(15,23,42,0.08)',
                    padding: 18,
                    boxShadow: '0 16px 36px rgba(15,23,42,0.04)',
                  }}
                >
                  <div style={{ fontFamily: font.display, fontSize: 24, fontWeight: 700, color: colors.indigo }}>{item.value}</div>
                  <div style={{ marginTop: 8, color: colors.muted, fontSize: 13, lineHeight: 1.5 }}>{item.label}</div>
                </div>
              ))}
            </div>
          </div>
        </div>
      </section>

      <section style={{ background: '#eef1f4', color: colors.ink, padding: '72px 20px' }}>
        <div style={{ maxWidth: 1200, margin: '0 auto' }}>
          <div className="hm-dual-grid" style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 18 }}>
            <article style={roleCard}>
              <div style={roleTag}>{home.requestorTitle}</div>
              <h3 style={roleTitle}>{home.requestorTitle}</h3>
              <p style={roleBody}>{home.requestorBody}</p>
              <a href={masterUi} target="_blank" rel="noreferrer" className="hm-btn" style={{ ...primaryButton, alignSelf: 'start' }}>
                {home.requestorCta}
              </a>
            </article>
            <article style={roleCard}>
              <div style={roleTag}>{home.providerTitle}</div>
              <h3 style={roleTitle}>{home.providerTitle}</h3>
              <p style={roleBody}>{home.providerBody}</p>
              <a href={workerUi} target="_blank" rel="noreferrer" className="hm-btn" style={{ ...primaryButton, alignSelf: 'start' }}>
                {home.providerCta}
              </a>
            </article>
          </div>
        </div>
      </section>

      <section style={{ background: colors.dark, color: colors.white, padding: '80px 20px' }}>
        <div style={{ maxWidth: 1200, margin: '0 auto', display: 'grid', gap: 18 }}>
          {[
            {
              title: home.tokenTitle,
              body: home.tokenBody,
              cta: home.tokenCta,
              action: () => navigate('/account'),
            },
            {
              title: home.accessTitle,
              body: home.accessBody,
              cta: home.accessCta,
              action: () => navigate('/vpn'),
            },
            {
              title: home.communityTitle,
              body: home.communityBody,
              cta: home.communityCta,
              action: () => navigate('/features'),
            },
          ].map((block, index) => (
            <div
              key={block.title}
              style={{
                ...glassStrong,
                borderRadius: 24,
                padding: 28,
                display: 'grid',
                gridTemplateColumns: '1.4fr auto',
                gap: 18,
                alignItems: 'center',
              }}
              className="hm-hero-grid"
            >
              <div>
                <div style={{ fontFamily: font.display, fontSize: 12, letterSpacing: '0.14em', textTransform: 'uppercase', color: '#a5b4fc' }}>
                  0{index + 1}
                </div>
                <h3 style={{ margin: '10px 0 10px', fontFamily: font.serif, fontSize: 30, letterSpacing: '-0.02em' }}>{block.title}</h3>
                <p style={{ margin: 0, color: 'rgba(226,232,240,0.72)', lineHeight: 1.8, maxWidth: 720 }}>{block.body}</p>
              </div>
              <button type="button" className="hm-btn" style={primaryButton} onClick={block.action}>
                {block.cta}
              </button>
            </div>
          ))}
        </div>
      </section>
    </>
  );
}

const roleCard = {
  background: colors.white,
  borderRadius: 22,
  border: '1px solid rgba(15,23,42,0.08)',
  padding: 28,
  display: 'grid',
  gap: 14,
  boxShadow: '0 18px 40px rgba(15,23,42,0.05)',
  minHeight: 280,
};

const roleTag = {
  display: 'inline-flex',
  width: 'fit-content',
  padding: '6px 10px',
  borderRadius: 999,
  background: 'rgba(99,102,241,0.1)',
  color: '#4f46e5',
  fontSize: 11,
  fontWeight: 700,
  letterSpacing: '0.12em',
  textTransform: 'uppercase',
  fontFamily: font.display,
};

const roleTitle = {
  margin: 0,
  fontFamily: font.serif,
  fontSize: 30,
  letterSpacing: '-0.02em',
  lineHeight: 1.15,
};

const roleBody = {
  margin: 0,
  color: colors.muted,
  lineHeight: 1.8,
  fontSize: 15,
};
