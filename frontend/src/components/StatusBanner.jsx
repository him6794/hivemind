import React from 'react';
import { font } from '../theme';

export default function StatusBanner({ tone = 'neutral', children }) {
  const styles = {
    ok: { bg: 'rgba(16, 185, 129, 0.14)', border: 'rgba(52, 211, 153, 0.35)', color: '#d1fae5' },
    err: { bg: 'rgba(239, 68, 68, 0.14)', border: 'rgba(248, 113, 113, 0.35)', color: '#fee2e2' },
    neutral: { bg: 'rgba(255,255,255,0.05)', border: 'rgba(255,255,255,0.1)', color: 'rgba(248,250,252,0.88)' },
  }[tone];

  return (
    <div
      style={{
        marginTop: 14,
        padding: '12px 14px',
        borderRadius: 14,
        background: styles.bg,
        border: `1px solid ${styles.border}`,
        color: styles.color,
        lineHeight: 1.65,
        fontSize: 13,
        fontFamily: font.sans,
        whiteSpace: 'pre-wrap',
      }}
    >
      {children}
    </div>
  );
}
