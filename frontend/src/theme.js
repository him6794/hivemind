export const colors = {
  dark: '#0f172a',
  slate900: '#0f172a',
  accent: '#06b6d4',
  indigo: '#06b6d4',
  surface: '#f8f9fa',
  white: '#ffffff',
  ink: '#0f172a',
  muted: '#64748b',
  soft: '#e2e8f0',
  faq: '#f3f4f6',
  panel: '#eef1f4',
  footer: '#080d18',
};

export const font = {
  serif: '"Lora", Georgia, serif',
  sans: '"Inter", system-ui, sans-serif',
  display: '"Space Grotesk", system-ui, sans-serif',
};

export const glassSoft = {
  background: 'rgba(255, 255, 255, 0.05)',
  backdropFilter: 'blur(12px)',
  WebkitBackdropFilter: 'blur(12px)',
  border: '1px solid rgba(255, 255, 255, 0.08)',
};

export const glassStrong = {
  background: 'rgba(15, 23, 42, 0.8)',
  backdropFilter: 'blur(24px)',
  WebkitBackdropFilter: 'blur(24px)',
  border: '1px solid rgba(255, 255, 255, 0.08)',
  boxShadow: '0 8px 32px rgba(0, 0, 0, 0.2)',
};

export const primaryButton = {
  display: 'inline-flex',
  alignItems: 'center',
  justifyContent: 'center',
  gap: 8,
  padding: '14px 22px',
  borderRadius: 999,
  border: '1px solid rgba(6, 182, 212, 0.45)',
  background: 'linear-gradient(135deg, #06b6d4 0%, #0891b2 55%, #22d3ee 100%)',
  color: colors.white,
  fontWeight: 700,
  textDecoration: 'none',
  cursor: 'pointer',
  fontSize: 13,
  fontFamily: font.sans,
  letterSpacing: '0.01em',
  transition: 'transform 300ms cubic-bezier(0.4,0,0.2,1), box-shadow 300ms ease, opacity 300ms ease',
  boxShadow: 'none',
};

export const secondaryButton = {
  ...primaryButton,
  background: 'rgba(255,255,255,0.05)',
  border: '1px solid rgba(255,255,255,0.12)',
  boxShadow: 'none',
  fontWeight: 700,
};

export const fieldStyle = {
  width: '100%',
  boxSizing: 'border-box',
  padding: '12px 14px',
  borderRadius: 12,
  border: '1px solid rgba(255,255,255,0.12)',
  background: 'rgba(15, 23, 42, 0.45)',
  color: colors.white,
  outline: 'none',
  fontSize: 14,
  fontFamily: font.sans,
  transition: 'border-color 300ms ease, background 300ms ease, box-shadow 300ms ease',
};

export const pageWrap = {
  maxWidth: 1200,
  margin: '0 auto',
  width: '100%',
};

export const shellWrap = {
  maxWidth: 1400,
  margin: '0 auto',
  width: '100%',
};
