function downloadFile(fileType, buttonElement) {
    const token = localStorage.getItem('access_token');
    if (!token) {
        alert('請先登入才能下載檔案');
        window.location.href = '/login';
        return;
    }

    // 禁用按鈕防止重複點擊
    const originalText = buttonElement.textContent;
    buttonElement.disabled = true;
    buttonElement.textContent = '下載中...';

    // 檢查是否為開發中的項目
    const developmentItems = ['server', 'mobile', 'web'];
    if (developmentItems.includes(fileType)) {
        fetch(`/api/download/${fileType}`, {
            method: 'GET',
            headers: {
                'Authorization': `Bearer ${token}`,
                'Content-Type': 'application/json'
            }
        })
        .then(response => response.json())
        .then(data => {
            if (data.status === 'development') {
                alert(`${data.message}\n預計發布時間: ${data.estimated_release || '待定'}`);
            } else {
                alert(data.error || '未知錯誤');
            }
        })
        .catch(error => {
            console.error('Error:', error);
            alert('請求失敗');
        })
        .finally(() => {
            buttonElement.disabled = false;
            buttonElement.textContent = originalText;
        });
        return;
    }

    // 顯示進度條
    let progressBar = document.getElementById('download-progress-bar');
    let progressWrapper = document.getElementById('download-progress-wrapper');
    if (!progressBar) {
        progressWrapper = document.createElement('div');
        progressWrapper.id = 'download-progress-wrapper';
        progressWrapper.className = 'fixed top-8 right-8 z-50 w-80 bg-white dark:bg-neutral-800 border border-neutral-200 dark:border-neutral-700 rounded-lg shadow-lg p-4 flex flex-col items-center transition-theme';
        
        progressBar = document.createElement('div');
        progressBar.id = 'download-progress-bar';
        progressBar.className = 'w-full h-4 bg-neutral-200 dark:bg-neutral-700 rounded-full overflow-hidden mb-2';
        
        const bar = document.createElement('div');
        bar.className = 'h-full w-0 bg-green-500 transition-all duration-300';
        bar.id = 'download-progress-inner';
        progressBar.appendChild(bar);
        
        const percent = document.createElement('span');
        percent.id = 'download-progress-percent';
        percent.className = 'text-sm font-medium text-green-600 dark:text-green-400';
        percent.textContent = '0%';
        
        progressWrapper.appendChild(progressBar);
        progressWrapper.appendChild(percent);
        document.body.appendChild(progressWrapper);
    } else {
        progressWrapper.style.display = 'flex';
        document.getElementById('download-progress-inner').style.width = '0%';
        document.getElementById('download-progress-percent').textContent = '0%';
    }

    // 使用 XMLHttpRequest 下載並顯示進度
    const xhr = new XMLHttpRequest();
    xhr.open('GET', `/api/download/${fileType}`, true);
    xhr.setRequestHeader('Authorization', `Bearer ${token}`);
    xhr.responseType = 'blob';

    xhr.onprogress = function (event) {
        if (event.lengthComputable) {
            const percent = Math.round((event.loaded / event.total) * 100);
            document.getElementById('download-progress-inner').style.width = percent + '%';
            document.getElementById('download-progress-percent').textContent = percent + '%';
        }
    };

    xhr.onload = function () {
        progressWrapper.style.display = 'none';
        if (xhr.status === 200) {
            // 取得檔案名
            let filename = `HiveMind-${fileType}.zip`;
            const disposition = xhr.getResponseHeader('content-disposition');
            if (disposition) {
                const matches = disposition.match(/filename="?([^"]+)"?/);
                if (matches) filename = matches[1];
            }
            const blob = xhr.response;
            const url = window.URL.createObjectURL(blob);
            const a = document.createElement('a');
            a.href = url;
            a.download = filename;
            document.body.appendChild(a);
            a.click();
            window.URL.revokeObjectURL(url);
            document.body.removeChild(a);
            showDownloadSuccess(filename);
        } else {
            // 嘗試解析錯誤訊息
            const reader = new FileReader();
            reader.onload = function() {
                try {
                    const data = JSON.parse(reader.result);
                    alert(data.error || '下載失敗');
                } catch {
                    alert('下載失敗');
                }
            };
            reader.readAsText(xhr.response);
        }
        buttonElement.disabled = false;
        buttonElement.textContent = originalText;
    };

    xhr.onerror = function () {
        progressWrapper.style.display = 'none';
        alert('下載失敗，請稍後再試');
        buttonElement.disabled = false;
        buttonElement.textContent = originalText;
    };

    xhr.send();
}

function showDownloadSuccess(filename) {
    // 創建成功提示
    const notification = document.createElement('div');
    notification.className = 'download-notification';
    notification.innerHTML = `
        <div class="notification-content">
            <i class="fas fa-check-circle"></i>
            <span>檔案下載成功: ${filename}</span>
        </div>
    `;
    
    document.body.appendChild(notification);
    
    setTimeout(() => {
        notification.classList.add('show');
    }, 100);
    
    setTimeout(() => {
        notification.classList.remove('show');
        setTimeout(() => {
            document.body.removeChild(notification);
        }, 300);
    }, 3000);
}

// 添加通知樣式
const style = document.createElement('style');
style.textContent = `
.download-notification {
    position: fixed;
    top: 20px;
    right: 20px;
    background: #059669;
    color: white;
    padding: 15px 20px;
    border-radius: 8px;
    box-shadow: 0 4px 12px rgba(0,0,0,0.2);
    transform: translateX(100%);
    transition: transform 0.3s ease;
    z-index: 10000;
}

.download-notification.show {
    transform: translateX(0);
}

.notification-content {
    display: flex;
    align-items: center;
    gap: 10px;
}

.notification-content i {
    font-size: 18px;
}
`;
document.head.appendChild(style);
