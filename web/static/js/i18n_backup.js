// 國際化翻譯字典 
const i18n = {
    "zh-tw": {
        // 導航欄
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
        
        // 首頁內容
        "welcome": "解鎖分散式運算的力量",
        "subtitle": "工作端 + 節點池，打造高效能分布式平台，讓你專注開發、我們處理執行。",
        "system_resources": "系統資源",
        "resource_subtitle": "我們的分布式網路提供強大的運算資源",
        "total_ram": "總記憶體",
        "total_vram": "顯示記憶體", 
        "cpu_cores": "CPU 核心",
        "uptime": "可用時間",
        "active_nodes": "活躍節點",
        "completed_tasks": "完成任務",
        "response_time": "響應時間 (小時)",
        "better_computing_experience": "打造更好的運算體驗",
        "features_intro": "我們提供全方位的分布式運算解決方案，讓您的專案擁有無限可能。",
        "feature1_title": "模組化架構",
        "feature1_desc": "將工作拆解為可分配的子任務，自由擴展你的運算能力。",
        "feature2_title": "即時節點分配",
        "feature2_desc": "節點池自動分配最合適的節點，支援自定義任務標準。",
        "feature3_title": "安全與隔離",
        "feature3_desc": "任務在 Docker 容器中執行，保證節點之間彼此獨立、安全。",
        "high_performance_title": "高性能處理",
        "high_performance_desc": "充分利用分布式架構，提供前所未有的運算效能和可擴展性。",
        "how_it_works": "運作流程",
        "workflow_intro": "簡單三步驟，開始您的分布式運算之旅",
        "step1_title": "註冊帳戶",
        "step1_desc": "創建您的 HiveMind 帳戶，開始享受分布式運算服務。",
        "step2_title": "下載客戶端",
        "step2_desc": "選擇適合的客戶端版本，快速部署到您的環境中。",
        "step3_title": "查看文檔",
        "step3_desc": "閱讀完整的使用指南和 API 文檔，快速上手。",
        "tech_advantages": "技術優勢",
        "advantage1_title": "高效並行",
        "advantage1_desc": "透過多節點並行處理，大幅提升運算效率。",
        "advantage2_title": "彈性擴展", 
        "advantage2_desc": "根據需求動態調整運算資源，靈活配置。",
        "advantage3_title": "安全可靠",
        "advantage3_desc": "採用安全標準，確保數據和運算過程的安全性。",
        "performance_comparison": "效能對比",
        "traditional_computing": "傳統運算",
        "cloud_computing": "雲端運算",
        "hivemind_computing": "HiveMind",
        "about_title": "關於我們",
        "about_desc": "我們是一群相信「運算力應人人可用」的開發者，打造這套平台，目標是讓任何人都能輕鬆取得分散式資源，不再受限本機硬體。",
        "value1_title": "開放創新",
        "value1_desc": "擁抱開源精神，持續創新技術",
        "value2_title": "社群驅動",
        "value2_desc": "以用戶需求為核心，共同成長",
        "value3_title": "全球視野",
        "value3_desc": "建設普惠的全球運算網路",
        "ready_to_start": "準備開始了嗎？",
        "cta_subtitle": "加入 HiveMind，體驗前所未有的分布式運算能力",
        "register_now": "立即註冊",
        "download_client": "下載客戶端",
        "footer_copyright": "© 2025 Hivemind Project. Built with love by Justin.",

        // 登入頁面
        "login_title": "歡迎回來",
        "login_subtitle": "登入您的 HiveMind 帳戶",
        "login_username_label": "使用者名稱",
        "login_username_placeholder": "請輸入使用者名稱",
        "login_password_label": "密碼",
        "login_password_placeholder": "請輸入密碼",
        "login_captcha_label": "真人驗證",
        "login_btn": "登入",
        "login_no_account": "還沒有帳戶？",
        "login_register_link": "立即註冊",
        "login_forgot_password": "忘記密碼？",
        
        // 註冊頁面
        "register_title": "註冊帳戶",
        "register_subtitle": "創建您的 HiveMind 帳戶",
        "register_username_label": "使用者名稱",
        "register_username_placeholder": "請輸入使用者名稱",
        "register_password_label": "密碼",
        "register_password_placeholder": "請輸入密碼",
        "register_confirm_label": "確認密碼",
        "register_confirm_placeholder": "請再次輸入密碼",
        "register_captcha_label": "真人驗證",
        "register_btn": "創建帳戶",
        "register_already_account": "已經有帳戶了？",
        "register_login_link": "立即登入",
        "register_email_note": "註冊後請添加並驗證電子郵件以獲得100 CPT獎勵",
        
        // 下載頁面
        "download_title": "下載 HiveMind",
        "download_subtitle": "選擇適合您需求的客戶端，開始使用分布式運算平台",
        "worker_title": "工作端 (Worker)",
        "worker_description": "貢獻您的閒置資源到網路中並賺取獎勵",
        "master_title": "主控端 (Master)",
        "master_description": "部署主控伺服器以管理分布式運算任務",
        "download_windows": "下載 Windows 版本",
        "download_linux": "下載 Linux 版本",
        "popular": "熱門",
        "enterprise": "企業版",
        "coming_soon": "即將推出",
        "coming_soon_subtitle": "更多平台版本正在開發中",
        "web_version": "網頁版",
        "web_version_desc": "直接在瀏覽器運行，無需安裝",
        "mobile_version": "手機版",
        "mobile_version_desc": "Android 與 iOS 應用程式",
        "docker_version": "docker版本",
        "docker_version_desc": "一鍵部署至雲端平台",
        "help_subtitle": "查看我們的文檔或聯繫支援團隊",
        "view_docs": "查看文檔",
        "join_community": "加入社群",

        // 餘額頁面
        "balance_title": "我的面板",
        "balance_subtitle": "查看您的帳戶餘額並進行轉帳操作",
        "balance_my_balance": "我的餘額",
        "balance_transfer": "轉帳",
        "balance_receiver_label": "收款人用戶名",
        "balance_receiver_placeholder": "請輸入收款人用戶名",
        "balance_amount_label": "金額",
        "balance_amount_placeholder": "請輸入金額",
        "balance_transfer_btn": "確認轉帳",
        "balance_account_settings": "帳戶設定",
        "balance_email_status": "電子郵件狀態",
        "balance_email_management": "電子郵件管理",
        "balance_email_label": "電子郵件",
        "balance_email_placeholder": "請輸入電子郵件",
        "balance_update_email_btn": "更新電子郵件",
        "balance_email_not_set": "未設定電子郵件",
        "balance_email_reward_hint": "驗證電子郵件可獲得100 CPT",
        "balance_email_verified": "已驗證",
        "balance_email_unverified": "未驗證",
        "balance_email_check_inbox": "請檢查您的信箱",
        "balance_password_management": "密碼管理",
        "balance_current_password_label": "目前密碼",
        "balance_current_password_placeholder": "請輸入目前密碼",
        "balance_new_password_label": "新密碼",
        "balance_new_password_placeholder": "請輸入新密碼",
        "balance_confirm_new_password_label": "確認新密碼",
        "balance_confirm_new_password_placeholder": "請再次輸入新密碼",
        "balance_update_password_btn": "更新密碼",
        "balance_credit_score": "信任分數",
        "balance_credit_score_range": "範圍: 0-1000",

        // 贊助頁面
        "sponsor_thanks": "感謝以下贊助者對 HiveMind 的支持！",
        "sponsor_main_developer": "主要開發者",
        "visit_website": "訪問網站",
        "become_sponsor": "成為贊助者",
        "sponsor_cta_desc": "支持 HiveMind 的發展，讓更多人享受分布式運算的便利",
        "support_project": "支持專案",

        // 條款頁面
        "terms_title": "服務條款",
        "terms_service_agreement": "服務協議",
        "terms_prohibited_behavior": "禁止行為",
        "terms_service_changes": "服務變更",
        "terms_violation_handling": "違規處理",
        "terms_agreement": "使用本平台即表示您同意遵守相關法律法規及本協議條款。",
        "terms_illegal": "您不得利用本平台從事任何違法、侵權或損害他人權益的活動。",
        "terms_changes": "平台保留隨時調整服務內容、條款及政策的權利，重大變更將進行公告。",
        "terms_violation": "若您違反條款，平台有權暫停或終止您的使用權。",
        "terms_contact": "若有任何疑問，請聯繫我們：",

        // 隱私政策頁面
        "privacy_title": "隱私權政策",
        "privacy_welcome": "歡迎使用 HiveMind 分布式運算平台（下稱「本平台」），我們非常重視您的個人資料保護，請詳閱本政策：",
        "privacy_collection_title": "1. 資料收集",
        "privacy_collection_content": "本平台僅在註冊、登入及使用服務時，收集必要的帳號、信箱及登入紀錄資訊。",
        "privacy_collection_note": "我們不會主動收集您的敏感個人資料。",
        "privacy_cookie_title": "2. Cookie 與追蹤",
        "privacy_cookie_content": "僅用於登入狀態維持及平台安全，不涉及廣告追蹤。",
        "privacy_usage_title": "3. 資料用途",
        "privacy_usage_content": "僅用於帳戶管理、服務提供、平台安全及法律遵循。",
        "privacy_third_party_title": "4. 第三方服務",
        "privacy_third_party_content": "本平台可能整合第三方登入、支付等服務，僅在必要時傳遞最少資訊。",
        "privacy_security_title": "5. 資料安全",
        "privacy_security_content": "我們採用加密、存取控制等措施，保護您的資料安全。",
        "privacy_rights_title": "6. 您的權利",
        "privacy_rights_content": "您隨時可查詢、更正、刪除個人資料，或聯繫我們行使權利。",
        "privacy_contact_title": "7. 聯繫方式",
        "privacy_contact_content": "若有隱私疑慮，請來信：hivemind@justin0711.com",
        "privacy_revision": "本政策若有修訂，將於此頁面公告。",

        // 忘記密碼頁面翻譯
        "login_forgot_password": "忘記密碼？",
        "forgot_password_title": "忘記密碼",
        "forgot_password_subtitle": "輸入您的電子郵件地址以重設密碼",
        "forgot_password_form_title": "重設密碼",
        "forgot_password_description": "請輸入您的電子郵件地址，我們將發送密碼重設連結給您",
        "forgot_password_email_label": "電子郵件地址",
        "forgot_password_email_placeholder": "請輸入電子郵件地址",
        "forgot_password_send_btn": "發送重設連結",
        "forgot_password_remember": "記起密碼了？",
        "forgot_password_login_link": "立即登入",

        // 重設密碼頁面翻譯
        "reset_password_title": "重設密碼",
        "reset_password_subtitle": "設定您的新密碼",
        "reset_password_form_title": "設定新密碼",
        "reset_password_new_label": "新密碼",
        "reset_password_new_placeholder": "請輸入新密碼",
        "reset_password_confirm_label": "確認新密碼",
        "reset_password_confirm_placeholder": "請再次輸入新密碼",
        "reset_password_requirements": "密碼必須至少6個字元",
        "reset_password_submit_btn": "重設密碼",
        "reset_password_login_link": "返回登入頁面",

        // 註冊頁面電子郵件提示翻譯
        "register_email_note": "註冊後請添加並驗證電子郵件以獲得100 CPT獎勵",

        // 餘額頁面完整翻譯
        "balance_account_settings": "帳戶設定",
        "balance_email_status": "電子郵件狀態",
        "balance_email_management": "電子郵件管理",
        "balance_email_label": "電子郵件",
        "balance_email_placeholder": "請輸入電子郵件",
        "balance_update_email_btn": "更新電子郵件",
        "balance_email_not_set": "未設定電子郵件",
        "balance_email_reward_hint": "驗證電子郵件可獲得100 CPT",
        "balance_email_verified": "已驗證",
        "balance_email_unverified": "未驗證",
        "balance_email_check_inbox": "請檢查您的信箱",
        "balance_password_management": "密碼管理",
        "balance_current_password_label": "目前密碼",
        "balance_current_password_placeholder": "請輸入目前密碼",
        "balance_new_password_label": "新密碼",
        "balance_new_password_placeholder": "請輸入新密碼",
        "balance_confirm_new_password_label": "確認新密碼",
        "balance_confirm_new_password_placeholder": "請再次輸入新密碼",
        "balance_update_password_btn": "更新密碼",
        "balance_credit_score": "信任分數",
        "balance_credit_score_range": "範圍: 0-1000",

        // 電子郵件驗證頁面翻譯
        "email_verification_title": "電子郵件驗證",
        "email_verification_subtitle": "驗證您的電子郵件地址",
        "verifying": "驗證中...",
        "please_wait": "請稍候，正在驗證您的電子郵件地址",
        "verification_success": "驗證成功！",
        "verification_failed": "驗證失敗",
        "verification_reward": "恭喜！您已獲得 100 CPT 獎勵代幣",
        "verification_no_reward": "電子郵件驗證成功（獎勵僅限首次驗證）",
        "verification_error_info": "如果您已經驗證過，請忽略此錯誤",
        "view_balance": "查看餘額",
        "back_to_home": "返回首頁",
        "resend_verification": "重新發送驗證郵件",

        // 通用翻譯
        "loading": "載入中...",
        "error": "錯誤",
        "success": "成功",
        "cancel": "取消",
        "confirm": "確認",
        "save": "儲存",
        "close": "關閉",

        // 導航欄補充翻譯  
        "nav_dashboard": "查看面板",

        // 文檔相關翻譯
        "docs": {
            "documentation": "文檔",
            "navigation": "文檔導航",
            "home": "首頁",
            "api": "API 文檔",
            "deployment": "部署指南",
            "developer": "開發者指南",
            "modules": "模組文檔",
            "modules_overview": "模組概述",
            "node_pool": "節點池",
            "web_interface": "Web 介面",
            "search": "搜索文檔...",
            "print": "列印",
            "view_source": "查看原始碼",
            "table_of_contents": "目錄",
            "last_updated": "最後更新",
            "edit_on_github": "在 GitHub 上編輯",
            "copy_code": "複製代碼",
            "copied": "已複製",
            "copy_failed": "複製到剪貼板失敗"
        }
    },
    "en": {
        // Navbar
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
        
        // Home Page Content
        "welcome": "Unlock the Power of Distributed Computing",
        "subtitle": "Worker + Node Pool, build a high-performance distributed platform. You focus on development, we handle the execution.",
        "system_resources": "System Resources",
        "resource_subtitle": "Our distributed network provides powerful computing resources",
        "total_ram": "Total RAM",
        "total_vram": "Video Memory",
        "cpu_cores": "CPU Cores", 
        "uptime": "Uptime",
        "active_nodes": "Active Nodes",
        "completed_tasks": "Completed Tasks",
        "response_time": "Response Time (hrs)",
        "better_computing_experience": "Better Computing Experience",
        "features_intro": "We provide comprehensive distributed computing solutions to unlock unlimited possibilities for your projects.",
        "feature1_title": "Modular Architecture",
        "feature1_desc": "Break down tasks into manageable subtasks and scale your computing power freely.",
        "feature2_title": "Instant Node Allocation",
        "feature2_desc": "The node pool automatically allocates the most suitable nodes, supporting custom task criteria.",
        "feature3_title": "Security and Isolation",
        "feature3_desc": "Tasks run in Docker containers, ensuring independence and security between nodes.",
        "high_performance_title": "High Performance Processing",
        "high_performance_desc": "Leverage distributed architecture for unprecedented computing power and scalability.",
        "how_it_works": "How It Works",
        "workflow_intro": "Three simple steps to start your distributed computing journey",
        "step1_title": "Register Account",
        "step1_desc": "Create your HiveMind account and start enjoying distributed computing services.",
        "step2_title": "Download Client",
        "step2_desc": "Choose the right client version and quickly deploy to your environment.",
        "step3_title": "View Documentation",
        "step3_desc": "Read comprehensive guides and API documentation to get started quickly.",
        "tech_advantages": "Technical Advantages",
        "advantage1_title": "High Performance",
        "advantage1_desc": "Significantly improve computing efficiency through multi-node parallel processing.",
        "advantage2_title": "Flexible Scaling",
        "advantage2_desc": "Dynamically adjust computing resources according to demand with flexible configuration.",
        "advantage3_title": "Secure & Reliable", 
        "advantage3_desc": "Adopt security standards to ensure the safety of data and computing processes.",
        "performance_comparison": "Performance Comparison",
        "traditional_computing": "Traditional Computing",
        "cloud_computing": "Cloud Computing",
        "hivemind_computing": "HiveMind",
        "about_title": "About Us",
        "about_desc": "We are a group of developers who believe that 'computing power should be accessible to all'. We created this platform to enable anyone to easily access distributed resources, no longer limited by local hardware.",
        "value1_title": "Open Innovation",
        "value1_desc": "Embrace open source spirit, continuously innovate technology",
        "value2_title": "Community Driven",
        "value2_desc": "User-centric approach, growing together",
        "value3_title": "Global Vision",
        "value3_desc": "Building an inclusive global computing network",
        "ready_to_start": "Ready to get started?",
        "cta_subtitle": "Join HiveMind and experience unprecedented distributed computing power",
        "register_now": "Register Now",
        "download_client": "Download Client",
        "footer_copyright": "© 2025 Hivemind Project. Built with love by Justin.",

        // Login Page
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
        "login_forgot_password": "Forgot password?",
        
        // Register Page
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
        "register_email_note": "Please add and verify email after registration to get 100 CPT reward",
        
        // Download Page
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
        "coming_soon_subtitle": "More platform versions are in development",
        "web_version": "Web Version",
        "web_version_desc": "Run directly in browser, no installation required",
        "mobile_version": "Mobile Version",
        "mobile_version_desc": "Android and iOS applications",
        "docker_version": "Docker Version",
        "docker_version_desc": "One-click deployment to cloud platforms",
        "need_help": "Need Help?",
        "help_subtitle": "Check our documentation or contact the support team",
        "view_docs": "View Docs",
        "join_community": "Join Community",

        // Balance Page
        "balance_title": "Dashboard & Transfer",
        "balance_subtitle": "View your account balance and perform transfer operations",
        "balance_my_balance": "My Balance",
        "balance_transfer": "Transfer",
        "balance_receiver_label": "Recipient Username",
        "balance_receiver_placeholder": "Please enter recipient username",
        "balance_amount_label": "Amount",
        "balance_amount_placeholder": "Please enter amount",
        "balance_transfer_btn": "Confirm Transfer",
        "balance_account_settings": "Account Settings",
        "balance_email_status": "Email Status",
        "balance_email_management": "Email Management",
        "balance_email_label": "Email",
        "balance_email_placeholder": "Please enter email",
        "balance_update_email_btn": "Update Email",
        "balance_email_not_set": "Email not set",
        "balance_email_reward_hint": "Verify email to get 100 CPT",
        "balance_email_verified": "Verified",
        "balance_email_unverified": "Unverified",
        "balance_email_check_inbox": "Please check your inbox",
        "balance_password_management": "Password Management",
        "balance_current_password_label": "Current Password",
        "balance_current_password_placeholder": "Please enter current password",
        "balance_new_password_label": "New Password",
        "balance_new_password_placeholder": "Please enter new password",
        "balance_confirm_new_password_label": "Confirm New Password",
        "balance_confirm_new_password_placeholder": "Please enter new password again",
        "balance_update_password_btn": "Update Password",
        "balance_credit_score": "Credit Score",
        "balance_credit_score_range": "Range: 0-1000",

        // Sponsor Page
        "sponsor_thanks": "Thank you to the following sponsors for supporting HiveMind!",
        "sponsor_main_developer": "Lead Developer",
        "visit_website": "Visit Website",
        "become_sponsor": "Become a Sponsor",
        "sponsor_cta_desc": "Support HiveMind's development and let more people enjoy the convenience of distributed computing",
        "support_project": "Support Project",

        // Terms Page
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

        // Privacy Policy Page
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
        "privacy_revision": "If this policy is revised, it will be announced on this page.",

        // 忘記密碼頁面翻譯
        "login_forgot_password": "忘記密碼？",
        "forgot_password_title": "忘記密碼",
        "forgot_password_subtitle": "輸入您的電子郵件地址以重設密碼",
        "forgot_password_form_title": "重設密碼",
        "forgot_password_description": "請輸入您的電子郵件地址，我們將發送密碼重設連結給您",
        "forgot_password_email_label": "電子郵件地址",
        "forgot_password_email_placeholder": "請輸入電子郵件地址",
        "forgot_password_send_btn": "發送重設連結",
        "forgot_password_remember": "記起密碼了？",
        "forgot_password_login_link": "立即登入",

        // 重設密碼頁面翻譯
        "reset_password_title": "重設密碼",
        "reset_password_subtitle": "設定您的新密碼",
        "reset_password_form_title": "設定新密碼",
        "reset_password_new_label": "新密碼",
        "reset_password_new_placeholder": "請輸入新密碼",
        "reset_password_confirm_label": "確認新密碼",
        "reset_password_confirm_placeholder": "請再次輸入新密碼",
        "reset_password_requirements": "密碼必須至少6個字元",
        "reset_password_submit_btn": "重設密碼",
        "reset_password_login_link": "返回登入頁面",

        // 註冊頁面電子郵件提示翻譯
        "register_email_note": "註冊後請添加並驗證電子郵件以獲得100 CPT獎勵",

        // 餘額頁面完整翻譯
        "balance_account_settings": "帳戶設定",
        "balance_email_status": "電子郵件狀態",
        "balance_email_management": "電子郵件管理",
        "balance_email_label": "電子郵件",
        "balance_email_placeholder": "請輸入電子郵件",
        "balance_update_email_btn": "更新電子郵件",
        "balance_email_not_set": "未設定電子郵件",
        "balance_email_reward_hint": "驗證電子郵件可獲得100 CPT",
        "balance_email_verified": "已驗證",
        "balance_email_unverified": "未驗證",
        "balance_email_check_inbox": "請檢查您的信箱",
        "balance_password_management": "密碼管理",
        "balance_current_password_label": "目前密碼",
        "balance_current_password_placeholder": "請輸入目前密碼",
        "balance_new_password_label": "新密碼",
        "balance_new_password_placeholder": "請輸入新密碼",
        "balance_confirm_new_password_label": "確認新密碼",
        "balance_confirm_new_password_placeholder": "請再次輸入新密碼",
        "balance_update_password_btn": "更新密碼",
        "balance_credit_score": "信任分數",
        "balance_credit_score_range": "範圍: 0-1000",

        // 電子郵件驗證頁面翻譯
        "email_verification_title": "電子郵件驗證",
        "email_verification_subtitle": "驗證您的電子郵件地址",
        "verifying": "驗證中...",
        "please_wait": "請稍候，正在驗證您的電子郵件地址",
        "verification_success": "驗證成功！",
        "verification_failed": "驗證失敗",
        "verification_reward": "恭喜！您已獲得 100 CPT 獎勵代幣",
        "verification_no_reward": "電子郵件驗證成功（獎勵僅限首次驗證）",
        "verification_error_info": "如果您已經驗證過，請忽略此錯誤",
        "view_balance": "查看餘額",
        "back_to_home": "返回首頁",
        "resend_verification": "重新發送驗證郵件",

        // 通用翻譯
        "loading": "載入中...",
        "error": "錯誤",
        "success": "成功",
        "cancel": "取消",
        "confirm": "確認",
        "save": "儲存",
        "close": "關閉",

        // 導航欄補充翻譯  
        "nav_dashboard": "查看面板",

        // 文檔相關翻譯
        "docs": {
            "documentation": "文檔",
            "navigation": "文檔導航",
            "home": "首頁",
            "api": "API 文檔",
            "deployment": "部署指南",
            "developer": "開發者指南",
            "modules": "模組文檔",
            "modules_overview": "模組概述",
            "node_pool": "節點池",
            "web_interface": "Web 介面",
            "search": "搜索文檔...",
            "print": "列印",
            "view_source": "查看原始碼",
            "table_of_contents": "目錄",
            "last_updated": "最後更新",
            "edit_on_github": "在 GitHub 上編輯",
            "copy_code": "複製代碼",
            "copied": "已複製",
            "copy_failed": "複製到剪貼板失敗"
        }
    },
    "en": {
        // Navbar
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
        
        // Home Page Content
        "welcome": "Unlock the Power of Distributed Computing",
        "subtitle": "Worker + Node Pool, build a high-performance distributed platform. You focus on development, we handle the execution.",
        "system_resources": "System Resources",
        "resource_subtitle": "Our distributed network provides powerful computing resources",
        "total_ram": "Total RAM",
        "total_vram": "Video Memory",
        "cpu_cores": "CPU Cores", 
        "uptime": "Uptime",
        "active_nodes": "Active Nodes",
        "completed_tasks": "Completed Tasks",
        "response_time": "Response Time (hrs)",
        "better_computing_experience": "Better Computing Experience",
        "features_intro": "We provide comprehensive distributed computing solutions to unlock unlimited possibilities for your projects.",
        "feature1_title": "Modular Architecture",
        "feature1_desc": "Break down tasks into manageable subtasks and scale your computing power freely.",
        "feature2_title": "Instant Node Allocation",
        "feature2_desc": "The node pool automatically allocates the most suitable nodes, supporting custom task criteria.",
        "feature3_title": "Security and Isolation",
        "feature3_desc": "Tasks run in Docker containers, ensuring independence and security between nodes.",
        "high_performance_title": "High Performance Processing",
        "high_performance_desc": "Leverage distributed architecture for unprecedented computing power and scalability.",
        "how_it_works": "How It Works",
        "workflow_intro": "Three simple steps to start your distributed computing journey",
        "step1_title": "Register Account",
        "step1_desc": "Create your HiveMind account and start enjoying distributed computing services.",
        "step2_title": "Download Client",
        "step2_desc": "Choose the right client version and quickly deploy to your environment.",
        "step3_title": "View Documentation",
        "step3_desc": "Read comprehensive guides and API documentation to get started quickly.",
        "tech_advantages": "Technical Advantages",
        "advantage1_title": "High Performance",
        "advantage1_desc": "Significantly improve computing efficiency through multi-node parallel processing.",
        "advantage2_title": "Flexible Scaling",
        "advantage2_desc": "Dynamically adjust computing resources according to demand with flexible configuration.",
        "advantage3_title": "Secure & Reliable", 
        "advantage3_desc": "Adopt security standards to ensure the safety of data and computing processes.",
        "performance_comparison": "Performance Comparison",
        "traditional_computing": "Traditional Computing",
        "cloud_computing": "Cloud Computing",
        "hivemind_computing": "HiveMind",
        "about_title": "About Us",
        "about_desc": "We are a group of developers who believe that 'computing power should be accessible to all'. We created this platform to enable anyone to easily access distributed resources, no longer limited by local hardware.",
        "value1_title": "Open Innovation",
        "value1_desc": "Embrace open source spirit, continuously innovate technology",
        "value2_title": "Community Driven",
        "value2_desc": "User-centric approach, growing together",
        "value3_title": "Global Vision",
        "value3_desc": "Building an inclusive global computing network",
        "ready_to_start": "Ready to get started?",
        "cta_subtitle": "Join HiveMind and experience unprecedented distributed computing power",
        "register_now": "Register Now",
        "download_client": "Download Client",
        "footer_copyright": "© 2025 Hivemind Project. Built with love by Justin.",

        // Login Page
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
        "login_forgot_password": "Forgot password?",
        
        // Register Page
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
        "register_email_note": "Please add and verify email after registration to get 100 CPT reward",
        
        // Download Page
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
        "coming_soon_subtitle": "More platform versions are in development",
        "web_version": "Web Version",
        "web_version_desc": "Run directly in browser, no installation required",
        "mobile_version": "Mobile Version",
        "mobile_version_desc": "Android and iOS applications",
        "docker_version": "Docker Version",
        "docker_version_desc": "One-click deployment to cloud platforms",
        "need_help": "Need Help?",
        "help_subtitle": "Check our documentation or contact the support team",
        "view_docs": "View Docs",
        "join_community": "Join Community",

        // Balance Page
        "balance_title": "Dashboard & Transfer",
        "balance_subtitle": "View your account balance and perform transfer operations",
        "balance_my_balance": "My Balance",
        "balance_transfer": "Transfer",
        "balance_receiver_label": "Recipient Username",
        "balance_receiver_placeholder": "Please enter recipient username",
        "balance_amount_label": "Amount",
        "balance_amount_placeholder": "Please enter amount",
        "balance_transfer_btn": "Confirm Transfer",
        "balance_account_settings": "Account Settings",
        "balance_email_status": "Email Status",
        "balance_email_management": "Email Management",
        "balance_email_label": "Email",
        "balance_email_placeholder": "Please enter email",
        "balance_update_email_btn": "Update Email",
        "balance_email_not_set": "Email not set",
        "balance_email_reward_hint": "Verify email to get 100 CPT",
        "balance_email_verified": "Verified",
        "balance_email_unverified": "Unverified",
        "balance_email_check_inbox": "Please check your inbox",
        "balance_password_management": "Password Management",
        "balance_current_password_label": "Current Password",
        "balance_current_password_placeholder": "Please enter current password",
        "balance_new_password_label": "New Password",
        "balance_new_password_placeholder": "Please enter new password",
        "balance_confirm_new_password_label": "Confirm New Password",
        "balance_confirm_new_password_placeholder": "Please enter new password again",
        "balance_update_password_btn": "Update Password",
        "balance_credit_score": "Credit Score",
        "balance_credit_score_range": "Range: 0-1000",

        // Sponsor Page
        "sponsor_thanks": "Thank you to the following sponsors for supporting HiveMind!",
        "sponsor_main_developer": "Lead Developer",
        "visit_website": "Visit Website",
        "become_sponsor": "Become a Sponsor",
        "sponsor_cta_desc": "Support HiveMind's development and let more people enjoy the convenience of distributed computing",
        "support_project": "Support Project",

        // Terms Page
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

        // Privacy Policy Page
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
        "privacy_revision": "If this policy is revised, it will be announced on this page.",

        // 忘記密碼頁面翻譯
        "login_forgot_password": "Forgot password?",
        "forgot_password_title": "Forgot Password",
        "forgot_password_subtitle": "Enter your email address to reset your password",
        "forgot_password_form_title": "Reset Password",
        "forgot_password_description": "Please enter your email address and we will send you a password reset link",
        "forgot_password_email_label": "Email Address",
        "forgot_password_email_placeholder": "Please enter email address",
        "forgot_password_send_btn": "Send Reset Link",
        "forgot_password_remember": "Remember your password?",
        "forgot_password_login_link": "Sign in now",

        // Reset Password Page
        "reset_password_title": "Reset Password",
        "reset_password_subtitle": "Set your new password",
        "reset_password_form_title": "Set New Password",
        "reset_password_new_label": "New Password",
        "reset_password_new_placeholder": "Please enter new password",
        "reset_password_confirm_label": "Confirm New Password",
        "reset_password_confirm_placeholder": "Please enter new password again",
        "reset_password_requirements": "Password must be at least 6 characters",
        "reset_password_submit_btn": "Reset Password",
        "reset_password_login_link": "Back to login page",

        // 新增註冊頁面電子郵件提示翻譯
        "register_email_note": "Please add and verify email after registration to get 100 CPT reward",

        // 新增餘額頁面完整翻譯
        "balance_account_settings": "Account Settings",
        "balance_email_status": "Email Status",
        "balance_email_management": "Email Management",
        "balance_email_label": "Email",
        "balance_email_placeholder": "Please enter email",
        "balance_update_email_btn": "Update Email",
        "balance_email_not_set": "Email not set",
        "balance_email_reward_hint": "Verify email to get 100 CPT",
        "balance_email_verified": "Verified",
        "balance_email_unverified": "Unverified",
        "balance_email_check_inbox": "Please check your inbox",
        "balance_password_management": "Password Management",
        "balance_current_password_label": "Current Password",
        "balance_current_password_placeholder": "Please enter current password",
        "balance_new_password_label": "New Password",
        "balance_new_password_placeholder": "Please enter new password",
        "balance_confirm_new_password_label": "Confirm New Password",
        "balance_confirm_new_password_placeholder": "Please enter new password again",
        "balance_update_password_btn": "Update Password",
        "balance_credit_score": "Credit Score",
        "balance_credit_score_range": "Range: 0-1000",

        // 新增電子郵件驗證頁面翻譯
        "email_verification_title": "Email Verification",
        "email_verification_subtitle": "Verify your email address",
        "verifying": "Verifying...",
        "please_wait": "Please wait, verifying your email address",
        "verification_success": "Verification Successful!",
        "verification_failed": "Verification Failed",
        "verification_reward": "Congratulations! You have received 100 CPT reward tokens",
        "verification_no_reward": "Email verified successfully (reward only for first verification)",
        "verification_error_info": "If you have already verified, please ignore this error",
        "view_balance": "View Balance",
        "back_to_home": "Back to Home",
        "resend_verification": "Resend Verification Email",

        // Common
        "loading": "Loading...",
        "error": "Error",
        "success": "Success",
        "cancel": "Cancel",
        "confirm": "Confirm",
        "save": "Save",
        "close": "Close",

        // Navigation
        "nav_dashboard": "Dashboard",

        // 翻譯數據
        docs: {
            documentation: 'Documentation',
            navigation: 'Documentation Navigation',
            home: 'Home',
            api: 'API Documentation',
            deployment: 'Deployment Guide',
            developer: 'Developer Guide',
            modules: 'Module Documentation',
            modules_overview: 'Module Overview',
            node_pool: 'Node Pool',
            web_interface: 'Web Interface',
            search: 'Search documentation...',
            print: 'Print',
            view_source: 'View Source',
            table_of_contents: 'Table of Contents',
            last_updated: 'Last Updated',
            edit_on_github: 'Edit on GitHub',
            copy_code: 'Copy Code',
            copied: 'Copied',
            copy_failed: 'Failed to copy to clipboard'
        }
    },
    "zh-cn": {
        docs: {
            documentation: '文档',
            navigation: '文档导航',
            home: '首页',
            api: 'API 文档',
            deployment: '部署指南',
            developer: '开发者指南',
            modules: '模块文档',
            modules_overview: '模块概述',
            node_pool: '节点池',
            web_interface: 'Web 界面',
            search: '搜索文档...',
            print: '打印',
            view_source: '查看源码',
            table_of_contents: '目录',
            last_updated: '最后更新',
            edit_on_github: '在 GitHub 上编辑',
            copy_code: '复制代码',
            copied: '已复制',
            copy_failed: '复制到剪贴板失败'
        }
    }
};

