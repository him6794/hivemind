import React, { useState } from 'react';
import SectionEyebrow from '../components/SectionEyebrow';
import StatusBanner from '../components/StatusBanner';
import { colors, font, glassSoft, glassStrong, primaryButton, secondaryButton, fieldStyle } from '../theme';
import { getSection } from '../i18n';

export default function AccountPage({
  lang,
  t,
  navigate,
  apiBase,
  masterUi,
  workerUi,
  session,
}) {
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

  return (
    <section style={{ padding: '48px 18px 88px' }}>
      <div style={{ maxWidth: 1180, margin: '0 auto' }}>
        <SectionEyebrow dark>{account.title}</SectionEyebrow>
        <h1
          style={{
            margin: '16px 0 12px',
            fontFamily: font.serif,
            fontWeight: 600,
            fontSize: 'clamp(34px, 4.8vw, 56px)',
            lineHeight: 1.1,
            letterSpacing: '-0.03em',
            maxWidth: 760,
          }}
        >
          {account.title}
        </h1>
        <p style={{ margin: '0 0 28px', maxWidth: 680, color: 'rgba(226,232,240,0.78)', lineHeight: 1.8 }}>
          {account.body}
        </p>

        <div className="hm-account-grid" style={{ display: 'grid', gridTemplateColumns: 'minmax(0, 0.95fr) minmax(320px, 0.85fr)', gap: 18 }}>
          <div style={{ ...glassSoft, borderRadius: 24, padding: 24 }}>
            <h2 style={{ margin: '0 0 10px', fontFamily: font.serif, fontSize: 28 }}>{account.apiSurface}</h2>
            <p style={{ margin: 0, color: 'rgba(226,232,240,0.72)', lineHeight: 1.75 }}>
              {apiBase}
            </p>
            <div style={{ display: 'flex', gap: 10, flexWrap: 'wrap', marginTop: 22 }}>
              <a href={masterUi} target="_blank" rel="noreferrer" className="hm-btn" style={primaryButton}>
                {t('common.openMaster')}
              </a>
              <a href={workerUi} target="_blank" rel="noreferrer" className="hm-btn" style={secondaryButton}>
                {t('common.openWorker')}
              </a>
              <button type="button" className="hm-btn" style={secondaryButton} onClick={() => navigate('/vpn')}>
                {account.goVpn}
              </button>
            </div>
          </div>

          <div style={{ ...glassStrong, borderRadius: 24, padding: 14 }}>
            <div style={{ ...glassSoft, borderRadius: 18, padding: 18 }}>
              {!loggedIn ? (
                <>
                  <div style={{ display: 'flex', gap: 8, marginBottom: 14 }}>
                    {[
                      ['login', t('common.signIn')],
                      ['register', t('common.createAccount')],
                    ].map(([mode, label]) => (
                      <button
                        key={mode}
                        type="button"
                        className="hm-btn"
                        onClick={() => setAuthMode(mode)}
                        style={{
                          ...primaryButton,
                          flex: 1,
                          background:
                            authMode === mode
                              ? 'linear-gradient(135deg, #6366f1 0%, #7c3aed 55%, #a855f7 100%)'
                              : 'rgba(255,255,255,0.05)',
                          boxShadow: authMode === mode ? primaryButton.boxShadow : 'none',
                          border:
                            authMode === mode
                              ? '1px solid rgba(99, 102, 241, 0.45)'
                              : '1px solid rgba(255,255,255,0.12)',
                        }}
                      >
                        {label}
                      </button>
                    ))}
                  </div>

                  <form onSubmit={handleAuthSubmit} style={{ display: 'grid', gap: 12 }}>
                    <label style={{ display: 'grid', gap: 6 }}>
                      <span style={{ fontSize: 12, color: 'rgba(226,232,240,0.7)' }}>{account.username}</span>
                      <input
                        className="hm-field"
                        value={username}
                        onChange={(e) => setUsername(e.target.value)}
                        style={fieldStyle}
                        autoComplete="username"
                        required
                      />
                    </label>
                    <label style={{ display: 'grid', gap: 6 }}>
                      <span style={{ fontSize: 12, color: 'rgba(226,232,240,0.7)' }}>{account.password}</span>
                      <input
                        className="hm-field"
                        type="password"
                        value={password}
                        onChange={(e) => setPassword(e.target.value)}
                        style={fieldStyle}
                        autoComplete={authMode === 'register' ? 'new-password' : 'current-password'}
                        required
                      />
                    </label>
                    {authMode === 'register' ? (
                      <label style={{ display: 'grid', gap: 6 }}>
                        <span style={{ fontSize: 12, color: 'rgba(226,232,240,0.7)' }}>{account.confirm}</span>
                        <input
                          className="hm-field"
                          type="password"
                          value={confirmPassword}
                          onChange={(e) => setConfirmPassword(e.target.value)}
                          style={fieldStyle}
                          autoComplete="new-password"
                          required
                        />
                      </label>
                    ) : null}
                    <button type="submit" disabled={loading} className="hm-btn" style={primaryButton}>
                      {busyAction === 'auth'
                        ? t('common.working')
                        : authMode === 'register'
                          ? account.createAndSignIn
                          : t('common.signIn')}
                    </button>
                  </form>
                </>
              ) : (
                <div style={{ display: 'grid', gap: 14 }}>
                  <div style={{ display: 'flex', justifyContent: 'space-between', gap: 10, alignItems: 'center', flexWrap: 'wrap' }}>
                    <div>
                      <div style={{ fontSize: 12, color: 'rgba(226,232,240,0.62)', fontFamily: font.display, letterSpacing: '0.08em', textTransform: 'uppercase' }}>
                        {t('common.signedIn')}
                      </div>
                      <div style={{ fontFamily: font.serif, fontSize: 28, fontWeight: 600, letterSpacing: '-0.02em' }}>
                        {sessionUser || username || 'user'}
                      </div>
                    </div>
                    <button type="button" className="hm-btn" onClick={() => logout()} style={{ ...secondaryButton, background: 'transparent' }}>
                      {t('common.signOut')}
                    </button>
                  </div>

                  <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 10 }}>
                    <div style={{ ...glassSoft, borderRadius: 14, padding: 14 }}>
                      <div style={{ fontSize: 12, color: 'rgba(226,232,240,0.62)' }}>{account.balance}</div>
                      <div style={{ marginTop: 4, fontFamily: font.display, fontSize: 30, fontWeight: 700, color: '#c7d2fe' }}>
                        {bootstrapping ? t('common.loading') : balance ?? '—'}
                      </div>
                    </div>
                    <div style={{ ...glassSoft, borderRadius: 14, padding: 14 }}>
                      <div style={{ fontSize: 12, color: 'rgba(226,232,240,0.62)' }}>{account.apiSurface}</div>
                      <div style={{ marginTop: 8, fontSize: 12, lineHeight: 1.5, wordBreak: 'break-all', color: 'rgba(248,250,252,0.88)' }}>
                        {apiBase}
                      </div>
                    </div>
                  </div>

                  <form onSubmit={handleTransfer} style={{ display: 'grid', gap: 10 }}>
                    <div style={{ fontFamily: font.display, fontSize: 13, fontWeight: 600, letterSpacing: '0.04em' }}>
                      {account.transfer}
                    </div>
                    <input
                      className="hm-field"
                      value={transferTo}
                      onChange={(e) => setTransferTo(e.target.value)}
                      style={fieldStyle}
                      placeholder={account.recipient}
                      autoComplete="off"
                    />
                    <input
                      className="hm-field"
                      value={transferAmount}
                      onChange={(e) => setTransferAmount(e.target.value)}
                      style={fieldStyle}
                      placeholder={account.amount}
                      inputMode="numeric"
                    />
                    <button type="submit" disabled={loading} className="hm-btn" style={primaryButton}>
                      {busyAction === 'transfer' ? account.sending : account.send}
                    </button>
                    {transferNote ? <div style={{ fontSize: 12, color: 'rgba(226,232,240,0.68)' }}>{transferNote}</div> : null}
                  </form>
                </div>
              )}

              <StatusBanner tone={statusTone}>{status}</StatusBanner>
            </div>
          </div>
        </div>
      </div>
    </section>
  );
}
