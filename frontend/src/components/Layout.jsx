import React from 'react';
import NavBar from './NavBar';
import { colors, font } from '../theme';
import { getSection } from '../i18n';

export default function Layout({ children, t, path, navigate, masterUi, workerUi, onToggleLang, sessionUser, lang }) {
  const footer = getSection(lang, 'footer');

  return (
    <div
      style={{
        minHeight: '100vh',
        color: colors.white,
        background: colors.dark,
        fontFamily: font.sans,
      }}
    >
      <div
        aria-hidden
        style={{
          position: 'fixed',
          inset: 0,
          pointerEvents: 'none',
          backgroundImage:
            'linear-gradient(rgba(148,163,184,0.05) 1px, transparent 1px), linear-gradient(90deg, rgba(148,163,184,0.05) 1px, transparent 1px)',
          backgroundSize: '64px 64px',
          maskImage: 'radial-gradient(circle at 50% 10%, black 0%, transparent 70%)',
          WebkitMaskImage: 'radial-gradient(circle at 50% 10%, black 0%, transparent 70%)',
          opacity: 0.7,
        }}
      />
      <div
        aria-hidden
        style={{
          position: 'fixed',
          top: '-18%',
          left: '50%',
          width: 900,
          height: 520,
          transform: 'translateX(-50%)',
          background:
            'radial-gradient(circle, rgba(99,102,241,0.28) 0%, rgba(168,85,247,0.14) 36%, rgba(236,72,153,0.08) 55%, transparent 72%)',
          filter: 'blur(22px)',
          opacity: 0.35,
          pointerEvents: 'none',
        }}
      />

      <NavBar
        t={t}
        path={path}
        navigate={navigate}
        masterUi={masterUi}
        onToggleLang={onToggleLang}
        sessionUser={sessionUser}
      />

      <main style={{ position: 'relative' }}>{children}</main>

      <footer
        style={{
          position: 'relative',
          marginTop: 20,
          borderTop: '1px solid rgba(255,255,255,0.08)',
          background: 'rgba(8, 13, 24, 0.92)',
          padding: '48px 20px 28px',
        }}
      >
        <div style={{ maxWidth: 1200, margin: '0 auto' }}>
          <div
            className="hm-footer-grid"
            style={{
              display: 'grid',
              gridTemplateColumns: '1.2fr repeat(4, minmax(0, 1fr))',
              gap: 24,
            }}
          >
            <div>
              <div style={{ fontFamily: font.display, fontWeight: 700, fontSize: 22, letterSpacing: '-0.03em' }}>{t('brand')}</div>
              <p style={{ margin: '12px 0 0', color: 'rgba(226,232,240,0.62)', lineHeight: 1.7, maxWidth: 280 }}>
                {t('tagline')}
              </p>
            </div>

            <div>
              <div style={{ fontFamily: font.display, fontSize: 13, letterSpacing: '0.08em', textTransform: 'uppercase', color: 'rgba(226,232,240,0.5)', marginBottom: 12 }}>
                {footer.platform}
              </div>
              <div style={{ display: 'grid', gap: 10 }}>
                <button type="button" onClick={() => navigate('/features')} style={linkBtn}>{t('nav.features')}</button>
                <button type="button" onClick={() => navigate('/account')} style={linkBtn}>{t('nav.account')}</button>
                <button type="button" onClick={() => navigate('/vpn')} style={linkBtn}>{t('nav.vpn')}</button>
              </div>
            </div>

            <div>
              <div style={{ fontFamily: font.display, fontSize: 13, letterSpacing: '0.08em', textTransform: 'uppercase', color: 'rgba(226,232,240,0.5)', marginBottom: 12 }}>
                {footer.community}
              </div>
              <div style={{ display: 'grid', gap: 10 }}>
                <a href={masterUi} target="_blank" rel="noreferrer" style={linkA}>{t('common.openMaster')}</a>
                <a href={workerUi} target="_blank" rel="noreferrer" style={linkA}>{t('common.openWorker')}</a>
              </div>
            </div>

            <div>
              <div style={{ fontFamily: font.display, fontSize: 13, letterSpacing: '0.08em', textTransform: 'uppercase', color: 'rgba(226,232,240,0.5)', marginBottom: 12 }}>
                {footer.token}
              </div>
              <div style={{ display: 'grid', gap: 10 }}>
                <button type="button" onClick={() => navigate('/account')} style={linkBtn}>CPT</button>
                <button type="button" onClick={() => navigate('/vpn')} style={linkBtn}>VPN</button>
              </div>
            </div>

            <div>
              <div style={{ fontFamily: font.display, fontSize: 13, letterSpacing: '0.08em', textTransform: 'uppercase', color: 'rgba(226,232,240,0.5)', marginBottom: 12 }}>
                {footer.resources}
              </div>
              <div style={{ display: 'grid', gap: 10 }}>
                <button type="button" onClick={() => navigate('/faq')} style={linkBtn}>{t('nav.faq')}</button>
                <button type="button" onClick={() => navigate('/features')} style={linkBtn}>{footer.about}</button>
              </div>
            </div>
          </div>

          <div
            style={{
              marginTop: 36,
              paddingTop: 18,
              borderTop: '1px solid rgba(255,255,255,0.06)',
              display: 'flex',
              justifyContent: 'space-between',
              gap: 12,
              flexWrap: 'wrap',
              color: 'rgba(226,232,240,0.45)',
              fontSize: 13,
            }}
          >
            <span>{t('brand')}</span>
            <span>{footer.rights}</span>
          </div>
        </div>
      </footer>

      <style>{`
        html, body, #root { margin: 0; min-height: 100%; background: ${colors.dark}; }
        a { color: inherit; text-decoration: none; }
        button, input, textarea { font: inherit; }
        .hm-btn:hover { transform: translateY(-1px); }
        .hm-btn:active { transform: translateY(0); opacity: 0.92; }
        .hm-logo:hover { transform: rotate(180deg); }
        .hm-field:focus {
          border-color: rgba(165, 180, 252, 0.7) !important;
          box-shadow: 0 0 0 3px rgba(99, 102, 241, 0.18);
          background: rgba(15, 23, 42, 0.72) !important;
        }
        .hm-cursor {
          display: inline-block;
          margin-left: 2px;
          animation: hmBlink 1s steps(1) infinite;
        }
        @keyframes hmBlink {
          0%, 49% { opacity: 1; }
          50%, 100% { opacity: 0; }
        }
        @media (max-width: 920px) {
          .hm-hero-grid, .hm-feature-grid, .hm-account-grid, .hm-vpn-grid, .hm-dual-grid, .hm-footer-grid, .hm-stats-grid { grid-template-columns: 1fr !important; }
          .hm-feature-sidebar { position: static !important; }
        }
      `}</style>
    </div>
  );
}

const linkBtn = {
  border: 0,
  background: 'transparent',
  color: 'rgba(226,232,240,0.78)',
  textAlign: 'left',
  padding: 0,
  cursor: 'pointer',
  fontSize: 14,
};

const linkA = {
  color: 'rgba(226,232,240,0.78)',
  fontSize: 14,
};
