import React, { useState } from 'react';
import { colors, font, primaryButton, secondaryButton } from '../theme';
import { getSection } from '../i18n';

const lightField = {
  width: '100%',
  boxSizing: 'border-box',
  height: 48,
  padding: '12px 16px',
  borderRadius: 12,
  border: '1px solid #E5E7EB',
  background: colors.white,
  color: colors.ink,
  outline: 'none',
  fontSize: 14,
  fontFamily: font.sans,
  transition: 'border-color 300ms ease, box-shadow 300ms ease',
};

const lightSecondary = {
  ...secondaryButton,
  background: colors.white,
  color: colors.ink,
  border: '1px solid #E5E7EB',
  boxShadow: 'none',
};

const charcoalButton = {
  ...primaryButton,
  background: colors.ink,
  border: '1px solid rgba(15,23,42,0.92)',
  boxShadow: '0 10px 24px rgba(15,23,42,0.14)',
};

export default function AccountPage({ lang, t, navigate, session }) {
  const account = getSection(lang, 'account');
  const {
    token,
    sessionUser,
    username,
    setUsername,
    password,
    setPassword,
    confirmPassword,
    setConfirmPassword,
    balance,
    bootstrapping,
    loading,
    busyAction,
    status,
    statusTone,
    transferTo,
    setTransferTo,
    transferAmount,
    setTransferAmount,
    transferNote,
    handleAuthSubmit,
    handleTransfer,
    logout,
    authMode,
    setAuthMode,
  } = session;

  const loggedIn = Boolean(token);
  const [showPassword, setShowPassword] = useState(false);
  const [showConfirm, setShowConfirm] = useState(false);

  if (loggedIn) {
    return (
      <section style={{ background: colors.surface, color: colors.ink, padding: '96px 24px 120px', minHeight: '70vh' }}>
        <div style={{ maxWidth: 760, margin: '0 auto', width: '100%' }}>
          <div style={{ marginBottom: 28, textAlign: 'center' }}>
            <div
              style={{
                display: 'inline-flex',
                alignItems: 'center',
                gap: 8,
                padding: '6px 12px',
                borderRadius: 999,
                background: 'rgba(6,182,212,0.1)',
                color: '#0e7490',
                fontSize: 12,
                fontWeight: 700,
                letterSpacing: '0.08em',
                textTransform: 'uppercase',
                fontFamily: font.display,
              }}
            >
              {account.kicker || account.title}
            </div>
            <h1
              style={{
                margin: '18px 0 12px',
                fontFamily: font.serif,
                fontWeight: 500,
                fontSize: 'clamp(40px, 5vw, 60px)',
                lineHeight: 1,
                letterSpacing: '-0.04em',
              }}
            >
              {account.title}
            </h1>
            <p style={{ margin: '0 auto', maxWidth: 520, color: colors.muted, lineHeight: 1.8, fontSize: 15 }}>
              {account.body}
            </p>
          </div>

          <div
            style={{
              background: colors.white,
              borderRadius: 28,
              border: '1px solid rgba(15,23,42,0.08)',
              boxShadow: '0 18px 40px rgba(15,23,42,0.05)',
              padding: 28,
            }}
          >
            <div style={{ display: 'grid', gap: 18 }}>
              <div style={{ display: 'flex', justifyContent: 'space-between', gap: 10, alignItems: 'center', flexWrap: 'wrap' }}>
                <div>
                  <div
                    style={{
                      fontSize: 11,
                      color: colors.muted,
                      fontFamily: font.display,
                      letterSpacing: '0.14em',
                      textTransform: 'uppercase',
                      fontWeight: 700,
                    }}
                  >
                    {t('common.signedIn')}
                  </div>
                  <div
                    style={{
                      fontFamily: font.serif,
                      fontSize: 32,
                      fontWeight: 500,
                      letterSpacing: '-0.03em',
                      marginTop: 6,
                      color: colors.ink,
                    }}
                  >
                    {sessionUser || username || 'user'}
                  </div>
                </div>
                <button type="button" className="hm-btn" onClick={() => logout()} style={lightSecondary}>
                  {t('common.signOut')}
                </button>
              </div>

              <div
                style={{
                  borderRadius: 20,
                  padding: 18,
                  background: colors.faq,
                  border: '1px solid rgba(15,23,42,0.06)',
                }}
              >
                <div style={{ fontSize: 12, color: colors.muted, letterSpacing: '0.04em' }}>{account.balance}</div>
                <div
                  style={{
                    marginTop: 8,
                    fontFamily: font.display,
                    fontSize: 34,
                    fontWeight: 700,
                    color: '#0e7490',
                    letterSpacing: '-0.03em',
                  }}
                >
                  {bootstrapping ? t('common.loading') : balance ?? '—'}
                </div>
              </div>

              <form onSubmit={handleTransfer} style={{ display: 'grid', gap: 12 }}>
                <div
                  style={{
                    fontFamily: font.display,
                    fontSize: 12,
                    fontWeight: 700,
                    letterSpacing: '0.12em',
                    textTransform: 'uppercase',
                    color: colors.ink,
                  }}
                >
                  {account.transfer}
                </div>
                <input
                  className="hm-field-light"
                  value={transferTo}
                  onChange={(e) => setTransferTo(e.target.value)}
                  style={lightField}
                  placeholder={account.recipient}
                  autoComplete="off"
                />
                <input
                  className="hm-field-light"
                  value={transferAmount}
                  onChange={(e) => setTransferAmount(e.target.value)}
                  style={lightField}
                  placeholder={account.amount}
                  inputMode="numeric"
                />
                <button type="submit" disabled={loading} className="hm-btn" style={charcoalButton}>
                  {busyAction === 'transfer' ? account.sending : account.send}
                </button>
                {transferNote ? <div style={{ fontSize: 12, color: colors.muted }}>{transferNote}</div> : null}
              </form>

              <button type="button" className="hm-btn" style={lightSecondary} onClick={() => navigate('/download')}>
                {account.nextDownload}
              </button>
            </div>

            <LightStatusBanner tone={statusTone}>{status}</LightStatusBanner>
          </div>
        </div>
      </section>
    );
  }

  const isRegister = authMode === 'register';

  return (
    <section className="hm-auth-shell" style={{ minHeight: 'calc(100vh - 72px)', background: colors.surface, color: colors.ink }}>
      <div className="hm-auth-grid" style={{ minHeight: 'calc(100vh - 72px)', display: 'grid', gridTemplateColumns: '1fr 1fr' }}>
        <aside
          className="hm-auth-brand"
          style={{
            position: 'relative',
            background: '#F7F7F7',
            borderRight: '1px solid rgba(15,23,42,0.06)',
            overflow: 'hidden',
            minHeight: 420,
          }}
        >
          <div
            aria-hidden
            style={{
              position: 'absolute',
              inset: 0,
              background:
                'radial-gradient(circle at 30% 28%, rgba(6,182,212,0.16), transparent 34%), radial-gradient(circle at 72% 68%, rgba(34,211,238,0.12), transparent 28%)',
            }}
          />

          <div
            style={{
              position: 'relative',
              zIndex: 1,
              height: '100%',
              minHeight: 420,
              display: 'flex',
              flexDirection: 'column',
              alignItems: 'center',
              justifyContent: 'center',
              padding: '96px 40px 56px',
              textAlign: 'center',
            }}
          >
            <FloatingCharacters />
            <div style={{ marginTop: 48, maxWidth: 360 }}>
              <div
                style={{
                  display: 'inline-flex',
                  alignItems: 'center',
                  gap: 8,
                  padding: '6px 12px',
                  borderRadius: 999,
                  background: 'rgba(6,182,212,0.12)',
                  color: '#0e7490',
                  fontSize: 12,
                  fontWeight: 700,
                  letterSpacing: '0.08em',
                  textTransform: 'uppercase',
                  fontFamily: font.display,
                }}
              >
                {account.kicker || account.title}
              </div>
              <h2
                style={{
                  margin: '18px 0 12px',
                  fontFamily: font.serif,
                  fontWeight: 500,
                  fontSize: 'clamp(32px, 3.4vw, 44px)',
                  lineHeight: 1.05,
                  letterSpacing: '-0.03em',
                }}
              >
                {account.title}
              </h2>
              <p style={{ margin: 0, color: colors.muted, lineHeight: 1.8, fontSize: 15 }}>{account.body}</p>
            </div>
          </div>
        </aside>

        <div
          className="hm-auth-form-pane"
          style={{
            background: colors.white,
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            padding: '64px 48px',
          }}
        >
          <div style={{ width: '100%', maxWidth: 448 }}>
            <div style={{ marginBottom: 28 }}>
              <h1
                style={{
                  margin: 0,
                  fontFamily: font.serif,
                  fontWeight: 600,
                  fontSize: 'clamp(32px, 4vw, 40px)',
                  letterSpacing: '-0.03em',
                  lineHeight: 1.1,
                  color: colors.ink,
                }}
              >
                {isRegister ? account.welcomeNew : account.welcomeBack}
              </h1>
              <p style={{ margin: '12px 0 0', color: colors.muted, lineHeight: 1.7, fontSize: 15 }}>
                {isRegister ? account.registerSubtitle : account.loginSubtitle}
              </p>
            </div>

            <div
              style={{
                display: 'flex',
                gap: 8,
                marginBottom: 24,
                padding: 4,
                borderRadius: 999,
                background: colors.faq,
              }}
            >
              {[
                ['login', t('common.signIn')],
                ['register', t('common.createAccount')],
              ].map(([mode, label]) => {
                const active = authMode === mode;
                return (
                  <button
                    key={mode}
                    type="button"
                    className="hm-btn"
                    onClick={() => {
                      setAuthMode(mode);
                      setShowPassword(false);
                      setShowConfirm(false);
                    }}
                    style={{
                      ...primaryButton,
                      flex: 1,
                      height: 42,
                      padding: '0 16px',
                      background: active ? colors.ink : 'transparent',
                      color: active ? colors.white : colors.muted,
                      border: active ? '1px solid rgba(15,23,42,0.92)' : '1px solid transparent',
                      boxShadow: 'none',
                    }}
                  >
                    {label}
                  </button>
                );
              })}
            </div>

            <form onSubmit={handleAuthSubmit} style={{ display: 'grid', gap: 18 }}>
              <label style={{ display: 'grid', gap: 8 }}>
                <span style={{ fontSize: 13, color: colors.ink, fontWeight: 600 }}>{account.username}</span>
                <input
                  className="hm-field-light"
                  value={username}
                  onChange={(e) => setUsername(e.target.value)}
                  style={lightField}
                  autoComplete="username"
                  required
                />
              </label>

              <label style={{ display: 'grid', gap: 8 }}>
                <span style={{ fontSize: 13, color: colors.ink, fontWeight: 600 }}>{account.password}</span>
                <div style={{ position: 'relative' }}>
                  <input
                    className="hm-field-light"
                    type={showPassword ? 'text' : 'password'}
                    value={password}
                    onChange={(e) => setPassword(e.target.value)}
                    style={{ ...lightField, paddingRight: 48 }}
                    autoComplete={isRegister ? 'new-password' : 'current-password'}
                    required
                  />
                  <PasswordToggle
                    visible={showPassword}
                    label={showPassword ? account.hidePassword : account.showPassword}
                    onClick={() => setShowPassword((v) => !v)}
                  />
                </div>
              </label>

              {isRegister ? (
                <label style={{ display: 'grid', gap: 8 }}>
                  <span style={{ fontSize: 13, color: colors.ink, fontWeight: 600 }}>{account.confirm}</span>
                  <div style={{ position: 'relative' }}>
                    <input
                      className="hm-field-light"
                      type={showConfirm ? 'text' : 'password'}
                      value={confirmPassword}
                      onChange={(e) => setConfirmPassword(e.target.value)}
                      style={{ ...lightField, paddingRight: 48 }}
                      autoComplete="new-password"
                      required
                    />
                    <PasswordToggle
                      visible={showConfirm}
                      label={showConfirm ? account.hidePassword : account.showPassword}
                      onClick={() => setShowConfirm((v) => !v)}
                    />
                  </div>
                </label>
              ) : null}

              <button type="submit" disabled={loading} className="hm-btn" style={{ ...charcoalButton, height: 48, marginTop: 4 }}>
                {busyAction === 'auth'
                  ? t('common.working')
                  : isRegister
                    ? account.createAndSignIn
                    : t('common.signIn')}
              </button>
            </form>

            <div style={{ marginTop: 28, display: 'flex', flexWrap: 'wrap', gap: 8, alignItems: 'center', color: colors.muted, fontSize: 14 }}>
              <span>{isRegister ? account.hasAccount : account.noAccount}</span>
              <button
                type="button"
                onClick={() => {
                  setAuthMode(isRegister ? 'login' : 'register');
                  setShowPassword(false);
                  setShowConfirm(false);
                }}
                style={{
                  border: 0,
                  background: 'transparent',
                  color: '#0e7490',
                  fontWeight: 700,
                  cursor: 'pointer',
                  padding: 0,
                  fontFamily: font.sans,
                }}
              >
                {isRegister ? t('common.signIn') : t('common.createAccount')}
              </button>
            </div>

            <LightStatusBanner tone={statusTone}>{status}</LightStatusBanner>
          </div>
        </div>
      </div>

      <style>{`
        @keyframes hmAuthFloat {
          0%, 100% { transform: translateY(0) rotate(0deg); }
          50% { transform: translateY(-18px) rotate(3deg); }
        }
        @keyframes hmAuthTwinkle {
          0%, 100% { opacity: 0.35; transform: scale(1); }
          50% { opacity: 0.9; transform: scale(1.15); }
        }
        .hm-auth-char {
          position: absolute;
          display: grid;
          place-items: center;
          box-shadow: 0 18px 36px rgba(15,23,42,0.12);
        }
        .hm-auth-char-face {
          position: relative;
          width: 100%;
          height: 100%;
        }
        .hm-auth-eye {
          position: absolute;
          width: 10px;
          height: 10px;
          border-radius: 50%;
          background: #fff;
          top: 38%;
        }
        .hm-auth-eye::after {
          content: '';
          position: absolute;
          width: 3px;
          height: 3px;
          border-radius: 50%;
          background: rgba(15,23,42,0.55);
          top: 2px;
          right: 2px;
        }
        .hm-auth-smile {
          position: absolute;
          left: 50%;
          bottom: 28%;
          width: 14px;
          height: 8px;
          border-bottom: 2px solid rgba(255,255,255,0.92);
          border-radius: 0 0 12px 12px;
          transform: translateX(-50%);
        }
        .hm-auth-dot {
          position: absolute;
          width: 8px;
          height: 8px;
          border-radius: 50%;
          background: rgba(6,182,212,0.55);
          animation: hmAuthTwinkle 3s ease-in-out infinite;
        }
        @media (max-width: 1024px) {
          .hm-auth-grid { grid-template-columns: 1fr !important; }
          .hm-auth-brand { min-height: 320px !important; border-right: 0 !important; border-bottom: 1px solid rgba(15,23,42,0.06); }
          .hm-auth-form-pane { padding: 40px 24px 72px !important; }
        }
      `}</style>
    </section>
  );
}

