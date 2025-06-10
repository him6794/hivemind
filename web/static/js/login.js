// HiveMind Login - Enhanced with modern UI interactions
document.getElementById('loginForm').onsubmit = async function(e) {
    e.preventDefault();
    const username = document.getElementById('username').value;
    const password = document.getElementById('password').value;
    
    const msgElement = document.getElementById('loginMsg');
    const loginBtn = document.getElementById('loginBtn');
    
    // Reset previous messages
    msgElement.className = 'message';
    msgElement.innerText = '';
    
    // Show loading state
    loginBtn.classList.add('loading');
    loginBtn.disabled = true;
    loginBtn.innerText = '登入中...';
    
    msgElement.className = 'message warning show';
    msgElement.innerText = '正在驗證您的帳戶...';
    
    // Create abort controller for timeout
    const controller = new AbortController();
    const timeoutId = setTimeout(() => controller.abort(), 10000);
    
    try {
        const res = await fetch('/login', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ username, password }),
            signal: controller.signal
        });
        
        clearTimeout(timeoutId);
        const data = await res.json();
        
        if (res.ok && data.message === 'Login successful') {
            localStorage.setItem('username', username);
            localStorage.setItem('token', data.token || '');
            
            msgElement.className = 'message success show';
            msgElement.innerText = '登入成功！正在跳轉到用戶中心...';
            
            setTimeout(() => {
                window.location.href = '/dashboard.html';
            }, 1500);
        } else {
            msgElement.className = 'message error show';
            msgElement.innerText = data.message || '登入失敗，請檢查您的帳號密碼';
        }
    } catch (err) {
        clearTimeout(timeoutId);
        msgElement.className = 'message error show';
        
        if (err.name === 'AbortError') {
            msgElement.innerText = '請求超時，請檢查網路連接或伺服器狀態';
        } else {
            msgElement.innerText = '連接失敗，請稍後再試或聯繫技術支援';
        }
        console.error('登入錯誤:', err);
    } finally {
        // Reset button state
        loginBtn.classList.remove('loading');
        loginBtn.disabled = false;
        loginBtn.innerText = '登入平台';
    }
};
