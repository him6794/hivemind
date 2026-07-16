import React, { useState } from 'react';
import { secondaryButton } from '../theme';

export default function CopyButton({ text, label = 'Copy', copiedLabel = 'Copied' }) {
  const [copied, setCopied] = useState(false);

  return (
    <button
      type="button"
      className="hm-btn"
      onClick={async () => {
        try {
          await navigator.clipboard.writeText(text || '');
          setCopied(true);
          setTimeout(() => setCopied(false), 1400);
        } catch {
          setCopied(false);
        }
      }}
      style={{
        ...secondaryButton,
        padding: '8px 12px',
        fontSize: 12,
        background: 'transparent',
      }}
    >
      {copied ? copiedLabel : label}
    </button>
  );
}
