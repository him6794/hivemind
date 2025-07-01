/* filepath: d:\hivemind\web\static\js\main.js */
class HiveMindApp {
    constructor() {
        this.token = localStorage.getItem('auth_token');
        this.user = JSON.parse(localStorage.getItem('user') || 'null');
        this.init();
    }

    init() {
        this.initTheme();
        this.updateAuthUI();
        this.setupEventListeners();
        this.checkAuthStatus();
    }

    // 主題管理
    initTheme() {
        const theme = localStorage.getItem('theme');
        const prefersDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
        
        if (theme === 'dark' || (!theme && prefersDark)) {
            this.setTheme(true);
        } else {
            this.setTheme(false);
        }
    }

    setTheme(dark) {
        const html = document.documentElement;
        const icon = document.getElementById('theme-toggle-icon');
        
        if (dark) {
            html.classList.add('dark');
            localStorage.setItem('theme', 'dark');
            if (icon) {
                icon.classList.remove('fa-moon');
                icon.classList.add('fa-sun');
            }
        } else {
            html.classList.remove('dark');
            localStorage.setItem('theme', 'light');
            if (icon) {
                icon.classList.remove('fa-sun');
                icon.classList.add('fa-moon');
            }
        }
    }

    toggleTheme() {
        const isDark = document.documentElement.classList.contains('dark');
        this.setTheme(!isDark);
    }

    // 認證UI更新
    updateAuthUI() {
        const authButtons = document.getElementById('auth-buttons');
        const userMenu = document.getElementById('user-menu');
        const usernameDisplay = document.getElementById('username-display');

        if (this.token && this.user && this.user.username) {
            if (authButtons) authButtons.classList.add('hidden');
            if (userMenu) userMenu.classList.remove('hidden');
            if (usernameDisplay) usernameDisplay.textContent = this.user.username;
        } else {
            if (authButtons) authButtons.classList.remove('hidden');
            if (userMenu) userMenu.classList.add('hidden');
        }
    }

    // 事件監聽器設定
    setupEventListeners() {
        // 主題切換按鈕
        const themeToggle = document.getElementById('theme-toggle');
        if (themeToggle) {
            themeToggle.addEventListener('click', () => this.toggleTheme());
        }

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
    async checkAuthStatus() {
        if (!this.token) return;

        try {
            const response = await this.apiCall('/api/balance', 'GET');
            if (response.ok) {
                const data = await response.json();
                if (this.user) {
                    this.user.balance = data.balance;
                    localStorage.setItem('user', JSON.stringify(this.user));
                }
                this.updateAuthUI();
            } else {
                this.logout();
            }
        } catch (error) {
            console.error('檢查認證狀態失敗:', error);
        }
    }

    // API呼叫
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

        return fetch(url, config);
    }

    // 登出
    logout() {
        localStorage.removeItem('auth_token');
        localStorage.removeItem('user');
        this.token = null;
        this.user = null;
        this.updateAuthUI();
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

window.showBalance = async function() {
    if (!app || !app.token) {
        app.showNotification('請先登入', 'warning');
        return;
    }

    try {
        const response = await app.apiCall('/api/balance');
        const data = await response.json();
        if (response.ok) {
            app.showNotification(`目前餘額: ${data.balance} HVC`, 'info');
        } else {
            app.showNotification(data.error || '獲取餘額失敗', 'error');
        }
    } catch (error) {
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
            localStorage.setItem('auth_token', data.access_token);
            localStorage.setItem('user', JSON.stringify(data.user));
            
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
            localStorage.setItem('auth_token', data.access_token);
            localStorage.setItem('user', JSON.stringify(data.user));
            
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