// 國際化翻譯字典
const i18n = {
    "zh-tw": {
        "home": "主頁",
        "login": "登入",
        "register": "註冊",
        "download": "下載",
        "balance": "餘額查詢與轉帳",
        "sponsor": "贊助列表",
        "terms": "用戶條款",
        "privacy": "隱私權政策",
        "github": "GitHub",
        "community": "社區",
        "logout": "登出",
        "view_balance": "查看餘額",
        "switch_lang": "切換語言",
        "welcome": "解鎖分散式運算的力量",
        "subtitle": "工作端 + 節點池，打造高效能分布式平台，讓你專注開發、我們處理執行。",
        "feature1_title": "模組化架構",
        "feature1_desc": "將工作拆解為可分配的子任務，自由擴展你的運算能力。",
        "feature2_title": "即時節點分配", 
        "feature2_desc": "節點池自動分配最合適的節點，支援自定義任務標準。",
        "feature3_title": "安全與隔離",
        "feature3_desc": "任務在 Docker 容器中執行，保證節點之間彼此獨立、安全。",
        "about_title": "關於我們",
        "about_desc": "我們是一群相信「運算力應人人可用」的開發者，打造這套平台，目標是讓任何人都能輕鬆取得分散式資源，不再受限本機硬體。",
        "footer_copyright": "© 2025 Hivemind Project. Built with love by Justin.",
        // 高性能處理
        "high_performance_title": "高性能處理",
        "high_performance_desc": "充分利用分布式架構，提供前所未有的運算效能和可擴展性。",
        "better_computing_experience": "打造更好的運算體驗",
        "ready_to_start": "準備開始了嗎？",
        "register_now": "立即註冊",
        "team_info": "HiveMind 開發團隊 / 分布式運算專家",
        // 登入頁面
        "login_title": "歡迎回來",
        "login_subtitle": "登入您的 HiveMind 帳戶",
        "login_username_label": "用戶名",
        "login_username_placeholder": "請輸入用戶名",
        "login_password_label": "密碼",
        "login_password_placeholder": "請輸入密碼",
        "login_captcha_label": "人機驗證",
        "login_btn": "登入",
        "login_no_account": "還沒有帳戶？",
        "login_register_link": "立即註冊",
        // 註冊頁面
        "register_title": "註冊帳號",
        "register_subtitle": "創建您的 HiveMind 帳戶",
        "register_username_label": "用戶名",
        "register_username_placeholder": "請輸入用戶名",
        "register_password_label": "密碼",
        "register_password_placeholder": "請輸入密碼",
        "register_confirm_label": "確認密碼",
        "register_confirm_placeholder": "請再次輸入密碼",
        "register_captcha_label": "人機驗證",
        "register_btn": "創建帳戶",
        "register_already_account": "已經有帳戶了？",
        "register_login_link": "立即登入",
        // 下載頁面
        "download_title": "下載 HiveMind",
        "download_subtitle": "選擇適合您的客戶端開始使用分布式運算平台",
        "worker_title": "工作端",
        "worker_description": "將您的閒置資源貢獻給網路，獲得報酬",
        "master_title": "主控端",
        "master_description": "部署主控伺服器，管理分布式運算任務",
        "download_windows": "下載 Windows 版本",
        "download_linux": "下載 Linux 版本",
        "popular": "熱門",
        "enterprise": "企業級",
        "coming_soon": "即將推出",
        "web_version": "Web 版本",
        "web_version_desc": "瀏覽器直接運行，無需安裝",
        "mobile_version": "移動版本",
        "mobile_version_desc": "Android 和 iOS 應用程式",
        "cloud_version": "雲端版本",
        "cloud_version_desc": "一鍵部署到雲端平台",
        // 餘額頁面
        "balance_title": "餘額查詢與轉帳",
        "balance_my_balance": "我的餘額",
        "balance_transfer": "轉帳",
        "balance_receiver_label": "收款人用戶名",
        "balance_amount_label": "金額",
        "balance_transfer_btn": "轉帳",
        // 贊助頁面
        "sponsor_thanks": "感謝以下贊助者對 HiveMind 的支持！",
        "sponsor_main_developer": "主要開發者",
        // 條款頁面
        "terms_title": "使用條款",
        "terms_service_agreement": "服務條款同意",
        "terms_prohibited_behavior": "禁止行為",
        "terms_service_changes": "服務變更",
        "terms_violation_handling": "違規處理",
        "terms_agreement": "使用本平台即表示您同意遵守相關法律法規及本條款內容。",
        "terms_illegal": "不得利用本平台進行任何非法、侵權或損害他人權益之行為。",
        "terms_changes": "平台有權隨時調整服務內容、條款及政策，重大變更將公告通知。",
        "terms_violation": "如有違反條款，平台有權暫停或終止您的使用權限。",
        "terms_contact": "如有疑問，請聯絡我們：",
        // 隱私政策頁面
        "privacy_title": "隱私權政策",
        "privacy_welcome": "歡迎使用 HiveMind 分布式運算平台（以下簡稱「本平台」）。我們非常重視您的個人資料保護，請詳細閱讀本政策：",
        "privacy_collection_title": "1. 資料蒐集",
        "privacy_collection_content": "本平台僅於註冊、登入、使用服務時，蒐集必要的帳號、Email、登入紀錄等資訊。",
        "privacy_collection_note": "我們不會主動蒐集您的敏感個資。",
        "privacy_cookie_title": "2. Cookie 與追蹤",
        "privacy_cookie_content": "僅用於登入狀態維護與平台安全，不做廣告追蹤。",
        "privacy_usage_title": "3. 資料用途",
        "privacy_usage_content": "僅用於帳號管理、服務提供、平台安全與法令遵循。",
        "privacy_third_party_title": "4. 第三方服務",
        "privacy_third_party_content": "本平台可能串接第三方登入、金流等服務，僅於必要時傳遞最少資訊。",
        "privacy_security_title": "5. 資料安全",
        "privacy_security_content": "我們採用加密、權限控管等措施，保障您的資料安全。",
        "privacy_rights_title": "6. 您的權利",
        "privacy_rights_content": "您可隨時查詢、更正、刪除您的個人資料，或聯絡我們行使權利。",
        "privacy_contact_title": "7. 聯絡方式",
        "privacy_contact_content": "如有任何隱私疑慮，請來信 hivemind@justin0711.com",
        "privacy_revision": "本政策如有修訂，將公告於本頁面。"
    },
    "en": {
        "home": "Home",
        "login": "Login",
        "register": "Register", 
        "download": "Download",
        "balance": "Balance & Transfer",
        "sponsor": "Sponsors",
        "terms": "Terms",
        "privacy": "Privacy Policy",
        "github": "GitHub",
        "community": "Community",
        "logout": "Logout",
        "view_balance": "View Balance",
        "switch_lang": "Switch Language",
        "welcome": "Unlock the Power of Distributed Computing",
        "subtitle": "Worker + Node Pool, build a high-performance distributed platform. You focus on development, we handle the execution.",
        "feature1_title": "Modular Architecture",
        "feature1_desc": "Break down tasks into manageable subtasks and scale your computing power freely.",
        "feature2_title": "Instant Node Allocation",
        "feature2_desc": "The node pool automatically allocates the most suitable nodes, supporting custom task criteria.",
        "feature3_title": "Security and Isolation",
        "feature3_desc": "Tasks run in Docker containers, ensuring independence and security between nodes.",
        "about_title": "About Us",
        "about_desc": "We are a group of developers who believe that 'computing power should be accessible to all'. We created this platform to enable anyone to easily access distributed resources, no longer limited by local hardware.",
        "footer_copyright": "© 2025 Hivemind Project. Built with love by Justin.",
        // High Performance
        "high_performance_title": "High Performance Processing",
        "high_performance_desc": "Leverage distributed architecture for unprecedented computing power and scalability.",
        "better_computing_experience": "Better Computing Experience",
        "ready_to_start": "Ready to get started?",
        "register_now": "Register Now",
        "team_info": "HiveMind Development Team / Distributed Computing Experts",
        // 登入頁面
        "login_title": "Welcome Back",
        "login_subtitle": "Sign in to your HiveMind account",
        "login_username_label": "Username",
        "login_username_placeholder": "Please enter username",
        "login_password_label": "Password",
        "login_password_placeholder": "Please enter password",
        "login_captcha_label": "Human Verification",
        "login_btn": "Login",
        "login_no_account": "Don't have an account?",
        "login_register_link": "Sign up now",
        // 註冊頁面
        "register_title": "Register Account",
        "register_subtitle": "Create your HiveMind account",
        "register_username_label": "Username",
        "register_username_placeholder": "Please enter username",
        "register_password_label": "Password",
        "register_password_placeholder": "Please enter password",
        "register_confirm_label": "Confirm Password",
        "register_confirm_placeholder": "Please enter password again",
        "register_captcha_label": "Human Verification",
        "register_btn": "Create Account",
        "register_already_account": "Already have an account?",
        "register_login_link": "Sign in now",
        // 下載頁面
        "download_title": "Download HiveMind",
        "download_subtitle": "Choose the client that suits your needs to start using the distributed computing platform",
        "worker_title": "Worker Client",
        "worker_description": "Contribute your idle resources to the network and earn rewards",
        "master_title": "Master Client",
        "master_description": "Deploy master server to manage distributed computing tasks",
        "download_windows": "Download Windows Version",
        "download_linux": "Download Linux Version",
        "popular": "Popular",
        "enterprise": "Enterprise",
        "coming_soon": "Coming Soon",
        "web_version": "Web Version",
        "web_version_desc": "Run directly in browser, no installation required",
        "mobile_version": "Mobile Version",
        "mobile_version_desc": "Android and iOS applications",
        "cloud_version": "Cloud Version",
        "cloud_version_desc": "One-click deployment to cloud platforms",
        // 餘額頁面
        "balance_title": "Balance & Transfer",
        "balance_my_balance": "My Balance",
        "balance_transfer": "Transfer",
        "balance_receiver_label": "Recipient Username",
        "balance_amount_label": "Amount",
        "balance_transfer_btn": "Transfer",
        // 贊助頁面
        "sponsor_thanks": "Thank you to the following sponsors for supporting HiveMind!",
        "sponsor_main_developer": "Lead Developer",
        // 條款頁面
        "terms_title": "Terms of Service",
        "terms_service_agreement": "Service Agreement",
        "terms_prohibited_behavior": "Prohibited Behavior",
        "terms_service_changes": "Service Changes",
        "terms_violation_handling": "Violation Handling",
        "terms_agreement": "By using this platform, you agree to comply with relevant laws and regulations and the terms of this agreement.",
        "terms_illegal": "You may not use this platform for any illegal, infringing, or harmful activities that damage the rights of others.",
        "terms_changes": "The platform reserves the right to adjust service content, terms, and policies at any time. Major changes will be announced.",
        "terms_violation": "If you violate the terms, the platform has the right to suspend or terminate your usage rights.",
        "terms_contact": "If you have any questions, please contact us:",
        // 隱私政策頁面
        "privacy_title": "Privacy Policy",
        "privacy_welcome": "Welcome to the HiveMind distributed computing platform (hereinafter referred to as 'this platform'). We take your personal data protection very seriously. Please read this policy carefully:",
        "privacy_collection_title": "1. Data Collection",
        "privacy_collection_content": "This platform only collects necessary account, email, and login record information when registering, logging in, and using services.",
        "privacy_collection_note": "We will not actively collect your sensitive personal data.",
        "privacy_cookie_title": "2. Cookies and Tracking",
        "privacy_cookie_content": "Only used for login status maintenance and platform security, no advertising tracking.",
        "privacy_usage_title": "3. Data Usage",
        "privacy_usage_content": "Only used for account management, service provision, platform security, and legal compliance.",
        "privacy_third_party_title": "4. Third-party Services",
        "privacy_third_party_content": "This platform may integrate third-party login, payment, and other services, only passing minimal information when necessary.",
        "privacy_security_title": "5. Data Security",
        "privacy_security_content": "We adopt encryption, access control, and other measures to protect your data security.",
        "privacy_rights_title": "6. Your Rights",
        "privacy_rights_content": "You can query, correct, and delete your personal data at any time, or contact us to exercise your rights.",
        "privacy_contact_title": "7. Contact Information",
        "privacy_contact_content": "If you have any privacy concerns, please email hivemind@justin0711.com",
        "privacy_revision": "If this policy is revised, it will be announced on this page."
    }
};

// 國際化函數
function getLang() {
    const savedLang = localStorage.getItem('lang');
    if (savedLang === 'en') {
        return 'en';
    }
    return 'zh-tw';
}

function setLang(lang) {
    localStorage.setItem('lang', lang);
    document.querySelectorAll('[data-i18n]').forEach(el => {
        const key = el.getAttribute('data-i18n');
        const translation = i18n[lang][key];
        if (translation) {
            el.textContent = translation;
        }
    });
    
    document.querySelectorAll('[data-i18n-placeholder]').forEach(el => {
        const key = el.getAttribute('data-i18n-placeholder');
        const translation = i18n[lang][key];
        if (translation) {
            el.placeholder = translation;
        }
    });
}

function toggleLang() {
    const lang = getLang() === 'zh-tw' ? 'en' : 'zh-tw';
    setLang(lang);
}

// 頁面載入時自動設置語言
document.addEventListener('DOMContentLoaded', () => {
    setLang(getLang());
});
