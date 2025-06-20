$(document).ready(function() {
    // 固定使用工作端端口 5000
    const API_BASE_URL = 'http://127.0.0.1:5000';

    // 初始化 Chart.js 圖表
    const cpuChartCtx = document.getElementById('cpuChart').getContext('2d');
    const memoryChartCtx = document.getElementById('memoryChart').getContext('2d');

    let cpuChart = null;
    let memoryChart = null;

    try {
        // Chart.js 初始化 CPU 圖表
        cpuChart = new Chart(cpuChartCtx, {
            type: 'line',
            data: { 
                labels: [], 
                datasets: [{ 
                    label: 'CPU 使用率 (%)', 
                    data: [], 
                    borderColor: '#6366f1',
                    backgroundColor: 'rgba(99, 102, 241, 0.1)',
                    fill: true,
                    tension: 0.4,
                    borderWidth: 3,
                    pointBackgroundColor: '#6366f1',
                    pointBorderColor: '#ffffff',
                    pointBorderWidth: 2,
                    pointRadius: 5,
                    pointHoverRadius: 7
                }] 
            },
            options: { 
                responsive: true,
                maintainAspectRatio: false,
                interaction: {
                    intersect: false,
                    mode: 'index'
                },
                scales: { 
                    x: { 
                        title: { 
                            display: true, 
                            text: '時間',
                            color: '#64748b',
                            font: { weight: 'bold' }
                        },
                        grid: {
                            color: 'rgba(148, 163, 184, 0.1)'
                        },
                        ticks: {
                            color: '#64748b'
                        }
                    }, 
                    y: { 
                        title: { 
                            display: true, 
                            text: 'CPU 使用率 (%)',
                            color: '#64748b',
                            font: { weight: 'bold' }
                        }, 
                        beginAtZero: true, 
                        suggestedMax: 100,
                        grid: {
                            color: 'rgba(148, 163, 184, 0.1)'
                        },
                        ticks: {
                            color: '#64748b'
                        }
                    } 
                },
                plugins: {
                    legend: {
                        display: true,
                        position: 'top',
                        labels: {
                            usePointStyle: true,
                            padding: 20,
                            color: '#1e293b',
                            font: { weight: 'bold' }
                        }
                    },
                    tooltip: {
                        backgroundColor: 'rgba(15, 23, 42, 0.9)',
                        titleColor: '#f1f5f9',
                        bodyColor: '#f1f5f9',
                        borderColor: '#6366f1',
                        borderWidth: 1,
                        cornerRadius: 8,
                        displayColors: false
                    }
                }
            }
        });

        // Chart.js 初始化記憶體圖表
        memoryChart = new Chart(memoryChartCtx, {
            type: 'line',
            data: { 
                labels: [], 
                datasets: [{ 
                    label: '記憶體使用率 (%)', 
                    data: [], 
                    borderColor: '#8b5cf6',
                    backgroundColor: 'rgba(139, 92, 246, 0.1)',
                    fill: true,
                    tension: 0.4,
                    borderWidth: 3,
                    pointBackgroundColor: '#8b5cf6',
                    pointBorderColor: '#ffffff',
                    pointBorderWidth: 2,
                    pointRadius: 5,
                    pointHoverRadius: 7
                }] 
            },
            options: { 
                responsive: true,
                maintainAspectRatio: false,
                interaction: {
                    intersect: false,
                    mode: 'index'
                },
                scales: { 
                    x: { 
                        title: { 
                            display: true, 
                            text: '時間',
                            color: '#64748b',
                            font: { weight: 'bold' }
                        },
                        grid: {
                            color: 'rgba(148, 163, 184, 0.1)'
                        },
                        ticks: {
                            color: '#64748b'
                        }
                    }, 
                    y: { 
                        title: { 
                            display: true, 
                            text: '記憶體使用率 (%)',
                            color: '#64748b',
                            font: { weight: 'bold' }
                        }, 
                        beginAtZero: true, 
                        suggestedMax: 100,
                        grid: {
                            color: 'rgba(148, 163, 184, 0.1)'
                        },
                        ticks: {
                            color: '#64748b'
                        }
                    } 
                },
                plugins: {
                    legend: {
                        display: true,
                        position: 'top',
                        labels: {
                            usePointStyle: true,
                            padding: 20,
                            color: '#1e293b',
                            font: { weight: 'bold' }
                        }
                    },
                    tooltip: {
                        backgroundColor: 'rgba(15, 23, 42, 0.9)',
                        titleColor: '#f1f5f9',
                        bodyColor: '#f1f5f9',
                        borderColor: '#8b5cf6',
                        borderWidth: 1,
                        cornerRadius: 8,
                        displayColors: false
                    }
                }
            }
        });
        
        console.log("圖表初始化成功");
    } catch (error) {
        console.error("圖表初始化失敗:", error);
        $('.chart-container').html('<div class="text-center text-error">圖表初始化失敗，請檢查控制台日誌。</div>');
    }

    function updateStatus() {
        $.get(`${API_BASE_URL}/api/status`, function(data) {
            if (data.error) {
                console.error("Error from /api/status:", data.error);
                $('#task-status').text('Error loading status').removeClass().addClass('status error');
                return;
            }

            // 更新狀態顯示
            $('#node-id').text(data.node_id || 'N/A');
            $('#task-id').text(data.current_task_id || 'None');
            
            // 更新狀態標籤樣式
            const statusElement = $('#task-status');
            const status = data.status || 'Idle';
            statusElement.text(status).removeClass();
            
            if (status.toLowerCase().includes('idle')) {
                statusElement.addClass('status idle');
            } else if (status.toLowerCase().includes('running') || status.toLowerCase().includes('executing')) {
                statusElement.addClass('status running');
            } else if (status.toLowerCase().includes('error') || status.toLowerCase().includes('failed')) {
                statusElement.addClass('status error');
            } else {
                statusElement.addClass('status pending');
            }
            
            $('#ip-address').text(data.ip || 'N/A');
            $('#cpt-balance').text(data.cpt_balance || 0);
            
            const cpuPercent = data.cpu_percent || 0;
            const memoryPercent = data.memory_percent || 0;
            
            $('#cpu-usage').text(cpuPercent + '%');
            $('#memory-usage').text(memoryPercent + '%');
            
            // 更新資源卡片
            $('#cpu-metric').text(cpuPercent + '%');
            $('#memory-metric').text(memoryPercent + '%');
            
            const now = new Date().toLocaleTimeString();

            // 更新圖表數據
            if (cpuChart && cpuChart.data && cpuChart.data.labels) {
                cpuChart.data.labels.push(now);
                cpuChart.data.datasets[0].data.push(cpuPercent);

                if (cpuChart.data.labels.length > 20) {
                    cpuChart.data.labels.shift();
                    cpuChart.data.datasets[0].data.shift();
                }
                cpuChart.update('none');
            }

            if (memoryChart && memoryChart.data && memoryChart.data.labels) {
                memoryChart.data.labels.push(now);
                memoryChart.data.datasets[0].data.push(memoryPercent);

                if (memoryChart.data.labels.length > 20) {
                    memoryChart.data.labels.shift();
                    memoryChart.data.datasets[0].data.shift();
                }
                memoryChart.update('none');
            }

        }).fail(function(jqXHR, textStatus, errorThrown) {
            console.error("Failed to fetch /api/status:", textStatus, errorThrown);
            $('#task-status').text('Connection Error').removeClass().addClass('status error');

            if (jqXHR.status === 401) {
                console.warn("會話已過期，3秒後重新導向登入頁面");
                setTimeout(function() {
                    window.location.href = '/login';
                }, 3000);
            }
        });
    }

    function updateLogs() {
        $.get(`${API_BASE_URL}/api/logs`, function(data) {
            console.log("日誌數據:", data);
            
            if (data.error) {
                console.error("Error from /api/logs:", data.error);
                $('#logs').html(`<div class="text-error">載入日誌錯誤: ${data.error}</div>`);
                return;
            }
            
            const logsDiv = $('#logs');
            logsDiv.empty();
            
            if (data.logs && Array.isArray(data.logs)) {
                if (data.logs.length === 0) {
                    logsDiv.html('<div class="text-center" style="opacity: 0.7;">目前沒有日誌記錄</div>');
                } else {
                    data.logs.forEach(log => {
                        const logEntry = $('<div>').text(log).addClass('log-entry');
                        logsDiv.append(logEntry);
                    });
                    // 自動滾動到底部
                    logsDiv.scrollTop(logsDiv[0].scrollHeight);
                }
            } else {
                console.warn("日誌數據格式異常:", data);
                logsDiv.html('<div class="text-warning">未收到有效的日誌數據</div>');
            }
        }).fail(function(jqXHR, textStatus, errorThrown) {
            console.error("Failed to fetch /api/logs:", textStatus, errorThrown);
            $('#logs').html(`<div class="text-error">載入日誌錯誤: ${textStatus} (${jqXHR.status})</div>`);
            
            if (jqXHR.status === 401) {
                console.warn("會話已過期，3秒後重新導向登入頁面");
                setTimeout(function() {
                    window.location.href = '/login';
                }, 3000);
            }
        });
    }

    // 初始加載
    updateStatus();
    updateLogs();
    
    // 定期更新
    setInterval(updateStatus, 3000);  // 每3秒更新狀態
    setInterval(updateLogs, 5000);    // 每5秒更新日誌

    // 全局函數
    window.refreshStatus = function() {
        console.log("手動刷新狀態");
        updateStatus();
    }

    window.refreshLogs = function() {
        console.log("手動刷新日誌");
        updateLogs();
    }
});