function PasswordToggle({ visible, label, onClick }) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onClick={onClick}
      style={{
        position: 'absolute',
        right: 12,
        top: '50%',
        transform: 'translateY(-50%)',
        width: 28,
        height: 28,
        border: 0,
        borderRadius: 8,
        background: 'transparent',
        color: '#9CA3AF',
        cursor: 'pointer',
        display: 'grid',
        placeItems: 'center',
        padding: 0,
      }}
    >
      {visible ? (
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
          <path d="M3 3l18 18" />
          <path d="M10.6 10.6a2 2 0 0 0 2.8 2.8" />
          <path d="M9.9 5.1A10.5 10.5 0 0 1 12 5c5 0 9.3 3.1 11 7.5a11.7 11.7 0 0 1-2.5 3.8" />
          <path d="M6.1 6.1A11.8 11.8 0 0 0 1 12.5C2.7 16.9 7 20 12 20c1.6 0 3.1-.3 4.5-.9" />
        </svg>
      ) : (
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
          <path d="M1 12.5C2.7 8.1 7 5 12 5s9.3 3.1 11 7.5c-1.7 4.4-6 7.5-11 7.5S2.7 16.9 1 12.5z" />
          <circle cx="12" cy="12.5" r="3.2" />
        </svg>
      )}
    </button>
  );
}

