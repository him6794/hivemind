/* filepath: d:\hivemind\web\static\js\main.js */
class HiveMindApp {
    constructor() {
        // 統一使用 access_token 和 user_info
        this.token = localStorage.getItem('access_token');
        this.user = JSON.parse(localStorage.getItem('user_info') || 'null');
        this.tokenRefreshInProgress = false;
        this.init();
    }

    init() {
        this.initTheme();
        this.setupEventListeners();
    }

    // 主題管理 - 強制深色模式
    initTheme() {
        // 強制設定深色模式，忽略用戶偏好和裝置設定
        this.setTheme(true);
    }

    setTheme(dark) {
        const html = document.documentElement;
        
        // 強制深色模式
        html.classList.add('dark');
        localStorage.setItem('theme', 'dark');
        
        // 確保矩陣背景顯示
        const matrixBg = document.getElementById('matrix-bg');
        if (matrixBg) {
            matrixBg.style.display = 'block';
            matrixBg.classList.remove('hidden');
        }
    }

    // 移除主題切換功能
    toggleTheme() {
        // 不執行任何操作，保持深色模式
        return;
    }

    // 認證UI更新
    updateAuthUI() {
        // 不要控制 UI 元素的顯示/隱藏
        // 留給 base.html 處理
    }

    // 事件監聽器設定
    setupEventListeners() {
        // 移除主題切換按鈕事件監聽器
        // const themeToggle = document.getElementById('theme-toggle');
        // if (themeToggle) {
        //     themeToggle.addEventListener('click', () => this.toggleTheme());
        // }

        // 登出按鈕
        const logoutBtn = document.getElementById('logout-btn');
        if (logoutBtn) {
            logoutBtn.addEventListener('click', () => this.logout());
        }

        // 平滑滾動錨點
        document.querySelectorAll('a[href^="#"]').forEach(anchor => {
            anchor.addEventListener('click', function (e) {
                e.preventDefault();
                const target = document.querySelector(this.getAttribute('href'));
                if (target) {
                    target.scrollIntoView({
                        behavior: 'smooth',
                        block: 'start'
                    });
                }
            });
        });
    }

    // 檢查認證狀態
    checkAuthStatus() {
        if (!this.token) return;

        try {
            // 僅檢查，不更新 UI
            this.apiCall('/api/balance', 'GET');
        } catch (error) {
            console.error('檢查認證狀態失敗:', error);
        }
    }

    // API呼叫，自動處理 Token 刷新
    async apiCall(url, method = 'GET', data = null) {
        const config = {
            method,
            headers: {
                'Content-Type': 'application/json'
            }
        };

        if (this.token) {
            config.headers['Authorization'] = `Bearer ${this.token}`;
        }

        if (data) {
            config.body = JSON.stringify(data);
        }

        try {
            let response = await fetch(url, config);
            
            // 檢查是否為 Token 過期
            if (response.status === 401) {
                const errorData = await response.json();
                if (errorData.error_code === 'TOKEN_EXPIRED' && !this.tokenRefreshInProgress) {
                    console.log('Token 已過期，嘗試刷新...');
                    
                    const refreshed = await this.refreshToken();
                    if (refreshed) {
                        // 更新 Token 後重新發送請求
                        config.headers['Authorization'] = `Bearer ${this.token}`;
                        response = await fetch(url, config);
                    } else {
                        // Token 刷新失敗，跳轉到登入頁面
                        this.handleTokenExpired();
                        throw new Error('Token 刷新失敗，請重新登入');
                    }
                }
            }
            
            return response;
        } catch (error) {
            console.error('API 調用錯誤:', error);
            throw error;
        }
    }

    async refreshToken() {
        if (this.tokenRefreshInProgress || !this.token) {
            return false;
        }

        this.tokenRefreshInProgress = true;
        
        try {
            const response = await fetch('/api/refresh-token', {
                method: 'POST',
                headers: {
                    'Authorization': `Bearer ${this.token}`,
                    'Content-Type': 'application/json'
                }
            });

            if (response.ok) {
                const data = await response.json();
                this.token = data.access_token;
                localStorage.setItem('access_token', this.token);
                
                console.log('Token 刷新成功');
                this.showNotification('登入狀態已自動更新', 'success');
                return true;
            } else {
                console.log('Token 刷新失敗');
                return false;
            }
        } catch (error) {
            console.error('Token 刷新錯誤:', error);
            return false;
        } finally {
            this.tokenRefreshInProgress = false;
        }
    }

