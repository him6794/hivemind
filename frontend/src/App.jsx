import React, { useEffect, useMemo, useState } from 'react';

const rootStyles = {
  minHeight: '100vh',
  color: '#f4efe7',
  background:
    'radial-gradient(circle at top left, rgba(230, 177, 107, 0.14), transparent 26%), radial-gradient(circle at 80% 15%, rgba(163, 140, 108, 0.14), transparent 24%), linear-gradient(180deg, #090705 0%, #120f0d 46%, #f3ede4 46%, #f6f1e8 100%)',
  fontFamily: 'ui-sans-serif, system-ui, sans-serif',
};

const shellStyle = {
  borderRadius: 28,
  border: '1px solid rgba(255,255,255,0.12)',
  background: 'rgba(11, 10, 8, 0.78)',
  boxShadow: '0 30px 80px rgba(0, 0, 0, 0.25)',
  backdropFilter: 'blur(24px)',
};

const innerStyle = {
  borderRadius: 22,
  border: '1px solid rgba(255,255,255,0.08)',
  background: 'rgba(255,255,255,0.04)',
};

const sectionCard = {
  borderRadius: 24,
  border: '1px solid rgba(0,0,0,0.08)',
  background: '#fffaf2',
  boxShadow: '0 22px 60px rgba(31, 24, 15, 0.08)',
};

const fieldStyle = {
  width: '100%',
  boxSizing: 'border-box',
  padding: '13px 14px',
  borderRadius: 14,
  border: '1px solid rgba(245, 234, 218, 0.25)',
  background: 'rgba(255,255,255,0.06)',
  color: '#fff',
  outline: 'none',
};

const lightFieldStyle = {
  width: '100%',
  boxSizing: 'border-box',
  padding: '13px 14px',
  borderRadius: 14,
  border: '1px solid rgba(33, 23, 14, 0.14)',
  background: '#fff',
  color: '#21170e',
  outline: 'none',
};

const primaryButton = {
  display: 'inline-flex',
  alignItems: 'center',
  gap: 10,
  padding: '13px 18px',
  borderRadius: 999,
  border: '1px solid rgba(255,255,255,0.14)',
  background: 'linear-gradient(135deg, #e8c27d 0%, #c58d3a 100%)',
  color: '#160f06',
  fontWeight: 800,
  textDecoration: 'none',
  cursor: 'pointer',
};

const secondaryButton = {
  ...primaryButton,
  background: 'rgba(255,255,255,0.05)',
  color: '#fff',
};

function useReveal() {
  const [visible, setVisible] = useState(false);

  useEffect(() => {
    const node = document.querySelector('[data-reveal-root]');
    if (!node) {
      setVisible(true);
      return undefined;
    }
    const observer = new IntersectionObserver(
      ([entry]) => {
        if (entry.isIntersecting) {
          setVisible(true);
        }
      },
      { threshold: 0.12 },
    );
    observer.observe(node);
    return () => observer.disconnect();
  }, []);

  return visible;
}

function Stat({ value, label }) {
  return (
    <div style={{ padding: 18, borderRadius: 22, background: 'rgba(255,255,255,0.05)', border: '1px solid rgba(255,255,255,0.08)' }}>
      <div style={{ fontSize: 28, fontWeight: 800, color: '#f7d28a' }}>{value}</div>
      <div style={{ marginTop: 6, color: 'rgba(255,255,255,0.72)' }}>{label}</div>
    </div>
  );
}

