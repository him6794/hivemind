import React from 'react';
import SectionEyebrow from '../components/SectionEyebrow';
import StatusBanner from '../components/StatusBanner';
import CopyButton from '../components/CopyButton';
import { colors, font, glassSoft, glassStrong, primaryButton, secondaryButton, fieldStyle } from '../theme';
import { getSection } from '../i18n';

export default function VpnPage({ lang, t, navigate, session }) {
  const vpn = getSection(lang, 'vpn');
  const {
    token,
    loading,
    busyAction,
    status,
    statusTone,
    vpnClientName,
    setVpnClientName,
    vpnConfig,
    handleIssueVpnConfig,
    downloadVpnConfig,
    joinCommand,
  } = session;

  return (
    <section style={{ padding: '48px 18px 88px' }}>
      <div style={{ maxWidth: 1180, margin: '0 auto' }}>
        <SectionEyebrow dark>{vpn.title}</SectionEyebrow>
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
          {vpn.title}
        </h1>
        <p style={{ margin: '0 0 28px', maxWidth: 720, color: 'rgba(226,232,240,0.78)', lineHeight: 1.8 }}>
          {vpn.body}
        </p>

        <div className="hm-vpn-grid" style={{ display: 'grid', gridTemplateColumns: 'minmax(0, 0.9fr) minmax(320px, 0.9fr)', gap: 18 }}>
          <div style={{ ...glassSoft, borderRadius: 24, padding: 24 }}>
            <h2 style={{ margin: '0 0 14px', fontFamily: font.serif, fontSize: 26 }}>{vpn.stepsTitle}</h2>
            <ol style={{ margin: 0, paddingLeft: 18, display: 'grid', gap: 12, color: 'rgba(226,232,240,0.8)', lineHeight: 1.7 }}>
              {vpn.steps.map((step) => (
                <li key={step}>{step}</li>
              ))}
            </ol>
            <p style={{ margin: '20px 0 0', color: 'rgba(226,232,240,0.68)', lineHeight: 1.7 }}>{vpn.note}</p>
            {!token ? (
              <button type="button" className="hm-btn" style={{ ...primaryButton, marginTop: 22 }} onClick={() => navigate('/account')}>
                {t('common.signIn')}
              </button>
            ) : null}
          </div>

          <div style={{ ...glassStrong, borderRadius: 24, padding: 14 }}>
            <div style={{ ...glassSoft, borderRadius: 18, padding: 18, display: 'grid', gap: 12 }}>
              <form onSubmit={handleIssueVpnConfig} style={{ display: 'grid', gap: 10 }}>
                <label style={{ display: 'grid', gap: 6 }}>
                  <span style={{ fontSize: 12, color: 'rgba(226,232,240,0.7)' }}>{vpn.device}</span>
                  <input
                    className="hm-field"
                    value={vpnClientName}
                    onChange={(e) => setVpnClientName(e.target.value)}
                    style={fieldStyle}
                    placeholder={vpn.devicePh}
                  />
                </label>
                <button type="submit" disabled={loading || !token} className="hm-btn" style={primaryButton}>
                  {busyAction === 'vpn' ? vpn.issuing : vpn.generate}
                </button>
              </form>

              {vpnConfig ? (
                <div
                  style={{
                    ...glassSoft,
                    borderRadius: 14,
                    padding: 12,
                    fontSize: 12,
                    lineHeight: 1.7,
                    wordBreak: 'break-all',
                    display: 'grid',
                    gap: 6,
                  }}
                >
                  <div><strong style={{ color: '#c7d2fe' }}>login_server</strong>: {vpnConfig.login_server}</div>
                  <div><strong style={{ color: '#c7d2fe' }}>client_id</strong>: {vpnConfig.client_id}</div>
                  <div><strong style={{ color: '#c7d2fe' }}>virtual_ip</strong>: {vpnConfig.virtual_ip}</div>
                  <div><strong style={{ color: '#c7d2fe' }}>expires_at</strong>: {vpnConfig.expires_at}</div>
                  <div><strong style={{ color: '#c7d2fe' }}>auth_key</strong>: {vpnConfig.auth_key}</div>
                  <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap', marginTop: 4 }}>
                    <CopyButton text={vpnConfig.auth_key} label={vpn.copyKey} copiedLabel={t('common.copied')} />
                    <CopyButton text={joinCommand} label={vpn.copyJoin} copiedLabel={t('common.copied')} />
                    <button
                      type="button"
                      className="hm-btn"
                      onClick={downloadVpnConfig}
                      style={{ ...secondaryButton, padding: '8px 12px', fontSize: 12, background: 'transparent' }}
                    >
                      {t('common.download')}
                    </button>
                  </div>
                  <code
                    style={{
                      display: 'block',
                      marginTop: 4,
                      padding: 10,
                      borderRadius: 10,
                      background: 'rgba(15,23,42,0.55)',
                      border: '1px solid rgba(255,255,255,0.08)',
                      whiteSpace: 'pre-wrap',
                      color: '#e2e8f0',
                    }}
                  >
                    {joinCommand}
                  </code>
                </div>
              ) : null}

              <StatusBanner tone={statusTone}>{status}</StatusBanner>
            </div>
          </div>
        </div>
      </div>
    </section>
  );
}