    handleTokenExpired() {
        console.log('Token 過期且無法刷新，清除本地存儲');
        localStorage.removeItem('access_token');
        localStorage.removeItem('user_info');
        this.token = null;
        this.user = null;
        
        this.showNotification('登入已過期，請重新登入', 'warning');
        
        // 觸發認證狀態變更事件
        document.dispatchEvent(new Event('authChanged'));
        
        // 如果不在登入頁面，則跳轉
        if (!window.location.pathname.includes('/login')) {
            setTimeout(() => {
                window.location.href = '/login';
            }, 2000);
        }
    }

    // 定期檢查 Token 有效性
    startTokenValidationCheck() {
        // 每 5 分鐘檢查一次 Token
        setInterval(async () => {
            if (this.token && !this.tokenRefreshInProgress) {
                try {
                    const response = await this.apiCall('/api/balance', 'GET');
                    if (!response.ok) {
                        console.log('Token 驗證失敗，可能需要刷新');
                    }
                } catch (error) {
                    console.log('Token 驗證檢查出錯:', error);
                }
            }
        }, 5 * 60 * 1000); // 5分鐘
    }

    // 登出
    logout() {
        localStorage.removeItem('access_token');
        localStorage.removeItem('user_info');
        this.token = null;
        this.user = null;
        document.dispatchEvent(new Event('authChanged'));
        window.location.href = '/';
    }

    // 通知系統
    showNotification(message, type = 'info') {
        const notification = document.createElement('div');
        notification.className = `fixed top-20 right-4 z-50 p-4 rounded-lg shadow-lg max-w-sm transition-all duration-300`;
        
        const colors = {
            success: 'bg-green-50 dark:bg-green-900/30 border border-green-200 dark:border-green-700 text-green-700 dark:text-green-200',
            error: 'bg-red-50 dark:bg-red-900/30 border border-red-200 dark:border-red-700 text-red-700 dark:text-red-200',
            warning: 'bg-yellow-50 dark:bg-yellow-900/30 border border-yellow-200 dark:border-yellow-700 text-yellow-700 dark:text-yellow-200',
            info: 'bg-blue-50 dark:bg-blue-900/30 border border-blue-200 dark:border-blue-700 text-blue-700 dark:text-blue-200'
        };

        const icons = {
            success: 'fa-check-circle',
            error: 'fa-exclamation-circle',
            warning: 'fa-exclamation-triangle',
            info: 'fa-info-circle'
        };

        notification.className += ` ${colors[type] || colors.info}`;
        notification.innerHTML = `
            <div class="flex items-center space-x-2">
                <i class="fas ${icons[type] || icons.info}"></i>
                <span>${message}</span>
                <button class="ml-auto hover:opacity-70" onclick="this.parentElement.parentElement.remove()">
                    <i class="fas fa-times"></i>
                </button>
            </div>
        `;

        document.body.appendChild(notification);

        setTimeout(() => {
            if (notification.parentElement) {
                notification.remove();
            }
        }, 5000);
    }

    // 工具函數
    formatFileSize(bytes) {
        const sizes = ['B', 'KB', 'MB', 'GB'];
        if (bytes === 0) return '0 B';
        const i = Math.floor(Math.log(bytes) / Math.log(1024));
        return `${(bytes / Math.pow(1024, i)).toFixed(1)} ${sizes[i]}`;
    }

    formatDate(dateString) {
        const date = new Date(dateString);
        return date.toLocaleString('zh-TW');
    }
}

// 全局應用實例
let app;

// 初始化應用
document.addEventListener('DOMContentLoaded', () => {
    app = new HiveMindApp();
});

// 全局函數
window.logout = function() {
    if (app) app.logout();
};

// 更新餘額顯示函數，加入自動刷新
window.showBalance = async function() {
    if (!app || !app.token) {
        app.showNotification('請先登入', 'warning');
        return;
    }

    try {
        const response = await app.apiCall('/api/balance');
        const data = await response.json();
        
        if (response.ok) {
            app.showNotification(`目前餘額: ${data.balance} CPT`, 'info');
        } else {
            app.showNotification(data.error || '獲取餘額失敗', 'error');
        }
    } catch (error) {
        console.error('獲取餘額錯誤:', error);
        app.showNotification('獲取餘額失敗', 'error');
    }
};