/**
 * 獲取儲存在 localStorage 中的語言設定，預設為 'en'
 * @returns {string} 當前語言 ('en' 或 'zh-tw')
 */
function getLang() {
    // 優先從 localStorage 讀取，若無則預設為 'en'
    const savedLang = localStorage.getItem('lang');
    return savedLang === 'zh-tw' ? 'zh-tw' : 'en';
}

/**
 * 根據傳入的語言設定，更新頁面上的所有文字
 * @param {string} lang - 要設定的語言 ('en', 'zh-tw', 'zh-cn')
 */
function setLang(lang) {
    try {
        // 將語言設定存入 localStorage
        localStorage.setItem('lang', lang);
        
        const langDict = i18n[lang];
        if (!langDict) {
            console.error(`Language dictionary for "${lang}" not found.`);
            return;
        }

        // 更新帶有 data-i18n 屬性的元素文字內容
        document.querySelectorAll('[data-i18n]').forEach(el => {
            const key = el.getAttribute('data-i18n');
            
            // 處理嵌套的翻譯鍵（如 docs.api）
            if (key.includes('.')) {
                const keys = key.split('.');
                let translation = langDict;
                for (const k of keys) {
                    if (translation && translation[k]) {
                        translation = translation[k];
                    } else {
                        translation = null;
                        break;
                    }
                }
                if (translation) {
                    el.textContent = translation;
                } else {
                    console.warn(`Nested translation key "${key}" not found for language "${lang}".`);
                }
            } else if (langDict[key]) {
                el.textContent = langDict[key];
            } else {
                console.warn(`Translation key "${key}" not found for language "${lang}".`);
            }
        });
        
        // 更新帶有 data-i18n-placeholder 屬性的輸入框 placeholder
        document.querySelectorAll('[data-i18n-placeholder]').forEach(el => {
            const key = el.getAttribute('data-i18n-placeholder');
            
            // 處理嵌套的翻譯鍵
            if (key.includes('.')) {
                const keys = key.split('.');
                let translation = langDict;
                for (const k of keys) {
                    if (translation && translation[k]) {
                        translation = translation[k];
                    } else {
                        translation = null;
                        break;
                    }
                }
                if (translation) {
                    el.placeholder = translation;
                }
            } else if (langDict[key]) {
                el.placeholder = langDict[key];
            } else {
                console.warn(`Placeholder key "${key}" not found for language "${lang}".`);
            }
        });
    } catch (error) {
        console.error('Error setting language:', error);
    }
}

/**
 * 切換當前語言 (在 'zh-tw' 和 'en' 之間)
 */
function toggleLang() {
    try {
        const currentLang = getLang();
        const newLang = currentLang === 'zh-tw' ? 'en' : 'zh-tw';
        setLang(newLang);
    } catch (error) {
        console.error('Error toggling language:', error);
    }
}

// 新增函數：為其他頁面提供翻譯功能
function translatePage(lang) {
    setLang(lang);
}

// === 事件監聽與初始化 ===

// 確保在瀏覽器環境下才執行
if (typeof document !== 'undefined') {
    // 當 DOM 內容完全載入後，立即根據儲存的設定或預設值來設定頁面語言
    document.addEventListener('DOMContentLoaded', function() {
        try {
            const initialLang = getLang();
            setLang(initialLang);
        } catch (error) {
            console.error('Error on initial language setup:', error);
        }
    });
}

// 為了讓 HTML 中的 onclick="toggleLang()" 可以呼叫到，將函式掛載到 window 物件上
if (typeof window !== 'undefined') {
    window.toggleLang = toggleLang;
    window.translatePage = translatePage;
    window.setLang = setLang;
    window.getLang = getLang;
    window.i18n = i18n;
    window.I18n = I18n;
}