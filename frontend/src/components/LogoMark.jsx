import React from 'react';

export default function LogoMark({ size = 40 }) {
  return (
    <img
      className="hm-logo"
      src="/logo.png"
      alt=""
      width={size}
      height={size}
      draggable={false}
      style={{
        width: size,
        height: size,
        display: 'block',
        objectFit: 'contain',
        background: 'transparent',
        border: 0,
        borderRadius: 0,
        boxShadow: 'none',
        transition: 'transform 500ms ease',
      }}
      aria-hidden
    />
  );
}
