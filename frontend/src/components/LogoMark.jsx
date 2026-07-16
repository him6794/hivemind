import React from 'react';

export default function LogoMark({ size = 40 }) {
  return (
    <div
      className="hm-logo"
      style={{
        width: size,
        height: size,
        borderRadius: 12,
        display: 'grid',
        placeItems: 'center',
        background: 'linear-gradient(145deg, rgba(99,102,241,0.95), rgba(168,85,247,0.85))',
        boxShadow: '0 0 0 1px rgba(255,255,255,0.12), 0 12px 28px rgba(99,102,241,0.35)',
        transition: 'transform 500ms ease',
      }}
      aria-hidden
    >
      <div
        style={{
          width: size * 0.42,
          height: size * 0.42,
          borderRadius: '50%',
          border: '2px solid rgba(255,255,255,0.9)',
          boxShadow: 'inset 0 0 0 4px rgba(15,23,42,0.18)',
        }}
      />
    </div>
  );
}
