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

  useEffect(() => {
    const onScroll = () => setNavSolid(window.scrollY > 18);
    onScroll();
    window.addEventListener('scroll', onScroll, { passive: true });
    return () => window.removeEventListener('scroll', onScroll);
  }, []);

  return (
    <header
      style={{
        position: 'sticky',
        top: 0,
        zIndex: 40,
        transition: 'background 500ms ease, border-color 500ms ease, box-shadow 500ms ease, backdrop-filter 500ms ease',
        background: navSolid ? 'rgba(15, 23, 42, 0.82)' : 'transparent',
        borderBottom: navSolid ? '1px solid rgba(255,255,255,0.08)' : '1px solid transparent',
        backdropFilter: navSolid ? 'blur(16px)' : 'none',
        WebkitBackdropFilter: navSolid ? 'blur(16px)' : 'none',
        boxShadow: navSolid ? '0 8px 30px rgba(0,0,0,0.18)' : 'none',
      }}
    >
      <div
        style={{
          maxWidth: 1180,
          margin: '0 auto',
          padding: '14px 18px',
          display: 'flex',
          justifyContent: 'space-between',
          gap: 12,
          alignItems: 'center',
          flexWrap: 'wrap',
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
            <div style={{ fontFamily: font.display, fontWeight: 700, letterSpacing: '-0.03em', fontSize: 18 }}>
              {t('brand')}
            </div>
            <div style={{ color: 'rgba(226,232,240,0.62)', fontSize: 12 }}>{t('tagline')}</div>
          </div>
        </button>

        <nav
          style={{
            display: 'flex',
            gap: 4,
            flexWrap: 'wrap',
            alignItems: 'center',
            padding: '6px 8px',
            borderRadius: 999,
            background: 'rgba(255,255,255,0.1)',
            backdropFilter: 'blur(12px)',
            WebkitBackdropFilter: 'blur(12px)',
            border: '1px solid rgba(255,255,255,0.1)',
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
                  padding: '8px 12px',
                  borderRadius: 999,
                  background: active ? 'rgba(99,102,241,0.28)' : 'transparent',
                  color: active ? '#e0e7ff' : 'rgba(248,250,252,0.86)',
                  fontSize: 13,
                  fontWeight: active ? 600 : 500,
                  fontFamily: font.sans,
                  transition: 'background 500ms ease, color 500ms ease',
                }}
              >
                {t(`nav.${route.id}`)}
              </button>
            );
          })}
        </nav>

        <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap', alignItems: 'center' }}>
          {sessionUser ? (
            <span style={{ fontSize: 12, color: 'rgba(226,232,240,0.7)' }}>
              {t('common.signedIn')}: {sessionUser}
            </span>
          ) : null}
          <button type="button" className="hm-btn" onClick={onToggleLang} style={{ ...secondaryButton, padding: '10px 14px' }}>
            {t('nav.lang')}
          </button>
          <a href={masterUi} className="hm-btn" style={primaryButton} target="_blank" rel="noreferrer">
            {t('nav.master')}
          </a>
        </div>
      </div>
    </header>
  );
}
