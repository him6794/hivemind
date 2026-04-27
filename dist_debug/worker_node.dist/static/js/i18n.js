// Simple client-side i18n with localStorage persistence
(function(){
  const DEFAULT_LANG = localStorage.getItem('lang') || (navigator.language && navigator.language.toLowerCase().startsWith('zh') ? 'zh-tw' : 'en');

  const dict = {
    'en': {
      // generic
      'app.title': 'Hivemind Worker',
      'nav.back_login': 'Back to Login',
      'nav.logout': 'Logout',
      'nav.username': 'User',
      'lang.en': 'English',
      'lang.zh': '中文',

      // index/settings page
      'header.settings': 'HiveMind Worker Settings',
      'section.limits': 'Resource Limits',
  // performance overview
  'perf.title': 'Performance Overview',
  'perf.cpu': 'CPU Score',
  'perf.mem': 'Available Memory (GB)',
  'perf.gpu': 'GPU Score',
  'perf.gpumem': 'GPU Memory (GB)',
  'perf.ad.cpu': 'Advertised CPU',
  'perf.ad.mem': 'Advertised Memory (GB)',
  'perf.ad.gpu': 'Advertised GPU',
  'perf.ad.gpumem': 'Advertised GPU Memory (GB)',
      'label.cpu': 'CPU',
      'label.ram': 'Memory',
      'label.gpu': 'GPU',
      'label.gpu_mem': 'GPU Memory',
      'label.disk': 'Disk',
      'label.network': 'Network',
      'limit.cpu': 'CPU Limit: {v}%',
      'limit.ram': 'RAM Limit: {v}%',
      'limit.gpu': 'GPU Limit: {v}%',
      'limit.gpu_mem': 'GPU Memory Limit: {v}%',
      'limit.disk': 'Disk Limit: {v}%',
      'limit.network': 'Network Limit: {v} Mbps',
      'btn.save': 'Save Settings',
      'save.success': 'Settings saved. Limits will be applied on register or heartbeat.',
      'save.fail': 'Save failed: {msg}',
      'save.load_fail': 'Failed to load settings: {msg}',
      'section.help': 'Help',
      'help.1': 'These limits affect the resource values reported during registration and heartbeat. Master/Node Pool will only see the limited capacity.',
      'help.2': 'CPU/GPU are limited by percentage.',
      'help.3': 'Memory is limited by percentage, at least 1GB.',
      'help.4': 'Disk/Network are currently stored locally and not yet part of registration.',

      // login page
      'login.title': 'Worker Node Login',
      'login.subtitle': 'Connect to HiveMind Distributed Computing Network',
      'login.node_status': 'Node Status: {status}',
      'login.username': 'Username',
      'login.username_ph': 'Enter your username',
      'login.password': 'Password',
      'login.password_ph': 'Enter your password',
      'login.submit': 'Login & Register Node',
      'login.footer': 'After login, this device will be registered as a worker node automatically.',
      'login.location_hint': 'To change location settings, please restart the node program.',

      // monitor page
      'monitor.header': 'Worker Node Monitoring Panel',
      'monitor.node_status': 'Node Status',
      'monitor.node_id': 'Node ID',
      'monitor.status': 'Status',
      'monitor.current_task': 'Current Task',
      'monitor.cpt': 'CPT Balance',
      'monitor.ip': 'Local IP',
      'monitor.cpu': 'CPU Usage',
      'monitor.mem': 'Memory Usage',
      'monitor.resources': 'System Resources',
      'monitor.logs': 'System Logs',
      'monitor.refresh_logs': 'Refresh Logs',
      'monitor.loading_logs': 'Loading logs...',
      'monitor.error_loading_logs': 'Error loading logs: {msg}',
      'monitor.no_logs': 'Currently no log records',
      'monitor.invalid_logs': 'Did not receive valid log data',
      'monitor.error_loading_status': 'Error loading status',
      'monitor.connection_error': 'Connection Error',
      'monitor.tasks_running': '{n} tasks running',
      'monitor.no_tasks': 'no tasks',
      'monitor.main': 'Main: {id}',
      'chart.time': 'Time',
      'chart.cpu': 'CPU Usage (%)',
      'chart.memory': 'Memory Usage (%)'
    },
    'zh-tw': {
      // generic
      'app.title': 'Hivemind Worker',
      'nav.back_login': '返回登入',
      'nav.logout': '登出',
      'nav.username': '使用者',
      'lang.en': 'English',
      'lang.zh': '中文',

      // index/settings page
      'header.settings': 'HiveMind Worker 設定',
      'section.limits': '效能上限',
  // performance overview
  'perf.title': '效能概覽',
  'perf.cpu': 'CPU 分數',
  'perf.mem': '可用記憶體(GB)',
  'perf.gpu': 'GPU 分數',
  'perf.gpumem': 'GPU 記憶體(GB)',
  'perf.ad.cpu': '廣告 CPU',
  'perf.ad.mem': '廣告記憶體(GB)',
  'perf.ad.gpu': '廣告 GPU',
  'perf.ad.gpumem': '廣告 GPU 記憶體(GB)',
      'label.cpu': 'CPU',
      'label.ram': '記憶體',
      'label.gpu': 'GPU',
      'label.gpu_mem': 'GPU 記憶體',
      'label.disk': '磁碟',
      'label.network': '網路',
      'limit.cpu': 'CPU 上限: {v}%',
      'limit.ram': '記憶體上限: {v}%',
      'limit.gpu': 'GPU 上限: {v}%',
      'limit.gpu_mem': 'GPU 記憶體上限: {v}%',
      'limit.disk': '磁碟上限: {v}%',
      'limit.network': '網路上限: {v} Mbps',
      'btn.save': '保存設定',
      'save.success': '已保存設定，註冊或心跳時將採用上限。',
      'save.fail': '保存失敗: {msg}',
      'save.load_fail': '讀取設定失敗: {msg}',
      'section.help': '說明',
      'help.1': '這些上限會影響註冊與心跳刷新時對外回報的資源數值，Master/Node Pool 將只看到限制後的能力。',
      'help.2': 'CPU/GPU 以百分比限制分數。',
      'help.3': '記憶體以百分比計算，至少 1GB。',
      'help.4': '磁碟/網路目前僅本地保存，暫不參與註冊。',

      // login page
      'login.title': '節點登入',
      'login.subtitle': '連線到 HiveMind 分散式運算網路',
      'login.node_status': '節點狀態: {status}',
      'login.username': '使用者名稱',
      'login.username_ph': '請輸入使用者名稱',
      'login.password': '密碼',
      'login.password_ph': '請輸入密碼',
      'login.submit': '登入並註冊節點',
      'login.footer': '登入後，此裝置將自動註冊為工作節點。',
      'login.location_hint': '若要變更位置設定，請重新啟動節點程式。',

      // monitor page
      'monitor.header': '工作節點監控面板',
      'monitor.node_status': '節點狀態',
      'monitor.node_id': '節點 ID',
      'monitor.status': '狀態',
      'monitor.current_task': '目前任務',
      'monitor.cpt': 'CPT 餘額',
      'monitor.ip': '本機 IP',
      'monitor.cpu': 'CPU 使用率',
      'monitor.mem': '記憶體使用率',
      'monitor.resources': '系統資源',
      'monitor.logs': '系統日誌',
      'monitor.refresh_logs': '重新整理日誌',
      'monitor.loading_logs': '載入日誌中...',
      'monitor.error_loading_logs': '日誌載入失敗: {msg}',
      'monitor.no_logs': '目前沒有日誌紀錄',
      'monitor.invalid_logs': '未收到有效的日誌資料',
      'monitor.error_loading_status': '狀態載入失敗',
      'monitor.connection_error': '連線錯誤',
      'monitor.tasks_running': '執行中任務 {n} 個',
      'monitor.no_tasks': '沒有任務',
      'monitor.main': '主任務: {id}',
      'chart.time': '時間',
      'chart.cpu': 'CPU 使用率 (%)',
      'chart.memory': '記憶體使用率 (%)'
    }
  };

  function format(str, params){
    return str.replace(/\{(\w+)\}/g, (_, k) => (params && (k in params)) ? params[k] : `{${k}}`);
  }

  function t(key, params){
    const lang = i18n.lang;
    const table = dict[lang] || dict['en'];
    const fallback = dict['en'];
    const s = (table && table[key]) || (fallback && fallback[key]) || key;
    return params ? format(s, params) : s;
  }

  function applyTranslations(root){
    const el = root || document;
    el.querySelectorAll('[data-i18n]').forEach(node => {
      const key = node.getAttribute('data-i18n');
      if(!key) return;
      const attr = node.getAttribute('data-i18n-attr');
      const text = t(key);
      if(attr){
        node.setAttribute(attr, text);
      }else{
        node.textContent = text;
      }
    });

    // placeholders
    el.querySelectorAll('[data-i18n-ph]').forEach(node => {
      const key = node.getAttribute('data-i18n-ph');
      if(key) node.setAttribute('placeholder', t(key));
    });
  }

  function setLanguage(lang){
    const newLang = (lang || 'en').toLowerCase();
    // 先寫入 localStorage，因為 i18n.lang 的 getter 會回讀 localStorage
    localStorage.setItem('lang', newLang);
    document.documentElement.lang = newLang;
    applyTranslations();
    const selector = document.getElementById('lang-select');
    if(selector){ selector.value = newLang; }
    // 通知應用其他部分語言已變更
    try{
      const evt = new CustomEvent('i18n:languageChanged', { detail: { lang: newLang }});
      document.dispatchEvent(evt);
    }catch(_){}
  }

  function ensureLangOptions(select){
    if(!select) return;
    const current = select.value;
    select.innerHTML = '';
    const optEn = document.createElement('option');
    optEn.value = 'en'; optEn.textContent = dict['en']['lang.en'];
    const optZh = document.createElement('option');
    optZh.value = 'zh-tw'; optZh.textContent = dict['en']['lang.zh'];
    select.appendChild(optEn);
    select.appendChild(optZh);
    if(current) select.value = current;
  }

  function init(){
    document.addEventListener('DOMContentLoaded', () => {
      // insert language switcher if not present
      const hasSwitcher = document.querySelector('.lang-switcher');
      if(!hasSwitcher){
        const headerRight = document.querySelector('.header .header-right') || document.body;
        const wrap = document.createElement('div');
        wrap.className = 'lang-switcher';
        wrap.innerHTML = `<select id="lang-select" aria-label="Language"></select>`;
        headerRight.appendChild(wrap);
      }

      const selector = document.getElementById('lang-select');
      ensureLangOptions(selector);
      if(selector){
        selector.addEventListener('change', (e)=> setLanguage(e.target.value));
      }

      setLanguage(DEFAULT_LANG);
      // 當語言切換時，順帶更新動態標籤文字（例如狀態訊息等），靠 applyTranslations 已涵蓋
    });
  }

  window.i18n = { init, t, setLanguage, get lang(){ return localStorage.getItem('lang') || DEFAULT_LANG; } };
})();

i18n.init();
