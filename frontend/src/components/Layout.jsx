import React from 'react';
import NavBar from './NavBar';
import { colors, font } from '../theme';

export default function Layout({ children, t, path, navigate, masterUi, onToggleLang, sessionUser }) {
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
            'linear-gradient(rgba(148,163,184,0.07) 1px, transparent 1px), linear-gradient(90deg, rgba(148,163,184,0.07) 1px, transparent 1px)',
          backgroundSize: '56px 56px',
          maskImage: 'radial-gradient(circle at 50% 20%, black 0%, transparent 72%)',
          WebkitMaskImage: 'radial-gradient(circle at 50% 20%, black 0%, transparent 72%)',
          opacity: 0.55,
        }}
      />
      <div
        aria-hidden
        style={{
          position: 'fixed',
          top: '-12%',
          left: '50%',
          width: 720,
          height: 420,
          transform: 'translateX(-50%)',
          background:
            'radial-gradient(circle, rgba(99,102,241,0.34) 0%, rgba(168,85,247,0.18) 38%, rgba(236,72,153,0.08) 58%, transparent 72%)',
          filter: 'blur(20px)',
          opacity: 0.3,
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

      <footer style={{ position: 'relative', padding: '28px 18px 40px', color: 'rgba(226,232,240,0.55)', fontSize: 13 }}>
        <div style={{ maxWidth: 1180, margin: '0 auto', display: 'flex', justifyContent: 'space-between', gap: 12, flexWrap: 'wrap' }}>
          <span style={{ fontFamily: font.display }}>{t('brand')}</span>
          <span>{t('tagline')}</span>
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
          .hm-hero-grid, .hm-feature-grid, .hm-account-grid, .hm-vpn-grid { grid-template-columns: 1fr !important; }
          .hm-feature-sidebar { position: static !important; }
        }
      `}</style>
    </div>
  );
}