function SectionTitle({ eyebrow, title, body }) {
  return (
    <div style={{ marginBottom: 22 }}>
      <div
        style={{
          display: 'inline-flex',
          alignItems: 'center',
          gap: 8,
          padding: '6px 11px',
          borderRadius: 999,
          background: 'rgba(197, 141, 58, 0.12)',
          border: '1px solid rgba(197, 141, 58, 0.18)',
          color: '#8d5c1f',
          letterSpacing: '0.18em',
          textTransform: 'uppercase',
          fontSize: 11,
          fontWeight: 700,
        }}
      >
        {eyebrow}
      </div>
      <h2 style={{ margin: '14px 0 10px', fontSize: 'clamp(30px, 4vw, 52px)', lineHeight: 1.05, color: '#1b130d' }}>
        {title}
      </h2>
      <p style={{ margin: 0, maxWidth: 760, color: '#5e5143', fontSize: 16, lineHeight: 1.8 }}>{body}</p>
    </div>
  );
}

export default function App() {
  const apiBase = String(import.meta.env.VITE_API_BASE || 'http://localhost:8082').trim().replace(/\/$/, '');
  const [authMode, setAuthMode] = useState('register');
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [confirmPassword, setConfirmPassword] = useState('');
  const [status, setStatus] = useState('建立帳戶，登入後即可上傳 ZIP 任務');
  const [loading, setLoading] = useState(false);
  const [token, setToken] = useState('');
  const [balance, setBalance] = useState(null);

  const reveal = useReveal();

  async function readJson(res) {
    const text = await res.text();
    if (!text) return {};
    try {
      return JSON.parse(text);
    } catch {
      return {};
    }
  }

  async function api(method, path, body, authToken = token) {
    const headers = {};
    if (authToken) headers.Authorization = `Bearer ${authToken}`;
    if (body !== undefined) headers['Content-Type'] = 'application/json';

    const res = await fetch(`${apiBase}${path}`, {
      method,
      headers,
      body: body !== undefined ? JSON.stringify(body) : undefined,
    });
    const data = await readJson(res);
    return { ok: res.ok, data };
  }

  async function refreshBalance(authToken = token) {
    if (!authToken) return;
    const { data } = await api('GET', '/api/balance', undefined, authToken);
    if (data.success) {
      setBalance(Number(data.balance || 0));
    }
  }

  useEffect(() => {
    if (!token) return undefined;
    refreshBalance(token).catch(() => {});
    const id = setInterval(() => {
      refreshBalance(token).catch(() => {});
    }, 10000);
    return () => clearInterval(id);
  }, [token]);

  useEffect(() => {
    document.title = 'Hivemind | Distributed Compute Platform';
  }, []);

  const benefitCards = useMemo(
    () => [
      {
        title: 'ZIP-first task flow',
        body: '直接上傳 ZIP 就能建立任務，系統會自動轉成 torrent 並進行播種，省掉手動包裝與發佈步驟。',
      },
      {
        title: 'Role-separated consoles',
        body: 'Master 只負責任務與結果，Worker 只負責節點註冊與本機容量，避免把管理面板混在一起。',
      },
      {
        title: 'Auto-seeding delivery',
        body: '上傳後自動作種，任務包與結果流轉在分散式網路中，適合大檔案、批次處理與可重現工作負載。',
      },
      {
        title: 'Real usage telemetry',
        body: '以 CPU / GPU score 與實際執行時間追蹤資源使用，讓排程與報表更接近真實容量。',
      },
    ],
    [],
  );

  const tutorialSteps = useMemo(
    () => [
      {
        step: '01',
        title: '註冊帳戶',
        body: '輸入帳號密碼建立新帳戶。註冊完成後可直接登入官網，開始建立任務。',
      },
      {
        step: '02',
        title: '上傳 ZIP',
        body: '在 master 介面選擇 ZIP 檔，上傳後平台會自動建立 torrent 與播種來源。',
      },
      {
        step: '03',
        title: '追蹤進度',
        body: '在任務列表查看執行狀態、wall time、輸出日誌與完成結果。',
      },
      {
        step: '04',
        title: '下載結果',
        body: '任務完成後直接下載 artifact，保留同一份提交內容與輸出資料。',
      },
    ],
    [],
  );

  async function handleAuthSubmit(e) {
    e.preventDefault();
    setLoading(true);
    setStatus(authMode === 'register' ? '建立帳戶中...' : '登入中...');

    try {
      if (authMode === 'register') {
        if (password !== confirmPassword) {
          throw new Error('兩次輸入的密碼不一致');
        }
        const register = await api('POST', '/api/register', { username, password }, '');
        if (!register.data.success) {
          throw new Error(register.data.message || '註冊失敗');
        }
      }

      const login = await api('POST', '/api/login', { username, password }, '');
      if (!login.data.success || !login.data.token) {
        throw new Error(login.data.message || login.data.status_message || '登入失敗');
      }

      const nextToken = login.data.token;
      setToken(nextToken);
      setStatus(authMode === 'register' ? '帳戶已建立並登入' : '登入成功');
      await refreshBalance(nextToken);
    } catch (err) {
      setStatus(err.message);
    } finally {
      setLoading(false);
    }
  }

  function logout() {
    setToken('');
    setBalance(null);
    setStatus('已登出');
  }

  return (
    <main style={rootStyles} data-reveal-root>
      <section style={{ padding: '24px 20px 0' }}>
        <div
          style={{
            maxWidth: 1240,
            margin: '0 auto',
            padding: '16px 16px 18px',
            borderRadius: 999,
            border: '1px solid rgba(255,255,255,0.12)',
            background: 'rgba(11, 10, 8, 0.6)',
            boxShadow: '0 20px 60px rgba(0,0,0,0.2)',
            backdropFilter: 'blur(18px)',
            display: 'flex',
            justifyContent: 'space-between',
            gap: 14,
            flexWrap: 'wrap',
            alignItems: 'center',
          }}
        >
          <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
            <div
              style={{
                width: 42,
                height: 42,
                borderRadius: 14,
                background: 'linear-gradient(135deg, #f0d59a 0%, #b3782e 100%)',
                boxShadow: '0 10px 20px rgba(179, 120, 46, 0.25)',
              }}
            />
            <div>
              <div style={{ fontSize: 18, fontWeight: 800, letterSpacing: '0.04em' }}>Hivemind</div>
              <div style={{ color: 'rgba(255,255,255,0.62)', fontSize: 12 }}>Distributed compute for ZIP-based workloads</div>
            </div>
          </div>
          <div style={{ display: 'flex', gap: 10, flexWrap: 'wrap' }}>
            <a href="#benefits" style={secondaryButton}>看優點</a>
            <a href="#tutorial" style={secondaryButton}>看教學</a>
            <a href="#auth" style={primaryButton}>
              <span>立即開始</span>
              <span>→</span>
            </a>
          </div>
        </div>
      </section>

      <section style={{ padding: '34px 20px 66px' }}>
        <div style={{ maxWidth: 1240, margin: '0 auto', display: 'grid', gridTemplateColumns: '1.1fr 0.9fr', gap: 24, alignItems: 'stretch' }}>
          <div style={{ ...shellStyle, padding: 22, position: 'relative', overflow: 'hidden' }}>
            <div
              style={{
                position: 'absolute',
                inset: 0,
                background:
                  'radial-gradient(circle at 15% 15%, rgba(255, 211, 127, 0.18), transparent 20%), radial-gradient(circle at 80% 25%, rgba(255,255,255,0.09), transparent 18%)',
                pointerEvents: 'none',
              }}
            />
            <div style={{ position: 'relative', padding: '6px 6px 10px' }}>
              <div
                style={{
                  display: 'inline-flex',
                  padding: '7px 12px',
                  borderRadius: 999,
                  border: '1px solid rgba(255,255,255,0.12)',
                  background: 'rgba(255,255,255,0.04)',
                  color: '#f7d28a',
                  fontSize: 11,
                  letterSpacing: '0.2em',
                  textTransform: 'uppercase',
                  fontWeight: 700,
                }}
              >
                Official platform
              </div>
              <h1 style={{ margin: '18px 0 12px', fontSize: 'clamp(44px, 7vw, 84px)', lineHeight: 0.95, letterSpacing: '-0.04em' }}>
                高端分散式運算官網
              </h1>
              <p style={{ maxWidth: 760, fontSize: 18, lineHeight: 1.9, color: 'rgba(255,255,255,0.78)' }}>
                Hivemind 把 ZIP 任務上傳、分散式執行、資源定價、結果下載，整合成一個可部署、可追蹤、可擴展的工作流。
                你負責提交任務，系統負責自動作種、排程與執行。
              </p>

              <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap', marginTop: 22 }}>
                <a href="#auth" style={primaryButton}>
                  <span>建立帳戶</span>
                  <span>→</span>
                </a>
                <a href="#tutorial" style={secondaryButton}>
                  <span>快速教學</span>
                  <span>↘</span>
                </a>
              </div>

              <div style={{ display: 'grid', gridTemplateColumns: 'repeat(3, minmax(0, 1fr))', gap: 14, marginTop: 26 }}>
                <Stat value="ZIP" label="單檔上傳，系統自動作種" />
                <Stat value="CPU/GPU" label="分數化資源與定價模型" />
                <Stat value="Task" label="只看自己的任務與結果" />
              </div>
            </div>
          </div>

          <div id="auth" style={{ ...shellStyle, padding: 14 }}>
            <div style={{ ...innerStyle, padding: 18 }}>
              <div style={{ display: 'flex', gap: 8, marginBottom: 16 }}>
                <button
                  type="button"
                  onClick={() => setAuthMode('register')}
                  style={{
                    ...primaryButton,
                    border: authMode === 'register' ? '1px solid rgba(255,255,255,0.18)' : '1px solid rgba(255,255,255,0.08)',
                    flex: 1,
                    justifyContent: 'center',
                    background: authMode === 'register' ? 'linear-gradient(135deg, #e8c27d 0%, #c58d3a 100%)' : 'rgba(255,255,255,0.06)',
                    color: authMode === 'register' ? '#160f06' : '#fff',
                  }}
                >
                  註冊
                </button>
                <button
                  type="button"
                  onClick={() => setAuthMode('login')}
                  style={{
                    ...primaryButton,
                    border: authMode === 'login' ? '1px solid rgba(255,255,255,0.18)' : '1px solid rgba(255,255,255,0.08)',
                    flex: 1,
                    justifyContent: 'center',
                    background: authMode === 'login' ? 'linear-gradient(135deg, #e8c27d 0%, #c58d3a 100%)' : 'rgba(255,255,255,0.06)',
                    color: authMode === 'login' ? '#160f06' : '#fff',
                  }}
                >
                  登入
                </button>
              </div>

              <form onSubmit={handleAuthSubmit} style={{ display: 'grid', gap: 12 }}>
                <label style={{ display: 'grid', gap: 6 }}>
                  <span style={{ fontSize: 13, color: 'rgba(255,255,255,0.72)' }}>帳號</span>
                  <input value={username} onChange={(e) => setUsername(e.target.value)} style={fieldStyle} placeholder="yourname" />
                </label>
                <label style={{ display: 'grid', gap: 6 }}>
                  <span style={{ fontSize: 13, color: 'rgba(255,255,255,0.72)' }}>密碼</span>
                  <input
                    type="password"
                    value={password}
                    onChange={(e) => setPassword(e.target.value)}
                    style={fieldStyle}
                    placeholder="至少 8 碼"
                  />
                </label>
                {authMode === 'register' ? (
                  <label style={{ display: 'grid', gap: 6 }}>
                    <span style={{ fontSize: 13, color: 'rgba(255,255,255,0.72)' }}>確認密碼</span>
                    <input
                      type="password"
                      value={confirmPassword}
                      onChange={(e) => setConfirmPassword(e.target.value)}
                      style={fieldStyle}
                      placeholder="再次輸入密碼"
                    />
                  </label>
                ) : null}
                <button type="submit" disabled={loading} style={{ ...primaryButton, justifyContent: 'center', marginTop: 4 }}>
                  {loading ? '處理中...' : authMode === 'register' ? '建立並登入' : '登入'}
                </button>
              </form>

              <div
                style={{
                  marginTop: 14,
                  padding: 14,
                  borderRadius: 18,
                  background: 'rgba(255,255,255,0.06)',
                  border: '1px solid rgba(255,255,255,0.08)',
                  color: 'rgba(255,255,255,0.8)',
                  lineHeight: 1.7,
                }}
              >
                {status}
                {token ? (
                  <>
                    <div style={{ marginTop: 8, color: '#f7d28a' }}>已登入，帳戶餘額 {balance ?? 0} CPT</div>
                    <button
                      type="button"
                      onClick={logout}
                      style={{
                        marginTop: 12,
                        ...secondaryButton,
                        width: '100%',
                        justifyContent: 'center',
                        border: '1px solid rgba(255,255,255,0.1)',
                      }}
                    >
                      登出
                    </button>
                  </>
                ) : null}
              </div>
            </div>
          </div>
        </div>
      </section>

      <section id="benefits" style={{ padding: '40px 20px 72px', background: '#f6f1e8', color: '#1f160f' }}>
        <div style={{ maxWidth: 1240, margin: '0 auto' }}>
          <SectionTitle
            eyebrow="優點"
            title="為什麼這個平台值得部署"
            body="不是把表單放上去就叫產品。Hivemind 把任務、帳戶、節點、資源、定價與結果流整合成一個可持續運作的系統。"
          />
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(250px, 1fr))', gap: 18 }}>
            {benefitCards.map((card) => (
              <article key={card.title} style={{ ...sectionCard, padding: 20 }}>
                <div
                  style={{
                    width: 52,
                    height: 52,
                    borderRadius: 18,
                    background: 'linear-gradient(135deg, #1f160f 0%, #6f4a1d 100%)',
                    boxShadow: '0 16px 30px rgba(31, 22, 15, 0.12)',
                    marginBottom: 16,
                  }}
                />
                <h3 style={{ margin: '0 0 10px', fontSize: 22 }}>{card.title}</h3>
                <p style={{ margin: 0, color: '#5e5143', lineHeight: 1.8 }}>{card.body}</p>
              </article>
            ))}
          </div>
        </div>
      </section>

      <section id="tutorial" style={{ padding: '72px 20px', background: '#f3ede4', color: '#1f160f' }}>
        <div style={{ maxWidth: 1240, margin: '0 auto' }}>
          <SectionTitle
            eyebrow="教學"
            title="三分鐘上手"
            body="從註冊到下載結果，整個流程很短。官網把關鍵步驟寫清楚，讓第一次接觸的人也能直接跑通。"
          />
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(240px, 1fr))', gap: 18 }}>
            {tutorialSteps.map((item) => (
              <div key={item.step} style={{ ...sectionCard, padding: 20 }}>
                <div style={{ color: '#b06c1f', fontWeight: 800, letterSpacing: '0.18em', fontSize: 12 }}>{item.step}</div>
                <h3 style={{ margin: '10px 0 10px', fontSize: 24 }}>{item.title}</h3>
                <p style={{ margin: 0, color: '#5e5143', lineHeight: 1.8 }}>{item.body}</p>
              </div>
            ))}
          </div>
        </div>
      </section>

      <section style={{ padding: '72px 20px 96px', background: '#f6f1e8', color: '#1f160f' }}>
        <div style={{ maxWidth: 1240, margin: '0 auto', display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 20 }}>
          <div style={{ ...sectionCard, padding: 22 }}>
            <SectionTitle
              eyebrow="工作流"
              title="建立任務後會發生什麼"
              body="你上傳 ZIP 後，master 會自動轉成 torrent，排程器會選節點執行，完成後你就能查看進度與下載結果。"
            />
            <ol style={{ margin: 0, paddingLeft: 18, color: '#5e5143', lineHeight: 2 }}>
              <li>建立帳戶並登入。</li>
              <li>上傳 ZIP，填寫任務需求與上限。</li>
              <li>系統自動作種並開始排程。</li>
              <li>完成後查看日誌、結果與 artifact。</li>
            </ol>
          </div>
          <div style={{ ...sectionCard, padding: 22, background: '#1b130d', color: '#f8f4ed' }}>
            <SectionTitle
              eyebrow="部署"
              title="同一套平台，分成兩個角色介面"
              body="Master UI 與 Worker UI 分工清楚，官方首頁則負責導覽、註冊、登入與教學，減少使用者第一次接觸的成本。"
            />
            <div style={{ display: 'grid', gap: 12 }}>
              <div style={{ padding: 16, borderRadius: 18, background: 'rgba(255,255,255,0.04)', border: '1px solid rgba(255,255,255,0.08)' }}>
                官網: 註冊、登入、教學
              </div>
              <div style={{ padding: 16, borderRadius: 18, background: 'rgba(255,255,255,0.04)', border: '1px solid rgba(255,255,255,0.08)' }}>
                Master: ZIP 任務、進度、下載結果
              </div>
              <div style={{ padding: 16, borderRadius: 18, background: 'rgba(255,255,255,0.04)', border: '1px solid rgba(255,255,255,0.08)' }}>
                Worker: 本機註冊、資源顯示、節點回報
              </div>
            </div>
          </div>
        </div>
      </section>

      <section style={{ padding: '0 20px 56px', background: '#f6f1e8' }}>
        <div style={{ maxWidth: 1240, margin: '0 auto', ...shellStyle, padding: 20, color: '#fff' }}>
          <div style={{ display: 'grid', gridTemplateColumns: '1.2fr 0.8fr', gap: 16, alignItems: 'center' }}>
            <div>
              <div style={{ color: '#f7d28a', letterSpacing: '0.2em', textTransform: 'uppercase', fontSize: 11, fontWeight: 700 }}>
                Ready to deploy
              </div>
              <h2 style={{ margin: '12px 0 10px', fontSize: 'clamp(30px, 4vw, 54px)', lineHeight: 1.04 }}>
                讓你的工作流變成一個可被使用、可被理解、可被持續擴充的產品。
              </h2>
              <p style={{ margin: 0, color: 'rgba(255,255,255,0.74)', lineHeight: 1.8 }}>
                註冊後登入，看看官網、教學和兩個角色介面如何協同工作。之後可建立自己的帳戶來試跑完整流程。
              </p>
            </div>
            <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 12, flexWrap: 'wrap' }}>
              <a href="#auth" style={primaryButton}>
                <span>註冊帳戶</span>
                <span>→</span>
              </a>
              <a href="#benefits" style={secondaryButton}>
                <span>了解優點</span>
                <span>↘</span>
              </a>
            </div>
          </div>
        </div>
      </section>

      <div
        style={{
          position: 'fixed',
          inset: 0,
          pointerEvents: 'none',
          background:
            'linear-gradient(180deg, rgba(255,255,255,0.02), rgba(255,255,255,0.00)), radial-gradient(circle at 50% 50%, rgba(255,255,255,0.02), transparent 55%)',
          mixBlendMode: 'screen',
        }}
      />

      <div
        style={{
          position: 'fixed',
          inset: 0,
          pointerEvents: 'none',
          opacity: 0.05,
          backgroundImage:
            'linear-gradient(rgba(255,255,255,0.3) 1px, transparent 1px), linear-gradient(90deg, rgba(255,255,255,0.3) 1px, transparent 1px)',
          backgroundSize: '48px 48px',
        }}
      />

      <div
        style={{
          position: 'fixed',
          inset: 0,
          pointerEvents: 'none',
          opacity: reveal ? 1 : 0,
          transition: 'opacity 900ms cubic-bezier(0.32, 0.72, 0, 1)',
        }}
      />
    </main>
  );
}
