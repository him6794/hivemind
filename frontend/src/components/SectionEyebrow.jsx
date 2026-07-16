import React from 'react';
import { font } from '../theme';

export default function SectionEyebrow({ children, dark = false }) {
  return (
    <div
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        gap: 8,
        padding: '5px 11px',
        borderRadius: 999,
        background: dark ? 'rgba(6,182,212,0.12)' : 'rgba(6,182,212,0.1)',
        border: `1px solid ${dark ? 'rgba(103,232,249,0.24)' : 'rgba(6,182,212,0.16)'}`,
        color: dark ? '#a5f3fc' : '#0e7490',
        letterSpacing: '0.16em',
        textTransform: 'uppercase',
        fontSize: 10,
        fontWeight: 700,
        fontFamily: font.display,
      }}
    >
      {children}
    </div>
  );
}
