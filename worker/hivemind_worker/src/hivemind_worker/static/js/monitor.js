$(document).ready(function() {
    // 動態檢測API地址
    const API_BASE_URL = window.location.origin;

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
            
            // 支援多任務模式
            if (data.tasks && data.tasks.length > 0) {
                // 顯示任務數量
                $('#task-id').text(`${data.task_count} 個任務運行中`);
                
                // 顯示第一個任務ID（向後相容）
                if (data.current_task_id && data.current_task_id !== "None") {
                    $('#task-id').append(` (主要: ${data.current_task_id})`);
                }
                
                // 如果存在任務列表容器，更新任務列表
                if ($('#tasks-list').length) {
                    updateTasksList(data.tasks);
                } else {
                    // 否則只顯示第一個任務的ID
                    $('#task-id').text(data.current_task_id !== "None" ? data.current_task_id : "無任務");
                }
            } else {
                $('#task-id').text("無任務");
            }
            
            // 更新狀態標籤樣式，支援新的負載狀態
            const statusElement = $('#task-status');
            const status = data.status || 'Idle';
            statusElement.text(status).removeClass();
            
            if (status.toLowerCase().includes('idle') || status.toLowerCase().includes('light load')) {
                statusElement.addClass('status idle');
            } else if (status.toLowerCase().includes('running') || status.toLowerCase().includes('medium load')) {
                statusElement.addClass('status running');
            } else if (status.toLowerCase().includes('heavy load') || status.toLowerCase().includes('full')) {
                statusElement.addClass('status error');
            } else if (status.toLowerCase().includes('error') || status.toLowerCase().includes('failed')) {
                statusElement.addClass('status error');
            } else {
                statusElement.addClass('status pending');
            }
            
            // 顯示Docker狀態
            if ($('#docker-status').length) {
                const dockerStatus = data.docker_status || (data.docker_available ? 'available' : 'unavailable');
                $('#docker-status').text(dockerStatus);
                $('#docker-status').removeClass().addClass('status ' + 
                    (dockerStatus === 'available' ? 'idle' : 'error'));
            }
            
            // 更新資源使用情況
            updateResourcesDisplay(data);
            
            $('#ip-address').text(data.ip || 'N/A');
            $('#cpt-balance').text(data.cpt_balance || 0);
            
            const cpuPercent = data.cpu_percent || 0;
            const memoryPercent = data.memory_percent || 0;
            
            $('#cpu-usage').text(cpuPercent + '%');
            $('#memory-usage').text(memoryPercent + '%');
            
            // 更新資源卡片，加入負載狀態顏色
            const cpuElement = $('#cpu-metric');
            const memoryElement = $('#memory-metric');
            
            cpuElement.text(cpuPercent + '%');
            memoryElement.text(memoryPercent + '%');
            
            // 根據負載調整顏色
            function updateLoadColor(element, percent) {
                element.removeClass('load-normal load-medium load-high');
                if (percent > 80) {
                    element.addClass('load-high');
                } else if (percent > 60) {
                    element.addClass('load-medium');
                } else {
                    element.addClass('load-normal');
                }
            }
            
            updateLoadColor(cpuElement.parent(), cpuPercent);
            updateLoadColor(memoryElement.parent(), memoryPercent);

            // 更新圖表數據
            const now = new Date().toLocaleTimeString();

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
    
    // 新增函數：更新任務列表
    function updateTasksList(tasks) {
        const tasksListEl = $('#tasks-list');
        tasksListEl.empty();

        if (!tasks || tasks.length === 0) {
            tasksListEl.html('<div class="text-center p-3">目前沒有執行中的任務</div>');
            return;
        }

        tasks.forEach(task => {
            const taskEl = $('<div>').addClass('task-item p-2 my-1 border rounded');

            // 計算執行時間
            const startTime = new Date(task.start_time);
            const now = new Date();
            const duration = Math.floor((now - startTime) / 1000); // 秒
            const hours = Math.floor(duration / 3600);
            const minutes = Math.floor((duration % 3600) / 60);
            const seconds = duration % 60;
            const durationStr = `${hours.toString().padStart(2, '0')}:${minutes.toString().padStart(2, '0')}:${seconds.toString().padStart(2, '0')}`;

            // 格式化資源
            const resources = task.resources || {};
            const resourcesStr = `CPU: ${resources.cpu || 0}, RAM: ${resources.memory_gb || 0}GB, GPU: ${resources.gpu || 0}`;

            taskEl.html(`
                <div><strong>ID:</strong> ${task.id}</div>
                <div><strong>狀態:</strong> <span class="status ${task.status === 'Executing' ? 'running' : 'pending'}">${task.status}</span></div>
                <div><strong>開始時間:</strong> ${new Date(task.start_time).toLocaleString()}</div>
                <div><strong>執行時間:</strong> ${durationStr}</div>
                <div><strong>資源:</strong> ${resourcesStr}</div>
            `);

            tasksListEl.append(taskEl);
        });
    }

    // 新增函數：更新資源顯示
    function updateResourcesDisplay(data) {
        // 如果存在資源區塊
        if ($('#resource-status').length) {
            const availableResources = data.available_resources || {};
            const totalResources = data.total_resources || {};

            // 計算資源使用百分比
            const cpuUsagePercent = totalResources.cpu ? Math.round(((totalResources.cpu - availableResources.cpu) / totalResources.cpu) * 100) : 0;
            const memoryUsagePercent = totalResources.memory_gb ? Math.round(((totalResources.memory_gb - availableResources.memory_gb) / totalResources.memory_gb) * 100) : 0;
            const gpuUsagePercent = totalResources.gpu ? Math.round(((totalResources.gpu - availableResources.gpu) / totalResources.gpu) * 100) : 0;

            // 更新進度條
            updateProgressBar('#cpu-progress', cpuUsagePercent);
            updateProgressBar('#memory-progress', memoryUsagePercent);
            updateProgressBar('#gpu-progress', gpuUsagePercent);

            // 更新數值
            $('#cpu-usage-value').text(`${totalResources.cpu - availableResources.cpu}/${totalResources.cpu} (${cpuUsagePercent}%)`);
            $('#memory-usage-value').text(`${(totalResources.memory_gb - availableResources.memory_gb).toFixed(1)}/${totalResources.memory_gb.toFixed(1)}GB (${memoryUsagePercent}%)`);
            $('#gpu-usage-value').text(`${totalResources.gpu - availableResources.gpu}/${totalResources.gpu} (${gpuUsagePercent}%)`);
        }
    }

    // 更新進度條
    function updateProgressBar(selector, percent) {
        const progressBar = $(selector);
        if (progressBar.length) {
            progressBar.css('width', percent + '%');

            // 根據百分比調整顏色
            progressBar.removeClass('bg-success bg-warning bg-danger');
            if (percent > 80) {
                progressBar.addClass('bg-danger');
            } else if (percent > 60) {
                progressBar.addClass('bg-warning');
            } else {
                progressBar.addClass('bg-success');
            }
        }
    }
});