import React from 'react';
import { font } from '../theme';

export default function SectionEyebrow({ children, dark = false }) {
  return (
    <div
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        gap: 8,
        padding: '6px 12px',
        borderRadius: 999,
        background: dark ? 'rgba(99,102,241,0.12)' : 'rgba(99,102,241,0.1)',
        border: `1px solid ${dark ? 'rgba(165,180,252,0.28)' : 'rgba(99,102,241,0.18)'}`,
        color: dark ? '#c7d2fe' : '#4f46e5',
        letterSpacing: '0.14em',
        textTransform: 'uppercase',
        fontSize: 11,
        fontWeight: 600,
        fontFamily: font.display,
      }}
    >
      {children}
    </div>
  );
}
