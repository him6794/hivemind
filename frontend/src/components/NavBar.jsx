import React, { useEffect, useState } from 'react';
import LogoMark from './LogoMark';
import { colors, font, primaryButton, secondaryButton, shellWrap } from '../theme';

const routes = [
  { id: 'home', path: '/' },
  { id: 'features', path: '/features' },
  { id: 'account', path: '/account' },
  { id: 'download', path: '/download' },
  { id: 'faq', path: '/faq' },
];

export default function NavBar({ t, path, navigate, onToggleLang, sessionUser }) {
  const [navSolid, setNavSolid] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);

  useEffect(() => {
    const onScroll = () => setNavSolid(window.scrollY > 12);
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
        zIndex: 50,
        transition: 'background 500ms ease, border-color 500ms ease, backdrop-filter 500ms ease',
        background: navSolid || menuOpen ? 'rgba(15, 23, 42, 0.8)' : 'rgba(15, 23, 42, 0.35)',
        borderBottom: '1px solid rgba(255,255,255,0.05)',
        backdropFilter: 'blur(18px)',
        WebkitBackdropFilter: 'blur(18px)',
      }}
    >
      <div
        style={{
          ...shellWrap,
          padding: '16px 24px',
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
            <div style={{ fontFamily: font.display, fontWeight: 700, letterSpacing: '-0.04em', fontSize: 18 }}>
              {t('brand')}
            </div>
            <div style={{ color: 'rgba(226,232,240,0.5)', fontSize: 10, letterSpacing: '0.12em', textTransform: 'uppercase' }}>
              {t('tagline')}
            </div>
          </div>
        </button>

        <nav
          className="hm-desktop-nav"
          style={{
            display: 'flex',
            gap: 2,
            alignItems: 'center',
            flexWrap: 'wrap',
            padding: 4,
            borderRadius: 999,
            background: 'rgba(255,255,255,0.05)',
            border: '1px solid rgba(255,255,255,0.05)',
            backdropFilter: 'blur(12px)',
            WebkitBackdropFilter: 'blur(12px)',
          }}
        >
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
                  padding: '8px 16px',
                  borderRadius: 999,
                  background: active ? 'rgba(255,255,255,0.1)' : 'transparent',
                  color: active ? colors.white : 'rgba(248,250,252,0.6)',
                  fontSize: 12,
                  fontWeight: 600,
                  fontFamily: font.sans,
                  transition: 'background 300ms ease, color 300ms ease',
                }}
              >
                {t(`nav.${route.id}`)}
              </button>
            );
          })}
        </nav>

        <div className="hm-desktop-actions" style={{ display: 'flex', gap: 10, alignItems: 'center' }}>
          {sessionUser ? (
            <span style={{ fontSize: 12, color: 'rgba(226,232,240,0.65)', maxWidth: 120, overflow: 'hidden', textOverflow: 'ellipsis' }}>
              {sessionUser}
            </span>
          ) : null}
          <button
            type="button"
            className="hm-btn hm-btn-secondary"
            onClick={onToggleLang}
            style={{ ...secondaryButton, padding: '8px 14px', fontSize: 12 }}
          >
            {t('nav.lang')}
          </button>
          <button type="button" className="hm-btn" style={{ ...primaryButton, padding: '8px 16px', fontSize: 12 }} onClick={() => navigate('/download')}>
            {t('nav.start')}
          </button>
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
            border: '1px solid rgba(255,255,255,0.1)',
            background: 'rgba(255,255,255,0.05)',
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
            padding: '8px 24px 18px',
            display: 'grid',
            gap: 8,
            borderTop: '1px solid rgba(255,255,255,0.05)',
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
                background: path === route.path ? 'rgba(6,182,212,0.16)' : 'transparent',
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
            <button type="button" className="hm-btn hm-btn-secondary" onClick={onToggleLang} style={{ ...secondaryButton, flex: 1 }}>
              {t('nav.lang')}
            </button>
            <button type="button" className="hm-btn" style={{ ...primaryButton, flex: 1 }} onClick={() => navigate('/download')}>
              {t('nav.start')}
            </button>
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
