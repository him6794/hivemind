import React, { useState } from 'react';
import { Code2, Cpu, Download, Server } from 'lucide-react';
import SectionEyebrow from '../components/SectionEyebrow';
import { AnimatedStepper, Step } from '../components/AnimatedStepper';
import { colors, font, pageWrap, primaryButton, secondaryButton } from '../theme';
import { getSection } from '../i18n';

const packageIcons = {
  requestor: Cpu,
  provider: Server,
  source: Code2,
};

const lightSecondary = {
  ...secondaryButton,
  background: colors.white,
  color: colors.ink,
  border: '1px solid rgba(15,23,42,0.12)',
  boxShadow: 'none',
};

export default function DownloadPage({ lang, t, navigate }) {
  const download = getSection(lang, 'download');
  const [selected, setSelected] = useState(download.packages[0]?.id || 'requestor');
  const selectedPackage = download.packages.find((item) => item.id === selected) || download.packages[0];

  function handlePackageAction() {
    if (selectedPackage?.id === 'source') {
      window.open('https://github.com/him6794/hivemind', '_blank', 'noreferrer');
      return;
    }
    window.alert(download.comingSoon);
  }

  return (
    <section style={{ background: colors.surface, color: colors.ink, padding: '96px 24px 120px' }}>
      <div style={pageWrap}>
        <div style={{ maxWidth: 760, margin: '0 auto 40px', textAlign: 'center', display: 'flex', flexDirection: 'column', alignItems: 'center' }}>
          <SectionEyebrow>{download.kicker || download.title}</SectionEyebrow>
          <h1
            style={{
              margin: '22px 0 18px',
              fontFamily: font.serif,
              fontWeight: 500,
              fontSize: 'clamp(44px, 6vw, 68px)',
              lineHeight: 0.98,
              letterSpacing: '-0.04em',
              textAlign: 'center',
            }}
          >
            {download.title}
          </h1>
          <p style={{ margin: 0, maxWidth: 640, color: colors.muted, lineHeight: 1.85, fontSize: 16, textAlign: 'center' }}>
            {download.body}
          </p>
        </div>

        <div
          style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(3, minmax(0, 1fr))',
            gap: 12,
            marginBottom: 24,
          }}
          className="hm-dual-grid"
        >
          {download.packages.map((item) => {
            const Icon = packageIcons[item.id] || Download;
            const active = selected === item.id;
            return (
              <button
                key={item.id}
                type="button"
                onClick={() => setSelected(item.id)}
                style={{
                  textAlign: 'left',
                  cursor: 'pointer',
                  borderRadius: 22,
                  padding: '18px 18px',
                  border: active ? '1px solid rgba(6,182,212,0.45)' : '1px solid rgba(15,23,42,0.08)',
                  background: active ? 'rgba(6,182,212,0.08)' : colors.white,
                  boxShadow: active ? '0 12px 28px rgba(6,182,212,0.12)' : '0 10px 24px rgba(15,23,42,0.04)',
                  color: colors.ink,
                  display: 'grid',
                  gap: 10,
                }}
              >
                <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
                  <span
                    style={{
                      width: 36,
                      height: 36,
                      borderRadius: 12,
                      display: 'grid',
                      placeItems: 'center',
                      background: active ? 'rgba(6,182,212,0.16)' : colors.faq,
                      color: active ? '#0e7490' : colors.muted,
                    }}
                  >
                    <Icon size={18} />
                  </span>
                  <span style={{ fontFamily: font.display, fontWeight: 700, fontSize: 14, letterSpacing: '-0.01em' }}>
                    {item.label}
                  </span>
                </div>
                <div style={{ color: colors.muted, fontSize: 13, lineHeight: 1.55 }}>{item.desc}</div>
              </button>
            );
          })}
        </div>

        <div className="hm-dual-grid" style={{ display: 'grid', gridTemplateColumns: '0.95fr 1.05fr', gap: 22, alignItems: 'start' }}>
          <article
            style={{
              background: colors.white,
              borderRadius: 28,
              border: '1px solid rgba(15,23,42,0.08)',
              boxShadow: '0 18px 40px rgba(15,23,42,0.04)',
              padding: 32,
            }}
          >
            <div
              style={{
                display: 'inline-flex',
                alignItems: 'center',
                gap: 8,
                padding: '6px 12px',
                borderRadius: 999,
                background: 'rgba(6,182,212,0.1)',
                color: '#0e7490',
                fontSize: 11,
                fontWeight: 700,
                letterSpacing: '0.12em',
                textTransform: 'uppercase',
                fontFamily: font.display,
              }}
            >
              <Download size={14} />
              {selectedPackage.label}
            </div>
            <h2
              style={{
                margin: '18px 0 12px',
                fontFamily: font.serif,
                fontWeight: 500,
                fontSize: 'clamp(30px, 3.6vw, 38px)',
                letterSpacing: '-0.03em',
                lineHeight: 1.1,
                color: colors.ink,
              }}
            >
              {selectedPackage.title}
            </h2>
            <p style={{ margin: '0 0 14px', color: colors.muted, lineHeight: 1.8, fontSize: 15 }}>{selectedPackage.desc}</p>
            <div
              style={{
                color: colors.muted,
                fontSize: 12,
                marginBottom: 24,
                fontFamily: font.display,
                letterSpacing: '0.08em',
                textTransform: 'uppercase',
              }}
            >
              {selectedPackage.platform}
            </div>
            <div style={{ display: 'flex', gap: 10, flexWrap: 'wrap' }}>
              <button type="button" className="hm-btn" style={primaryButton} onClick={handlePackageAction}>
                {selectedPackage.action}
              </button>
              <button type="button" className="hm-btn" style={lightSecondary} onClick={() => navigate('/account')}>
                {download.openAccount}
              </button>
            </div>
            <p style={{ margin: '20px 0 0', color: colors.muted, lineHeight: 1.75, fontSize: 13 }}>{download.note}</p>
          </article>

          <div>
            <div
              style={{
                marginBottom: 14,
                fontFamily: font.display,
                letterSpacing: '0.14em',
                textTransform: 'uppercase',
                color: colors.muted,
                fontSize: 11,
                fontWeight: 700,
              }}
            >
              {download.stepsTitle}
            </div>
            <AnimatedStepper
              tone="light"
              backButtonText={t('common.back')}
              nextButtonText={t('common.continue')}
              completeButtonText={t('common.complete')}
              onFinalStepCompleted={() => navigate('/account')}
            >
              <Step title={download.stepAccountTitle}>
                <p style={{ margin: 0 }}>{download.stepAccountBody}</p>
              </Step>
              <Step title={download.stepDownloadTitle}>
                <p style={{ margin: 0 }}>{download.stepDownloadBody}</p>
              </Step>
              <Step title={download.stepConfigureTitle}>
                <p style={{ margin: 0 }}>{download.stepConfigureBody}</p>
              </Step>
              <Step title={download.stepReadyTitle}>
                <p style={{ margin: 0 }}>{download.stepReadyBody}</p>
              </Step>
            </AnimatedStepper>
          </div>
        </div>
      </div>
    </section>
  );
}
