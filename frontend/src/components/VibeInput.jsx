import React from 'react';
import { colors, font } from '../theme';

export default function VibeInput({ value, onChange, placeholder, hint }) {
  return (
    <div style={{ position: 'relative', marginTop: 28 }}>
      <div
        aria-hidden
        className="hm-vibe-glow"
        style={{
          position: 'absolute',
          inset: -4,
          borderRadius: 22,
          background: 'linear-gradient(90deg, #06b6d4, #22d3ee, #2dd4bf)',
          filter: 'blur(16px)',
          opacity: 0.35,
          transition: 'opacity 500ms ease',
        }}
      />
      <div
        style={{
          position: 'relative',
          background: colors.white,
          borderRadius: 16,
          padding: 16,
          boxShadow: '0 18px 40px rgba(15,23,42,0.18)',
        }}
      >
        <textarea
          value={value}
          onChange={onChange}
          placeholder={placeholder}
          rows={3}
          style={{
            width: '100%',
            border: 0,
            resize: 'none',
            outline: 'none',
            fontSize: 18,
            lineHeight: 1.5,
            color: colors.ink,
            fontFamily: font.sans,
            background: 'transparent',
            boxSizing: 'border-box',
          }}
        />
        <div style={{ display: 'flex', justifyContent: 'space-between', gap: 12, alignItems: 'center', marginTop: 8 }}>
          <div style={{ fontSize: 12, color: colors.muted, lineHeight: 1.5 }}>{hint}</div>
          <div
            style={{
              width: 36,
              height: 36,
              borderRadius: 999,
              display: 'grid',
              placeItems: 'center',
              background: 'linear-gradient(135deg, #06b6d4, #0891b2)',
              color: colors.white,
              fontSize: 16,
            }}
            aria-hidden
          >
            ↑
          </div>
        </div>
      </div>
    </div>
  );
}
