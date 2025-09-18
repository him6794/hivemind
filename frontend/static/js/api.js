/**
 * Frontend API Client for HiveMind
 * Handles communication with the FastAPI backend
 */

class HiveMindAPI {
    constructor() {
        this.baseURL = window.API_BASE_URL || 'https://hivemindapi.justin0711.com';
        this.token = this.getStoredToken();
    }

    getStoredToken() {
        return localStorage.getItem('hivemind_token');
    }

    setToken(token) {
        this.token = token;
        if (token) {
            localStorage.setItem('hivemind_token', token);
        } else {
            localStorage.removeItem('hivemind_token');
        }
    }

    getAuthHeaders() {
        const headers = {
            'Content-Type': 'application/json',
        };
        
        if (this.token) {
            headers['Authorization'] = `Bearer ${this.token}`;
        }
        
        return headers;
    }

    async makeRequest(endpoint, options = {}) {
        const url = `${this.baseURL}${endpoint}`;
        const requestOptions = {
            headers: this.getAuthHeaders(),
            ...options,
        };

        if (options.body && typeof options.body === 'object') {
            requestOptions.body = JSON.stringify(options.body);
        }

        try {
            const response = await fetch(url, requestOptions);
            const data = await response.json();
            
            if (!response.ok) {
                throw new Error(data.detail || `HTTP ${response.status}`);
            }
            
            return data;
        } catch (error) {
            console.error('API Request failed:', error);
            throw error;
        }
    }

    // Authentication endpoints
    async register(username, password, turnstileResponse = null) {
        const data = await this.makeRequest('/api/register', {
            method: 'POST',
            body: {
                username,
                password,
                cf_turnstile_response: turnstileResponse
            }
        });
        
        if (data.access_token) {
            this.setToken(data.access_token);
        }
        
        return data;
    }

    async login(username, password, turnstileResponse = null) {
        const data = await this.makeRequest('/api/login', {
            method: 'POST',
            body: {
                username,
                password,
                cf_turnstile_response: turnstileResponse
            }
        });
        
        if (data.access_token) {
            this.setToken(data.access_token);
        }
        
        return data;
    }

    async logout() {
        this.setToken(null);
        window.location.href = '/';
    }

    // User endpoints
    async getBalance() {
        return await this.makeRequest('/api/balance');
    }

    async transfer(receiverUsername, amount) {
        return await this.makeRequest('/api/transfer', {
            method: 'POST',
            body: {
                receiver_username: receiverUsername,
                amount,
                token: this.token
            }
        });
    }

    // System endpoints
    async getSystemHealth() {
        return await this.makeRequest('/api/health');
    }

    // Utility methods
    isAuthenticated() {
        return !!this.token;
    }
}

// Create global API instance
window.hivemindAPI = new HiveMindAPI();

// Authentication state management
function updateAuthUI() {
    const authButtons = document.getElementById('auth-buttons');
    if (!authButtons) return;
    
    if (window.hivemindAPI.isAuthenticated()) {
        authButtons.innerHTML = `
            <a class="btn btn-outline-light btn-sm" href="/dashboard.html" data-i18n="dashboard">控制台</a>
            <button class="btn btn-light btn-sm ms-1" onclick="window.hivemindAPI.logout()" data-i18n="logout">登出</button>
        `;
    } else {
        authButtons.innerHTML = `
            <a class="btn btn-outline-light btn-sm" href="/login.html" data-i18n="login">登入</a>
            <a class="btn btn-light btn-sm ms-1" href="/register.html" data-i18n="register">註冊</a>
        `;
    }
}

// Initialize on page load
document.addEventListener('DOMContentLoaded', function() {
    updateAuthUI();
    
    // Update i18n if available
    if (typeof updateI18n === 'function') {
        updateI18n();
    }
});