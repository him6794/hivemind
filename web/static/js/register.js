// HiveMind Register - Enhanced with modern UI interactions
document.getElementById('registerForm').onsubmit = async function(e) {
    e.preventDefault();
    const username = document.getElementById('username').value;
    const password = document.getElementById('password').value;
    const confirmPassword = document.getElementById('confirmPassword').value;
    
    const msgElement = document.getElementById('registerMsg');
    const registerBtn = document.getElementById('registerBtn');
    
    // Reset previous messages
    msgElement.className = 'message';
    msgElement.innerText = '';
    
    // Validate password confirmation
    if (password !== confirmPassword) {
        msgElement.className = 'message error show';
        msgElement.innerText = '密碼確認不一致，請重新檢查';
        return;
    }
    
    // Validate password strength
    if (password.length < 6) {
        msgElement.className = 'message error show';
        msgElement.innerText = '密碼長度至少需要 6 個字符';
        return;
    }
    
    // Show loading state
    registerBtn.classList.add('loading');
    registerBtn.disabled = true;
    registerBtn.innerText = '創建中...';
    
    msgElement.className = 'message warning show';
    msgElement.innerText = '正在創建您的帳戶...';
    
    try {
        const res = await fetch('/register', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ username, password })
        });
        
        const data = await res.json();
        
        if (res.ok && data.message === 'Registration successful') {
            msgElement.className = 'message success show';
            msgElement.innerText = '註冊成功！正在跳轉到登入頁面...';
            
            setTimeout(() => {
                window.location.href = '/login.html';
            }, 2000);
        } else {
            msgElement.className = 'message error show';
            msgElement.innerText = data.message || '註冊失敗，請稍後再試';
        }
    } catch (err) {
        msgElement.className = 'message error show';
        msgElement.innerText = '連接失敗，請檢查網路連接';
        console.error('註冊錯誤:', err);
    } finally {
        // Reset button state
        registerBtn.classList.remove('loading');
        registerBtn.disabled = false;
        registerBtn.innerText = '創建帳戶';
    }
};
