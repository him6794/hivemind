import React, { useEffect, useState } from 'react';
import LogoMark from './LogoMark';
import { colors, font, primaryButton, secondaryButton } from '../theme';

const routes = [
  { id: 'home', path: '/' },
  { id: 'features', path: '/features' },
  { id: 'account', path: '/account' },
  { id: 'vpn', path: '/vpn' },
  { id: 'faq', path: '/faq' },
];

export default function NavBar({ t, path, navigate, masterUi, onToggleLang, sessionUser }) {
  const [navSolid, setNavSolid] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);

  useEffect(() => {
    const onScroll = () => setNavSolid(window.scrollY > 18);
    onScroll();
    window.addEventListener('scroll', onScroll, { passive: true });
    return () => window.removeEventListener('scroll', onScroll);
  }, []);

  useEffect(() => {
    setMenuOpen(false);
  }, [path]);

  return (
    <header
      style={{
        position: 'sticky',
        top: 0,
        zIndex: 40,
        transition: 'background 500ms ease, border-color 500ms ease, box-shadow 500ms ease, backdrop-filter 500ms ease',
        background: navSolid || menuOpen ? 'rgba(15, 23, 42, 0.92)' : 'transparent',
        borderBottom: navSolid || menuOpen ? '1px solid rgba(255,255,255,0.08)' : '1px solid transparent',
        backdropFilter: navSolid || menuOpen ? 'blur(16px)' : 'none',
        WebkitBackdropFilter: navSolid || menuOpen ? 'blur(16px)' : 'none',
        boxShadow: navSolid ? '0 8px 30px rgba(0,0,0,0.18)' : 'none',
      }}
    >
      <div
        style={{
          maxWidth: 1200,
          margin: '0 auto',
          padding: '16px 20px',
          display: 'flex',
          justifyContent: 'space-between',
          gap: 16,
          alignItems: 'center',
        }}
      >
        <button
          type="button"
          onClick={() => navigate('/')}
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 12,
            background: 'transparent',
            border: 0,
            color: colors.white,
            cursor: 'pointer',
            padding: 0,
          }}
        >
          <LogoMark />
          <div style={{ textAlign: 'left' }}>
            <div style={{ fontFamily: font.display, fontWeight: 700, letterSpacing: '-0.04em', fontSize: 20 }}>
              {t('brand')}
            </div>
            <div style={{ color: 'rgba(226,232,240,0.58)', fontSize: 11, letterSpacing: '0.04em' }}>{t('tagline')}</div>
          </div>
        </button>

        <nav className="hm-desktop-nav" style={{ display: 'flex', gap: 4, alignItems: 'center', flexWrap: 'wrap' }}>
          {routes.map((route) => {
            const active = path === route.path;
            return (
              <button
                key={route.id}
                type="button"
                onClick={() => navigate(route.path)}
                style={{
                  border: 0,
                  cursor: 'pointer',
                  padding: '10px 12px',
                  borderRadius: 10,
                  background: active ? 'rgba(255,255,255,0.08)' : 'transparent',
                  color: active ? colors.white : 'rgba(248,250,252,0.78)',
                  fontSize: 14,
                  fontWeight: 600,
                  fontFamily: font.sans,
                  transition: 'background 500ms ease, color 500ms ease',
                }}
              >
                {t(`nav.${route.id}`)}
              </button>
            );
          })}
        </nav>

        <div className="hm-desktop-actions" style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
          {sessionUser ? (
            <span style={{ fontSize: 12, color: 'rgba(226,232,240,0.7)', maxWidth: 120, overflow: 'hidden', textOverflow: 'ellipsis' }}>
              {sessionUser}
            </span>
          ) : null}
          <button type="button" className="hm-btn" onClick={onToggleLang} style={{ ...secondaryButton, padding: '10px 14px' }}>
            {t('nav.lang')}
          </button>
          <a href={masterUi} className="hm-btn" style={primaryButton} target="_blank" rel="noreferrer">
            {t('nav.master')}
          </a>
        </div>

        <button
          type="button"
          className="hm-mobile-toggle"
          aria-label="Menu"
          onClick={() => setMenuOpen((v) => !v)}
          style={{
            display: 'none',
            width: 42,
            height: 42,
            borderRadius: 12,
            border: '1px solid rgba(255,255,255,0.12)',
            background: 'rgba(255,255,255,0.06)',
            color: colors.white,
            cursor: 'pointer',
          }}
        >
          {menuOpen ? '✕' : '☰'}
        </button>
      </div>

      {menuOpen ? (
        <div
          className="hm-mobile-menu"
          style={{
            padding: '8px 20px 18px',
            display: 'grid',
            gap: 8,
            borderTop: '1px solid rgba(255,255,255,0.06)',
          }}
        >
          {routes.map((route) => (
            <button
              key={route.id}
              type="button"
              onClick={() => navigate(route.path)}
              style={{
                textAlign: 'left',
                border: 0,
                background: path === route.path ? 'rgba(99,102,241,0.18)' : 'transparent',
                color: colors.white,
                padding: '12px 12px',
                borderRadius: 12,
                fontWeight: 600,
                cursor: 'pointer',
              }}
            >
              {t(`nav.${route.id}`)}
            </button>
          ))}
          <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap', paddingTop: 4 }}>
            <button type="button" className="hm-btn" onClick={onToggleLang} style={{ ...secondaryButton, flex: 1 }}>
              {t('nav.lang')}
            </button>
            <a href={masterUi} className="hm-btn" style={{ ...primaryButton, flex: 1 }} target="_blank" rel="noreferrer">
              {t('nav.master')}
            </a>
          </div>
        </div>
      ) : null}

      <style>{`
        @media (max-width: 920px) {
          .hm-desktop-nav, .hm-desktop-actions { display: none !important; }
          .hm-mobile-toggle { display: inline-grid !important; place-items: center; }
        }
      `}</style>
    </header>
  );
}
