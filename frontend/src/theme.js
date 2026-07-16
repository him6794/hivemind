export const colors = {
  dark: '#0f172a',
  slate900: '#0f172a',
  indigo: '#6366f1',
  surface: '#f8f9fa',
  white: '#ffffff',
  ink: '#0f172a',
  muted: '#64748b',
  soft: '#e2e8f0',
  faq: '#f3f4f6',
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
  border: '1px solid rgba(255, 255, 255, 0.1)',
};

export const glassStrong = {
  background: 'rgba(30, 41, 59, 0.7)',
  backdropFilter: 'blur(16px)',
  WebkitBackdropFilter: 'blur(16px)',
  border: '1px solid rgba(255, 255, 255, 0.1)',
  boxShadow: '0 4px 30px rgba(0, 0, 0, 0.1)',
};

export const primaryButton = {
  display: 'inline-flex',
  alignItems: 'center',
  justifyContent: 'center',
  gap: 8,
  padding: '12px 18px',
  borderRadius: 999,
  border: '1px solid rgba(99, 102, 241, 0.45)',
  background: 'linear-gradient(135deg, #6366f1 0%, #7c3aed 55%, #a855f7 100%)',
  color: colors.white,
  fontWeight: 600,
  textDecoration: 'none',
  cursor: 'pointer',
  fontSize: 14,
  fontFamily: font.sans,
  transition: 'transform 500ms ease, box-shadow 500ms ease, opacity 500ms ease',
  boxShadow: '0 0 20px rgba(255,255,255,0.2)',
};

export const secondaryButton = {
  ...primaryButton,
  background: 'rgba(255,255,255,0.08)',
  border: '1px solid rgba(255,255,255,0.12)',
  boxShadow: 'none',
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
  transition: 'border-color 500ms ease, background 500ms ease, box-shadow 500ms ease',
};
