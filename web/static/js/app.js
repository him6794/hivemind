class HiveMindApp {
    constructor() {
        this.token = localStorage.getItem('access_token');
        this.user = JSON.parse(localStorage.getItem('user') || 'null');
        this.init();
    }

    init() {
        this.updateAuthUI();
        this.setupEventListeners();
        this.checkAuthStatus();
    }

    updateAuthUI() {
        const authButtons = document.getElementById('auth-buttons');
        const userMenu = document.getElementById('user-menu');
        const userBalance = document.getElementById('user-balance');

        if (this.token && this.user) {
            authButtons.classList.add('hidden');
            userMenu.classList.remove('hidden');
            if (userBalance) {
                userBalance.textContent = `餘額: $${this.user.balance.toFixed(2)}`;
            }
        } else {
            authButtons.classList.remove('hidden');
            userMenu.classList.add('hidden');
        }
    }

    setupEventListeners() {
        // 登出按鈕
        const logoutBtn = document.getElementById('logout-btn');
        if (logoutBtn) {
            logoutBtn.addEventListener('click', () => this.logout());
        }

        // 手機菜單
        const mobileMenuBtn = document.getElementById('mobile-menu-btn');
        if (mobileMenuBtn) {
            mobileMenuBtn.addEventListener('click', () => this.toggleMobileMenu());
        }
    }

    async checkAuthStatus() {
        if (!this.token) return;

        try {
            const response = await this.apiCall('/api/balance', 'GET');
            if (response.ok) {
                const data = await response.json();
                this.user.balance = data.balance;
                localStorage.setItem('user', JSON.stringify(this.user));
                this.updateAuthUI();
            } else {
                this.logout();
            }
        } catch (error) {
            console.error('檢查認證狀態失敗:', error);
        }
    }

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

    logout() {
        localStorage.removeItem('access_token');
        localStorage.removeItem('user');
        this.token = null;
        this.user = null;
        this.updateAuthUI();
        window.location.href = '/';
    }

    toggleMobileMenu() {
        // 實現手機菜單切換
        console.log('切換手機菜單');
    }

    // 工具函數
    showNotification(message, type = 'info') {
        // 創建通知元素
        const notification = document.createElement('div');
        notification.className = `fixed top-20 right-4 z-50 p-4 rounded-lg shadow-lg max-w-sm transition-all duration-300 ${
            type === 'success' ? 'bg-green-600' :
            type === 'error' ? 'bg-red-600' :
            type === 'warning' ? 'bg-yellow-600' : 'bg-blue-600'
        }`;
        notification.innerHTML = `
            <div class="flex items-center space-x-2">
                <i class="fas ${
                    type === 'success' ? 'fa-check-circle' :
                    type === 'error' ? 'fa-exclamation-circle' :
                    type === 'warning' ? 'fa-exclamation-triangle' : 'fa-info-circle'
                }"></i>
                <span>${message}</span>
                <button class="ml-auto text-white hover:text-gray-200" onclick="this.parentElement.parentElement.remove()">
                    <i class="fas fa-times"></i>
                </button>
            </div>
        `;

        document.body.appendChild(notification);

        // 3秒後自動移除
        setTimeout(() => {
            if (notification.parentElement) {
                notification.remove();
            }
        }, 3000);
    }

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

// 初始化應用
document.addEventListener('DOMContentLoaded', () => {
    window.app = new HiveMindApp();
});

// 全局函數
window.downloadFile = async function(fileType) {
    if (!window.app.token) {
        window.app.showNotification('請先登入', 'warning');
        window.location.href = '/login';
        return;
    }

    try {
        const response = await window.app.apiCall(`/api/download/${fileType}`);
        const data = await response.json();

        if (response.ok) {
            if (fileType === 'client') {
                // 創建下載連結
                const link = document.createElement('a');
                link.href = data.download_url;
                link.download = `hivemind-client-${data.version}.zip`;
                document.body.appendChild(link);
                link.click();
                document.body.removeChild(link);
                window.app.showNotification('開始下載客戶端', 'success');
            } else if (fileType === 'vpn-config') {
                // 下載VPN配置文件
                const blob = new Blob([data.config], { type: 'text/plain' });
                const url = window.URL.createObjectURL(blob);
                const link = document.createElement('a');
                link.href = url;
                link.download = 'hivemind-vpn.conf';
                document.body.appendChild(link);
                link.click();
                document.body.removeChild(link);
                window.URL.revokeObjectURL(url);
                window.app.showNotification('VPN配置已下載', 'success');
            }
        } else {
            window.app.showNotification(data.error || '下載失敗', 'error');
        }
    } catch (error) {
        window.app.showNotification('下載失敗，請稍後再試', 'error');
    }
};

window.joinVPN = async function() {
    if (!window.app.token) {
        window.app.showNotification('請先登入', 'warning');
        window.location.href = '/login';
        return;
    }

    try {
        const response = await window.app.apiCall('/api/vpn/join', 'POST');
        const data = await response.json();

        if (response.ok) {
            window.app.showNotification('VPN配置生成成功', 'success');
            // 顯示配置或提供下載
            const modal = document.createElement('div');
            modal.className = 'fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center z-50';
            modal.innerHTML = `
                <div class="bg-gray-800 p-6 rounded-lg max-w-2xl w-full mx-4">
                    <h3 class="text-xl font-bold mb-4">VPN配置文件</h3>
                    <pre class="bg-gray-900 p-4 rounded text-sm overflow-auto max-h-60">${data.config}</pre>
                    <div class="mt-4 flex space-x-2">
                        <button onclick="downloadFile('vpn-config')" class="bg-blue-600 hover:bg-blue-700 px-4 py-2 rounded">下載配置</button>
                        <button onclick="this.closest('.fixed').remove()" class="bg-gray-600 hover:bg-gray-700 px-4 py-2 rounded">關閉</button>
                    </div>
                </div>
            `;
            document.body.appendChild(modal);
        } else {
            window.app.showNotification(data.error || '加入VPN失敗', 'error');
        }
    } catch (error) {
        window.app.showNotification('加入VPN失敗，請稍後再試', 'error');
    }
};
