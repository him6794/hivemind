import React, { useState } from 'react';
import { colors, font } from '../theme';

export default function FaqItem({ question, answer }) {
  const [open, setOpen] = useState(false);

  return (
    <div
      style={{
        background: colors.faq,
        borderRadius: 16,
        overflow: 'hidden',
        border: '1px solid rgba(15,23,42,0.05)',
      }}
    >
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        style={{
          width: '100%',
          textAlign: 'left',
          border: 0,
          background: 'transparent',
          padding: '18px 18px',
          display: 'flex',
          justifyContent: 'space-between',
          gap: 16,
          alignItems: 'center',
          cursor: 'pointer',
          color: colors.ink,
        }}
      >
        <span style={{ fontFamily: font.serif, fontSize: 18, fontWeight: 600, letterSpacing: '-0.02em' }}>{question}</span>
        <span
          style={{
            display: 'inline-block',
            transition: 'transform 500ms ease',
            transform: open ? 'rotate(180deg)' : 'rotate(0deg)',
            color: colors.muted,
            fontSize: 18,
          }}
        >
          ⌄
        </span>
      </button>
      <div
        style={{
          display: 'grid',
          gridTemplateRows: open ? '1fr' : '0fr',
          transition: 'grid-template-rows 500ms ease',
        }}
      >
        <div style={{ overflow: 'hidden' }}>
          <div style={{ padding: '0 18px 18px', color: colors.muted, lineHeight: 1.75, fontSize: 14 }}>{answer}</div>
        </div>
      </div>
    </div>
  );
}