function FloatingCharacters() {
  return (
    <div style={{ position: 'relative', width: 280, height: 220 }}>
      <span className="hm-auth-dot" style={{ left: 18, top: 24, animationDelay: '0.2s' }} />
      <span className="hm-auth-dot" style={{ right: 36, top: 48, width: 6, height: 6, animationDelay: '1.1s' }} />
      <span className="hm-auth-dot" style={{ left: 54, bottom: 28, width: 10, height: 10, background: 'rgba(14,116,144,0.35)', animationDelay: '0.7s' }} />

      <div
        className="hm-auth-char"
        style={{
          left: 28,
          top: 34,
          width: 72,
          height: 96,
          borderRadius: 48,
          background: 'linear-gradient(160deg, #06b6d4 0%, #0891b2 100%)',
          animation: 'hmAuthFloat 6s ease-in-out infinite',
        }}
      >
        <CharacterFace />
      </div>

      <div
        className="hm-auth-char"
        style={{
          right: 34,
          top: 18,
          width: 64,
          height: 88,
          borderRadius: 42,
          background: colors.ink,
          animation: 'hmAuthFloat 5s ease-in-out infinite',
          animationDelay: '0.4s',
        }}
      >
        <CharacterFace />
      </div>

      <div
        className="hm-auth-char"
        style={{
          left: 96,
          bottom: 18,
          width: 88,
          height: 44,
          borderRadius: '44px 44px 12px 12px',
          background: 'linear-gradient(135deg, #22d3ee 0%, #67e8f9 100%)',
          animation: 'hmAuthFloat 4.5s ease-in-out infinite',
          animationDelay: '0.8s',
        }}
      >
        <CharacterFace smileBottom="22%" />
      </div>

      <div
        className="hm-auth-char"
        style={{
          right: 48,
          bottom: 42,
          width: 64,
          height: 80,
          borderRadius: 50,
          background: 'linear-gradient(160deg, #0e7490 0%, #155e75 100%)',
          animation: 'hmAuthFloat 5.5s ease-in-out infinite',
          animationDelay: '1.2s',
        }}
      >
        <CharacterFace />
      </div>
    </div>
  );
}

function CharacterFace({ smileBottom = '28%' }) {
  return (
    <div className="hm-auth-char-face" aria-hidden>
      <span className="hm-auth-eye" style={{ left: '28%' }} />
      <span className="hm-auth-eye" style={{ right: '28%' }} />
      <span className="hm-auth-smile" style={{ bottom: smileBottom }} />
    </div>
  );
}

function LightStatusBanner({ tone = 'neutral', children }) {
  if (!children) return null;
  const styles = {
    ok: { bg: 'rgba(16, 185, 129, 0.1)', border: 'rgba(16, 185, 129, 0.22)', color: '#047857' },
    err: { bg: 'rgba(239, 68, 68, 0.08)', border: 'rgba(239, 68, 68, 0.18)', color: '#b91c1c' },
    neutral: { bg: 'rgba(15,23,42,0.04)', border: 'rgba(15,23,42,0.08)', color: colors.muted },
  }[tone] || {
    bg: 'rgba(15,23,42,0.04)',
    border: 'rgba(15,23,42,0.08)',
    color: colors.muted,
  };

  return (
    <div
      style={{
        marginTop: 16,
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
