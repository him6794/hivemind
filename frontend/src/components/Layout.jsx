import React from 'react';
import NavBar from './NavBar';
import { colors, font, shellWrap } from '../theme';
import { getSection } from '../i18n';

export default function Layout({ children, t, path, navigate, onToggleLang, sessionUser, lang }) {
  const footer = getSection(lang, 'footer');

  return (
    <div style={{ minHeight: '100vh', color: colors.white, background: colors.dark, fontFamily: font.sans }}>
      <div
        aria-hidden
        style={{
          position: 'fixed',
          inset: 0,
          pointerEvents: 'none',
          backgroundImage:
            'linear-gradient(rgba(148,163,184,0.03) 1px, transparent 1px), linear-gradient(90deg, rgba(148,163,184,0.03) 1px, transparent 1px)',
          backgroundSize: '48px 48px',
          maskImage: 'radial-gradient(circle at 50% 0%, black 0%, transparent 85%)',
          WebkitMaskImage: 'radial-gradient(circle at 50% 0%, black 0%, transparent 85%)',
          opacity: 1,
        }}
      />
      <div
        aria-hidden
        style={{
          position: 'fixed',
          top: '-18%',
          left: '50%',
          width: 920,
          height: 520,
          transform: 'translateX(-50%)',
          background:
            'radial-gradient(circle, rgba(6,182,212,0.22) 0%, rgba(34,211,238,0.1) 40%, rgba(45,212,191,0.05) 58%, transparent 72%)',
          filter: 'blur(28px)',
          opacity: 0.42,
          pointerEvents: 'none',
        }}
      />

      <NavBar
        t={t}
        path={path}
        navigate={navigate}
        onToggleLang={onToggleLang}
        sessionUser={sessionUser}
      />

      <main style={{ position: 'relative' }}>{children}</main>

      <footer
        style={{
          position: 'relative',
          marginTop: 0,
          borderTop: '1px solid rgba(255,255,255,0.05)',
          background: colors.footer,
          padding: '84px 24px 36px',
        }}
      >
        <div style={shellWrap}>
          <div className="hm-footer-grid" style={{ display: 'grid', gridTemplateColumns: '1.5fr repeat(3, minmax(0, 1fr))', gap: 36 }}>
            <div>
              <div style={{ fontFamily: font.display, fontWeight: 700, fontSize: 22, letterSpacing: '-0.03em' }}>{t('brand')}</div>
              <p style={{ margin: '14px 0 0', color: 'rgba(226,232,240,0.58)', lineHeight: 1.75, maxWidth: 300, fontSize: 14 }}>
                {t('tagline')}
              </p>
            </div>
            <FooterCol title={footer.platform}>
              <FooterBtn onClick={() => navigate('/features')}>{t('nav.features')}</FooterBtn>
              <FooterBtn onClick={() => navigate('/account')}>{t('nav.account')}</FooterBtn>
              <FooterBtn onClick={() => navigate('/download')}>{t('nav.download')}</FooterBtn>
            </FooterCol>
            <FooterCol title={footer.surfaces}>
              <FooterBtn onClick={() => navigate('/download')}>{t('common.getStarted')}</FooterBtn>
              <FooterBtn onClick={() => navigate('/download')}>{t('common.openDownload')}</FooterBtn>
            </FooterCol>
            <FooterCol title={footer.resources}>
              <FooterBtn onClick={() => navigate('/faq')}>{t('nav.faq')}</FooterBtn>
              <FooterBtn onClick={() => navigate('/account')}>{t('nav.account')}</FooterBtn>
            </FooterCol>
          </div>
          <div
            style={{
              marginTop: 56,
              paddingTop: 22,
              borderTop: '1px solid rgba(255,255,255,0.05)',
              display: 'flex',
              justifyContent: 'space-between',
              gap: 12,
              flexWrap: 'wrap',
              color: 'rgba(226,232,240,0.4)',
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
        .hm-btn:hover { transform: translateY(-2px); box-shadow: 0 12px 24px rgba(6,182,212,0.28); }
        .hm-btn:active { transform: translateY(0); opacity: 0.94; }
        .hm-btn-secondary:hover { background: rgba(255,255,255,0.08) !important; border-color: rgba(255,255,255,0.2) !important; box-shadow: none !important; }
        .hm-logo { transition: transform 500ms ease; }
        .hm-logo:hover { transform: scale(1.06); }
        .hm-field:focus {
          border-color: rgba(103, 232, 249, 0.55) !important;
          box-shadow: 0 0 0 3px rgba(6, 182, 212, 0.16);
          background: rgba(15, 23, 42, 0.72) !important;
        }
        .hm-field-light:focus {
          border-color: rgba(6, 182, 212, 0.55) !important;
          box-shadow: 0 0 0 3px rgba(6, 182, 212, 0.12);
          background: #ffffff !important;
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
          .hm-hero-grid, .hm-feature-grid, .hm-account-grid, .hm-dual-grid, .hm-footer-grid { grid-template-columns: 1fr !important; }
          .hm-feature-sidebar { position: static !important; }
        }
      `}</style>
    </div>
  );
}

function FooterCol({ title, children }) {
  return (
    <div>
      <div style={{ fontFamily: font.display, fontSize: 12, letterSpacing: '0.1em', textTransform: 'uppercase', color: 'rgba(226,232,240,0.45)', marginBottom: 14, fontWeight: 600 }}>
        {title}
      </div>
      <div style={{ display: 'grid', gap: 12 }}>{children}</div>
    </div>
  );
}

function FooterBtn({ onClick, children }) {
  return (
    <button
      type="button"
      onClick={onClick}
      style={{
        border: 0,
        background: 'transparent',
        color: 'rgba(226,232,240,0.72)',
        textAlign: 'left',
        padding: 0,
        cursor: 'pointer',
        fontSize: 14,
        transition: 'color 300ms ease',
      }}
      onMouseEnter={(e) => { e.currentTarget.style.color = '#67e8f9'; }}
      onMouseLeave={(e) => { e.currentTarget.style.color = 'rgba(226,232,240,0.72)'; }}
    >
      {children}
    </button>
  );
}