window.joinVPN = async function() {
    if (!app || !app.token) {
        app.showNotification('請先登入', 'warning');
        window.location.href = '/login';
        return;
    }

    try {
        const response = await app.apiCall('/api/vpn/join', 'POST');
        const data = await response.json();

        if (response.ok) {
            app.showNotification('VPN配置生成成功', 'success');
            
            // 自動下載配置檔案
            const blob = new Blob([data.config], { type: 'text/plain' });
            const url = window.URL.createObjectURL(blob);
            const a = document.createElement('a');
            a.href = url;
            a.download = `${data.client_name}.conf`;
            document.body.appendChild(a);
            a.click();
            document.body.removeChild(a);
            window.URL.revokeObjectURL(url);
            
            // 創建詳細信息模態框
            const modal = document.createElement('div');
            modal.className = 'fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center z-50';
            modal.innerHTML = `
                <div class="bg-white dark:bg-neutral-800 p-6 rounded-lg max-w-2xl w-full mx-4 max-h-[80vh] overflow-auto">
                    <h3 class="text-xl font-bold mb-4 text-neutral-900 dark:text-neutral-100">
                        <i class="fas fa-shield-alt mr-2"></i>VPN配置已生成
                    </h3>
                    <div class="space-y-3 mb-4">
                        <div class="flex justify-between">
                            <span class="font-medium text-neutral-700 dark:text-neutral-300">客戶端名稱：</span>
                            <span class="text-neutral-600 dark:text-neutral-400">${data.client_name}</span>
                        </div>
                        <div class="flex justify-between">
                            <span class="font-medium text-neutral-700 dark:text-neutral-300">分配 IP：</span>
                            <span class="text-neutral-600 dark:text-neutral-400">${data.client_ip}</span>
                        </div>
                    </div>
                    <div class="bg-neutral-100 dark:bg-neutral-900 p-4 rounded text-xs font-mono overflow-auto max-h-40 mb-4">
                        ${data.config.replace(/\n/g, '<br>')}
                    </div>
                    <div class="text-sm text-neutral-600 dark:text-neutral-400 mb-4">
                        配置檔案已自動下載。請按照以下步驟設置：
                        <ul class="list-disc list-inside mt-2 space-y-1">
                            <li>Windows: 安裝 WireGuard → 導入配置 → 激活</li>
                            <li>手機: 安裝 WireGuard 應用 → 掃描或導入配置</li>
                        </ul>
                    </div>
                    <div class="flex space-x-2">
                        <a href="/vpn" class="btn-primary">前往 VPN 管理</a>
                        <button onclick="this.closest('.fixed').remove()" class="btn-secondary">關閉</button>
                    </div>
                </div>
            `;
            document.body.appendChild(modal);
        } else {
            app.showNotification(data.error || '加入VPN失敗', 'error');
        }
    } catch (error) {
        app.showNotification('加入VPN失敗，請稍後再試', 'error');
    }
};

window.downloadClient = async function(type) {
    if (!app || !app.token) {
        app.showNotification('請先登入才能下載客戶端', 'warning');
        window.location.href = '/login';
        return;
    }
    
    try {
        const response = await app.apiCall('/api/download/client');
        const data = await response.json();
        
        if (response.ok && data.download_url) {
            app.showNotification(`開始下載${type}客戶端 v${data.version}`, 'success');
            // 這裡可以觸發實際的檔案下載
        } else {
            app.showNotification(data.error || '下載失敗', 'error');
        }
    } catch (error) {
        app.showNotification('下載失敗，請稍後再試', 'error');
    }
};

window.downloadVPNConfig = async function() {
    if (!app || !app.token) {
        app.showNotification('請先登入', 'warning');
        return;
    }
    
    try {
        const response = await app.apiCall('/api/download/vpn-config');
        const data = await response.json();
        
        if (response.ok && data.config) {
            // 創建並下載配置檔案
            const blob = new Blob([data.config], { type: 'text/plain' });
            const url = window.URL.createObjectURL(blob);
            const a = document.createElement('a');
            a.href = url;
            a.download = 'hivemind-vpn.conf';
            document.body.appendChild(a);
            a.click();
            document.body.removeChild(a);
            window.URL.revokeObjectURL(url);
            app.showNotification('VPN配置已下載', 'success');
        } else {
            app.showNotification(data.error || '下載VPN配置失敗', 'error');
        }
    } catch (error) {
        app.showNotification('下載VPN配置失敗', 'error');
    }
};

