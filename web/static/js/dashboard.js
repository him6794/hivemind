// HiveMind Dashboard - Enhanced with modern UI interactions
document.addEventListener('DOMContentLoaded', async function() {
    const username = localStorage.getItem('username');
    if (!username) {
        window.location.href = '/login.html';
        return;
    }
    
    // Display username immediately
    document.getElementById('userName').innerText = username;
    
    // Load balance with loading state
    try {
        const token = localStorage.getItem('token') || '';
        const res = await fetch(`/balance?user_id=${encodeURIComponent(username)}&token=${encodeURIComponent(token)}`);
        const data = await res.json();
        
        if (res.ok) {
            document.getElementById('balance').innerText = data.balance || 0;
        } else {
            document.getElementById('balance').innerText = '載入失敗';
            document.getElementById('balance').style.color = 'var(--error-color)';
        }
    } catch (err) {
        console.error('載入餘額失敗:', err);
        document.getElementById('balance').innerText = '連接錯誤';
        document.getElementById('balance').style.color = 'var(--error-color)';
    }
});

document.getElementById('transferForm').onsubmit = async function(e) {
    e.preventDefault();
    const username = localStorage.getItem('username');
    const token = localStorage.getItem('token') || '';
    const toUserId = document.getElementById('toUserId').value;
    const amount = parseInt(document.getElementById('amount').value);
    
    const msgElement = document.getElementById('transferMsg');
    const transferBtn = document.getElementById('transferBtn');
    
    // Reset previous messages
    msgElement.className = 'message';
    msgElement.innerText = '';
    
    // Validation
    if (!toUserId.trim()) {
        msgElement.className = 'message error show';
        msgElement.innerText = '請輸入收款人用戶名';
        return;
    }
    
    if (amount <= 0) {
        msgElement.className = 'message error show';
        msgElement.innerText = '轉帳金額必須大於 0';
        return;
    }
    
    if (toUserId.trim() === username) {
        msgElement.className = 'message error show';
        msgElement.innerText = '不能轉帳給自己';
        return;
    }
    
    // Show loading state
    transferBtn.classList.add('loading');
    transferBtn.disabled = true;
    transferBtn.innerText = '處理中...';
    
    msgElement.className = 'message warning show';
    msgElement.innerText = '正在處理您的轉帳請求...';
    
    try {
        const res = await fetch('/transfer', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ 
                token: token,
                to_user_id: toUserId.trim(), 
                amount: amount 
            })
        });
        
        const data = await res.json();
        
        if (res.ok && data.message && (data.message.includes('成功') || data.message.includes('successful'))) {
            msgElement.className = 'message success show';
            msgElement.innerText = `成功轉帳 ${amount} CPT 給 ${toUserId}`;
            
            // Clear form
            document.getElementById('toUserId').value = '';
            document.getElementById('amount').value = '';
            
            // Refresh balance after successful transfer
            setTimeout(async () => {
                try {
                    const balRes = await fetch(`/balance?user_id=${encodeURIComponent(username)}&token=${encodeURIComponent(token)}`);
                    const balData = await balRes.json();
                    if (balRes.ok) {
                        document.getElementById('balance').innerText = balData.balance || 0;
                        document.getElementById('balance').style.color = 'var(--accent-tertiary)';
                    }
                } catch (err) {
                    console.error('刷新餘額失敗:', err);
                }
            }, 1000);
            
        } else {
            msgElement.className = 'message error show';
            msgElement.innerText = data.message || '轉帳失敗，請稍後再試';
        }
    } catch (err) {
        msgElement.className = 'message error show';
        msgElement.innerText = '連接失敗，請檢查網路連接';
        console.error('轉帳錯誤:', err);
    } finally {
        // Reset button state
        transferBtn.classList.remove('loading');
        transferBtn.disabled = false;
        transferBtn.innerText = '發送 CPT';
    }
};

function logout() {
    // Clear all stored data
    localStorage.removeItem('username');
    localStorage.removeItem('token');
    
    // Show confirmation message (optional)
    const confirmed = confirm('確定要登出嗎？');
    if (confirmed) {
        window.location.href = '/login.html';
    }
}

// Add some interactive enhancements
document.addEventListener('DOMContentLoaded', function() {
    // Add hover effects to stat cards
    const statCards = document.querySelectorAll('.stat-card');
    statCards.forEach(card => {
        card.style.transition = 'transform 0.3s ease, box-shadow 0.3s ease';
        card.addEventListener('mouseenter', function() {
            this.style.transform = 'translateY(-2px)';
            this.style.boxShadow = '0 8px 25px rgba(0, 212, 255, 0.15)';
        });
        card.addEventListener('mouseleave', function() {
            this.style.transform = 'translateY(0)';
            this.style.boxShadow = 'none';
        });
    });
    
    // Add real-time form validation
    const amountInput = document.getElementById('amount');
    if (amountInput) {
        amountInput.addEventListener('input', function() {
            const value = parseInt(this.value);
            if (value <= 0 || isNaN(value)) {
                this.style.borderColor = 'var(--error-color)';
            } else {
                this.style.borderColor = 'var(--accent-primary)';
            }
        });
    }
    
    const userIdInput = document.getElementById('toUserId');
    if (userIdInput) {
        userIdInput.addEventListener('input', function() {
            const value = this.value.trim();
            const currentUser = localStorage.getItem('username');
            if (!value) {
                this.style.borderColor = 'var(--border-color)';
            } else if (value === currentUser) {
                this.style.borderColor = 'var(--error-color)';
            } else {
                this.style.borderColor = 'var(--accent-primary)';
            }
        });
    }
});
