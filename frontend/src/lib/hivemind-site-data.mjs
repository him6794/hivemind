const baseRoutes = {
  en: [
    { id: 'home', label: 'Overview' },
    { id: 'login', label: 'Sign in' },
    { id: 'register', label: 'Create account' },
    { id: 'account', label: 'Account' },
    { id: 'security', label: 'Trust' },
    { id: 'docs', label: 'Docs' },
  ],
  zh: [
    { id: 'home', label: '總覽' },
    { id: 'login', label: '登入' },
    { id: 'register', label: '建立帳號' },
    { id: 'account', label: '帳號中心' },
    { id: 'security', label: '信任與安全' },
    { id: 'docs', label: '文件' },
  ],
};

const definitions = {
  en: {
    brand: {
      name: 'Hivemind',
      strap: 'A public front door for distributed compute and a calm account center.',
    },
    routes: baseRoutes.en,
    hero: {
      badge: 'Official site',
      title: 'Distributed compute with a clean account layer.',
      body: 'Hivemind explains the network, helps people create an account, check balance, and follow the right path to deploy their own Master or Worker node.',
      primaryCta: 'Create account',
      secondaryCta: 'Deployment guide',
      bullets: [
        'Public product overview',
        'Account balance and access',
        'Deployment guidance for Master and Worker',
      ],
    },
    sections: {
      stats: [
        { value: 'Explain', label: 'Show what Hivemind is and where each surface belongs' },
        { value: 'Access', label: 'Register, sign in, and keep account state in one place' },
        { value: 'Balance', label: 'Review CPT balance without exposing operational consoles' },
        { value: 'Deploy', label: 'Send people to the right local node for real operations' },
      ],
      features: [
        {
          title: 'Public-facing product narrative',
          body: 'The homepage stays focused on product value, not internal services or operator-only controls.',
        },
        {
          title: 'Account center',
          body: 'Signed-in users can confirm identity, read balance, and find billing or transfer follow-up paths.',
        },
        {
          title: 'Deployment handoff',
          body: 'Master and Worker setup lives in guided docs so operational control stays on user-deployed surfaces.',
        },
        {
          title: 'Clear trust boundaries',
          body: 'The official site remains separate from task submission, worker registration, and local execution control.',
        },
      ],
      workflow: [
        {
          step: '01',
          title: 'Create an account',
          body: 'Register on the official site and keep authentication in one consistent place.',
        },
        {
          step: '02',
          title: 'Review your account state',
          body: 'Check balance, security guidance, and the next steps available from the official product surface.',
        },
        {
          step: '03',
          title: 'Deploy the right node',
          body: 'Use the docs to stand up a local Master node for task operations or a local Worker node for compute supply.',
        },
        {
          step: '04',
          title: 'Operate locally',
          body: 'Task submission and worker control happen on the user-deployed node surfaces, not on the official website.',
        },
      ],
      security: [
        'Browser traffic goes to same-origin website backend endpoints.',
        'The public site does not expose task or worker operations as account-center features.',
        'Deployment guidance distinguishes official infrastructure from user-deployed nodes.',
        'Account access stays separate from local execution and worker administration.',
      ],
      docs: [
        { method: 'POST', path: '/api/register', note: 'Create an official Hivemind account' },
        { method: 'POST', path: '/api/login', note: 'Authenticate through the website backend' },
        { method: 'GET', path: '/api/balance', note: 'Read current CPT balance for the signed-in account' },
      ],
      account: {
        summary: 'Use the official site for identity, balance visibility, and links to billing or deployment follow-up.',
        panels: [
          {
            title: 'Balance visibility',
            body: 'Read the current CPT balance through the website backend without opening task or worker consoles.',
          },
          {
            title: 'Billing follow-up',
            body: 'Use the documented account and payment flows exposed by the official product surface when available.',
          },
          {
            title: 'Deployment next steps',
            body: 'Open the docs to deploy a Master node for workloads or a Worker node for compute supply.',
          },
        ],
      },
    },
  },
  zh: {
    brand: {
      name: 'Hivemind',
      strap: '分散式算力的官方入口，也是保持邊界清楚的帳號中心。',
    },
    routes: baseRoutes.zh,
    hero: {
      badge: '官方網站',
      title: '把分散式算力說清楚，也把帳號邊界守清楚。',
      body: 'Hivemind 官方站負責產品介紹、帳號建立、登入、餘額查詢，以及引導使用者部署自己的 Master 或 Worker 節點。',
      primaryCta: '建立帳號',
      secondaryCta: '部署指南',
      bullets: [
        '公開產品說明',
        '帳號與餘額管理',
        'Master 與 Worker 部署指引',
      ],
    },
    sections: {
      stats: [
        { value: '說明', label: '清楚區分官方網站與使用者自架節點的責任' },
        { value: '登入', label: '把註冊、登入與帳號狀態留在同一個官方入口' },
        { value: '餘額', label: '查詢 CPT 餘額，不把官方站變成操作台' },
        { value: '部署', label: '把真正的操作行為導向正確的本地節點介面' },
      ],
      features: [
        {
          title: '公開產品首頁',
          body: '首頁專注在產品價值與使用方式，不混入內部服務名稱或營運控制功能。',
        },
        {
          title: '帳號中心',
          body: '已登入使用者可以確認身份、查看餘額，並找到付款或轉帳等後續入口。',
        },
        {
          title: '部署交接',
          body: 'Master 與 Worker 的安裝與部署說明放在文件頁，讓操作責任回到使用者自架節點。',
        },
        {
          title: '清楚的信任邊界',
          body: '官方網站不負責送出工作、註冊節點或控制本地執行環境。',
        },
      ],
      workflow: [
        {
          step: '01',
          title: '建立帳號',
          body: '先在官方網站完成註冊與登入，把身份與帳號流程集中在同一個入口。',
        },
        {
          step: '02',
          title: '查看帳號狀態',
          body: '確認 CPT 餘額、安全資訊，以及官方網站可以提供的後續動作。',
        },
        {
          step: '03',
          title: '部署正確節點',
          body: '若要提交工作，部署 Master 節點；若要提供算力，部署 Worker 節點。',
        },
        {
          step: '04',
          title: '在本地節點操作',
          body: '工作提交、進度控制與節點管理都發生在使用者自架的節點介面，而不是官方網站。',
        },
      ],
      security: [
        '瀏覽器只連到同源的網站後端端點。',
        '官方站的帳號中心不暴露工作或 worker 的操作能力。',
        '部署文件會清楚區分官方基礎設施與使用者自架節點。',
        '帳號存取與本地執行、worker 管理維持分離。',
      ],
      docs: [
        { method: 'POST', path: '/api/register', note: '建立 Hivemind 官方帳號' },
        { method: 'POST', path: '/api/login', note: '透過網站後端完成登入' },
        { method: 'GET', path: '/api/balance', note: '讀取已登入帳號目前的 CPT 餘額' },
      ],
      account: {
        summary: '官方網站只負責身份、餘額可見性，以及導向帳務與部署的下一步。',
        panels: [
          {
            title: '餘額可見性',
            body: '透過網站後端讀取目前 CPT 餘額，不把工作或節點控制混進帳號中心。',
          },
          {
            title: '帳務後續',
            body: '當官方產品提供付款、轉帳或其他帳務流程時，入口會留在官方站這一層。',
          },
          {
            title: '部署下一步',
            body: '需要提交工作就部署 Master，需要提供算力就部署 Worker，兩者都從文件頁開始。',
          },
        ],
      },
    },
  },
};

export function normalizeLocale(value) {
  return String(value || '').toLowerCase().startsWith('zh') ? 'zh' : 'en';
}

export function getSiteDefinition(locale) {
  return definitions[normalizeLocale(locale)];
}
