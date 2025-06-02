$(document).ready(function() {
    // 初始化 Chart.js 圖表 (保持不變)
    const cpuChartCtx = document.getElementById('cpuChart').getContext('2d');
    const memoryChartCtx = document.getElementById('memoryChart').getContext('2d');

    let cpuChart = null; // 初始化為 null
    let memoryChart = null; // 初始化為 null

    try {
        cpuChart = new Chart(cpuChartCtx, {
            type: 'line',
            data: { labels: [], datasets: [{ label: 'CPU 使用率 (%)', data: [], borderColor: 'rgba(75, 192, 192, 1)', fill: false }] },
            options: { scales: { x: { title: { display: true, text: '時間' } }, y: { title: { display: true, text: 'CPU 使用率 (%)' }, beginAtZero: true, suggestedMax: 100 } } } // 添加 suggestedMax
        });

        memoryChart = new Chart(memoryChartCtx, {
            type: 'line',
            data: { labels: [], datasets: [{ label: '記憶體使用率 (%)', data: [], borderColor: 'rgba(255, 99, 132, 1)', fill: false }] }, // 修改 Label
            options: { scales: { x: { title: { display: true, text: '時間' } }, y: { title: { display: true, text: '記憶體使用率 (%)' }, beginAtZero: true, suggestedMax: 100 } } } // 修改 Y 軸標題和添加 suggestedMax
        });
    } catch (error) {
        console.error("Failed to initialize charts:", error);
        // 可以在頁面上顯示錯誤提示
        $('.chart-container').text('圖表初始化失敗，請檢查控制台日誌。');
    }


    function updateStatus() {
        $.get('/api/status', function(data) {
            if (data.error) {
                console.error("Error from /api/status:", data.error);
                $('#statusData').text(`Error loading status: ${data.error}`);
                return;
            }

            // 更新文本內容
            $('#container-id').text(data.container_id || 'N/A');
            $('#task-id').text(data.current_task_id || 'None');
            $('#task-status').text(data.status || 'Idle');
            // 顯示本機 IP
            $('#ip-address').text(data.ip || 'N/A');
            
            // 更新 CPT 余额，添加单位显示
            const cptBalance = data.cpt_balance || 0;
            $('#cpt-balance').text(`${cptBalance} CPT`);
            
            $('#cpu-usage').text((data.cpu_percent || '0') + '%');
            $('#memory-usage').text((data.memory_percent || '0') + '%');

            const now = new Date().toLocaleTimeString();

            // --- 添加防禦性檢查 ---
            if (cpuChart && cpuChart.data && cpuChart.data.labels && cpuChart.data.datasets && cpuChart.data.datasets[0].data) {
                cpuChart.data.labels.push(now);
                cpuChart.data.datasets[0].data.push(data.cpu_percent || 0);

                if (cpuChart.data.labels.length > 20) {
                    cpuChart.data.labels.shift();
                    cpuChart.data.datasets[0].data.shift();
                }
                cpuChart.update();
            } else {
                console.warn("CPU Chart object or its properties are not ready for update.");
            }

            if (memoryChart && memoryChart.data && memoryChart.data.labels && memoryChart.data.datasets && memoryChart.data.datasets[0].data) {
                memoryChart.data.labels.push(now);
                memoryChart.data.datasets[0].data.push(data.memory_percent || 0);

                if (memoryChart.data.labels.length > 20) {
                    memoryChart.data.labels.shift();
                    memoryChart.data.datasets[0].data.shift();
                }
                memoryChart.update();
            } else {
                console.warn("Memory Chart object or its properties are not ready for update.");
            }
            // --- 檢查結束 ---

        }).fail(function(jqXHR, textStatus, errorThrown) {
            console.error("Failed to fetch /api/status:", textStatus, errorThrown);
            console.error("Response status:", jqXHR.status);
            $('#statusData').text(`Error loading status: ${textStatus} (${jqXHR.status})`);

            // Claude 建議的檢查
            if (jqXHR.status === 0 && jqXHR.readyState === 0) {
                console.warn("Possible redirect due to session expiration");
                setTimeout(function() {
                    // Check if we're now at the login page
                    if (window.location.pathname === '/' || window.location.pathname === '/login') {
                        alert("Session expired. Please log in again.");
                        window.location.href = '/login'; // 確保重定向
                    }
                }, 500);
            } else if (jqXHR.status === 401) { // 保留原有的 401 處理
                alert("Session expired or invalid. Please log in again.");
                window.location.href = '/login';
            }
        });
    }

    function updateLogs() {
        $.get('/api/logs', function(data) {
            if (data.error) {
                console.error("Error from /api/logs:", data.error);
                $('#logs').text(`Error loading logs: ${data.error}`);
                return;
            }
            const logsDiv = $('#logs');
            logsDiv.empty();
            if (data.logs && Array.isArray(data.logs)) {
                data.logs.forEach(log => {
                    logsDiv.append($('<div>').text(log));
                });
            } else {
                logsDiv.text("No logs received or invalid format.");
            }
        }).fail(function(jqXHR, textStatus, errorThrown) {
            console.error("Failed to fetch /api/logs:", textStatus, errorThrown);
            console.error("Response status:", jqXHR.status);
            $('#logs').text(`Error loading logs: ${textStatus} (${jqXHR.status})`);
            if (jqXHR.status === 401) {
                alert("Session expired or invalid. Please log in again.");
                window.location.href = '/login';
            }
        });
    }

    // 確保圖表初始化後再開始更新
    if (cpuChart && memoryChart) {
        updateStatus();
        updateLogs();
        setInterval(updateStatus, 2000);
        setInterval(updateLogs, 5000);
    } else {
        console.error("Charts were not initialized correctly. Status and log updates will not run.");
        // 可能在此處顯示更明顯的錯誤給用戶
    }

    // 添加刷新按钮功能
    window.refreshStatus = function() {
        updateStatus();
    }

    window.refreshLogs = function() {
        updateLogs();
    }
});