// 表單處理函數
window.handleLogin = async function(event) {
    event.preventDefault();
    
    const submitBtn = document.getElementById('submitBtn');
    const submitText = document.getElementById('submitText');
    const loadingIcon = document.getElementById('loadingIcon');
    const errorMsg = document.getElementById('error-message');
    const successMsg = document.getElementById('success-message');
    
    if (errorMsg) errorMsg.classList.add('hidden');
    if (successMsg) successMsg.classList.add('hidden');
    
    if (submitBtn) submitBtn.disabled = true;
    if (submitText) submitText.textContent = '登入中...';
    if (loadingIcon) loadingIcon.classList.remove('hidden');
    
    const formData = new FormData(event.target);
    
    try {
        const response = await fetch('/api/login', {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json',
            },
            body: JSON.stringify({
                username: formData.get('username'),
                password: formData.get('password')
            })
        });
        
        const data = await response.json();
        
        if (response.ok) {
            localStorage.setItem('access_token', data.access_token);
            localStorage.setItem('user_info', JSON.stringify(data.user));
            document.dispatchEvent(new Event('authChanged'));
            
            if (successMsg) {
                successMsg.textContent = '登入成功！正在跳轉...';
                successMsg.classList.remove('hidden');
            }
            
            setTimeout(() => {
                window.location.href = '/';
            }, 1500);
        } else {
            if (errorMsg) {
                errorMsg.textContent = data.error || '登入失敗';
                errorMsg.classList.remove('hidden');
            }
            resetButton();
        }
    } catch (error) {
        if (errorMsg) {
            errorMsg.textContent = '網路錯誤，請稍後再試';
            errorMsg.classList.remove('hidden');
        }
        resetButton();
    }
    
    function resetButton() {
        if (submitBtn) submitBtn.disabled = false;
        if (submitText) submitText.textContent = '登入';
        if (loadingIcon) loadingIcon.classList.add('hidden');
    }
};

window.handleRegister = async function(event) {
    event.preventDefault();
    
    const submitBtn = document.getElementById('submitBtn');
    const submitText = document.getElementById('submitText');
    const loadingIcon = document.getElementById('loadingIcon');
    const errorMsg = document.getElementById('error-message');
    const successMsg = document.getElementById('success-message');
    
    if (errorMsg) errorMsg.classList.add('hidden');
    if (successMsg) successMsg.classList.add('hidden');
    
    if (submitBtn) submitBtn.disabled = true;
    if (submitText) submitText.textContent = '創建中...';
    if (loadingIcon) loadingIcon.classList.remove('hidden');
    
    const formData = new FormData(event.target);
    const password = formData.get('password');
    const confirmPassword = formData.get('confirmPassword');
    
    if (password !== confirmPassword) {
        if (errorMsg) {
            errorMsg.textContent = '密碼與確認密碼不一致';
            errorMsg.classList.remove('hidden');
        }
        resetButton();
        return;
    }
    
    try {
        const response = await fetch('/api/register', {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json',
            },
            body: JSON.stringify({
                username: formData.get('username'),
                email: formData.get('email'),
                password: password
            })
        });
        
        const data = await response.json();
        
        if (response.ok) {
            localStorage.setItem('access_token', data.access_token);
            localStorage.setItem('user_info', JSON.stringify(data.user));
            document.dispatchEvent(new Event('authChanged'));
            
            if (successMsg) {
                successMsg.textContent = '註冊成功！正在跳轉...';
                successMsg.classList.remove('hidden');
            }
            
            setTimeout(() => {
                window.location.href = '/';
            }, 1500);
        } else {
            if (errorMsg) {
                errorMsg.textContent = data.error || '註冊失敗';
                errorMsg.classList.remove('hidden');
            }
            resetButton();
        }
    } catch (error) {
        if (errorMsg) {
            errorMsg.textContent = '網路錯誤，請稍後再試';
            errorMsg.classList.remove('hidden');
        }
        resetButton();
    }
    
    function resetButton() {
        if (submitBtn) submitBtn.disabled = false;
        if (submitText) submitText.textContent = '創建帳戶';
        if (loadingIcon) loadingIcon.classList.add('hidden');
    }
};