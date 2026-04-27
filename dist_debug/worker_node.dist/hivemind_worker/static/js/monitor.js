document.addEventListener('DOMContentLoaded', function(){
    // Dynamically detect API address
    const API_BASE_URL = window.location.origin;

    // Initialize Chart.js charts if available
    const cpuCanvas = document.getElementById('cpuChart');
    const memoryCanvas = document.getElementById('memoryChart');
    const netCanvas = document.getElementById('netChart');
    const gpuUtilCanvas = document.getElementById('gpuUtilChart');
    const gpuMemCanvas = document.getElementById('gpuMemChart');
    const cpuChartCtx = cpuCanvas ? cpuCanvas.getContext('2d') : null;
    const memoryChartCtx = memoryCanvas ? memoryCanvas.getContext('2d') : null;
    const netChartCtx = netCanvas ? netCanvas.getContext('2d') : null;
    const gpuUtilChartCtx = gpuUtilCanvas ? gpuUtilCanvas.getContext('2d') : null;
    const gpuMemChartCtx = gpuMemCanvas ? gpuMemCanvas.getContext('2d') : null;

    let cpuChart = null;
    let memoryChart = null;
    let netChart = null;
    let gpuUtilChart = null;
    let gpuMemChart = null;

    // Dark theme colors from CSS variables
    const cssVars = getComputedStyle(document.documentElement);
    const gridColor = 'rgba(148, 163, 184, 0.15)';
    const axisColor = cssVars.getPropertyValue('--text-secondary').trim() || '#9ca3af';
    const legendColor = cssVars.getPropertyValue('--text-primary').trim() || '#e5e7eb';

    try {
        if (window.Chart && cpuChartCtx) {
        // Chart.js initialize CPU chart
        cpuChart = new Chart(cpuChartCtx, {
            type: 'line',
            data: { 
                labels: [], 
                datasets: [{ 
                    label: (window.i18n? i18n.t('chart.cpu') : 'CPU Usage (%)'), 
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
                            text: (window.i18n? i18n.t('chart.time') : 'Time'),
                            color: axisColor,
                            font: { weight: 'bold' }
                        },
                        grid: { color: gridColor },
                        ticks: { color: axisColor }
                    }, 
                    y: { 
                        title: { 
                            display: true, 
                            text: (window.i18n? i18n.t('chart.cpu') : 'CPU Usage (%)'),
                            color: axisColor,
                            font: { weight: 'bold' }
                        }, 
                        beginAtZero: true, 
                        suggestedMax: 100,
                        grid: { color: gridColor },
                        ticks: { color: axisColor }
                    } 
                },
                plugins: {
                    legend: {
                        display: true,
                        position: 'top',
                        labels: {
                            usePointStyle: true,
                            padding: 20,
                            color: legendColor,
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
        }
        if (window.Chart && memoryChartCtx) {
        // Chart.js initialize memory chart
        memoryChart = new Chart(memoryChartCtx, {
            type: 'line',
            data: { 
                labels: [], 
                datasets: [{ 
                    label: (window.i18n? i18n.t('chart.memory') : 'Memory Usage (%)'), 
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
                            text: (window.i18n? i18n.t('chart.time') : 'Time'),
                            color: axisColor,
                            font: { weight: 'bold' }
                        },
                        grid: { color: gridColor },
                        ticks: { color: axisColor }
                    }, 
                    y: { 
                        title: { 
                            display: true, 
                            text: (window.i18n? i18n.t('chart.memory') : 'Memory Usage (%)'),
                            color: axisColor,
                            font: { weight: 'bold' }
                        }, 
                        beginAtZero: true, 
                        suggestedMax: 100,
                        grid: { color: gridColor },
                        ticks: { color: axisColor }
                    } 
                },
                plugins: {
                    legend: {
                        display: true,
                        position: 'top',
                        labels: {
                            usePointStyle: true,
                            padding: 20,
                            color: legendColor,
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
        }

        // Network chart (Rx/Tx Mbps)
        if (window.Chart && netChartCtx) {
            netChart = new Chart(netChartCtx, {
                type: 'line',
                data: {
                    labels: [],
                    datasets: [
                        { label: 'Rx (Mbps)', data: [], borderColor: '#22d3ee', backgroundColor: 'rgba(34,211,238,0.1)', fill: true, tension: 0.4, borderWidth: 2 },
                        { label: 'Tx (Mbps)', data: [], borderColor: '#f472b6', backgroundColor: 'rgba(244,114,182,0.1)', fill: true, tension: 0.4, borderWidth: 2 }
                    ]
                },
                options: {
                    responsive: true,
                    maintainAspectRatio: false,
                    interaction: { intersect: false, mode: 'index' },
                    scales: {
                        x: { ticks: { color: axisColor }, grid: { color: gridColor } },
                        y: { beginAtZero: true, ticks: { color: axisColor }, grid: { color: gridColor } }
                    },
                    plugins: { legend: { labels: { color: legendColor } } }
                }
            });
        }

        // GPU Util chart
        if (window.Chart && gpuUtilChartCtx) {
            gpuUtilChart = new Chart(gpuUtilChartCtx, {
                type: 'line',
                data: { labels: [], datasets: [{ label: 'GPU Util (%)', data: [], borderColor: '#34d399', backgroundColor: 'rgba(52,211,153,0.1)', fill: true, tension: 0.4, borderWidth: 2 }] },
                options: { responsive: true, maintainAspectRatio: false, scales: { x: { ticks: { color: axisColor }, grid: { color: gridColor } }, y: { beginAtZero: true, suggestedMax: 100, ticks: { color: axisColor }, grid: { color: gridColor } } }, plugins: { legend: { labels: { color: legendColor } } } }
            });
        }

        // GPU Memory chart
        if (window.Chart && gpuMemChartCtx) {
            gpuMemChart = new Chart(gpuMemChartCtx, {
                type: 'line',
                data: { labels: [], datasets: [{ label: 'GPU Mem Used (GB)', data: [], borderColor: '#fbbf24', backgroundColor: 'rgba(251,191,36,0.1)', fill: true, tension: 0.4, borderWidth: 2 }] },
                options: { responsive: true, maintainAspectRatio: false, scales: { x: { ticks: { color: axisColor }, grid: { color: gridColor } }, y: { beginAtZero: true, ticks: { color: axisColor }, grid: { color: gridColor } } }, plugins: { legend: { labels: { color: legendColor } } } }
            });
        }
        
        console.log("Charts initialized successfully");
    } catch (error) {
        console.error("Chart initialization failed:", error);
        document.querySelectorAll('.chart-container').forEach(el => {
          el.innerHTML = '<div class="text-center text-error">Chart initialization failed, please check the console logs.</div>';
        });
    }

        async function updateStatus() {
                try{
                    const res = await fetch(`${API_BASE_URL}/api/status`);
                    if(res.status === 401){
                        // 未授權：導向登入頁
                        setTimeout(()=>{ window.location.href = '/login'; }, 500);
                        return;
                    }
                    const data = await res.json();
            if (data.error) {
                console.error("Error from /api/status:", data.error);
                const el = document.getElementById('task-status');
                if(el){ el.textContent = (window.i18n? i18n.t('monitor.error_loading_status') : 'Error loading status'); el.className = 'status error'; }
                return;
            }

            // Update status display
            const nodeIdEl = document.getElementById('node-id');
            if(nodeIdEl) nodeIdEl.textContent = data.node_id || 'N/A';
            
            // Support multi-task mode
            if (data.tasks && data.tasks.length > 0) {
                // Display task count
                const taskIdEl = document.getElementById('task-id');
                if(taskIdEl){ taskIdEl.textContent = (window.i18n? i18n.t('monitor.tasks_running',{n: data.task_count}) : `${data.task_count} tasks running`); }
                
                // Display the first task ID (for backward compatibility)
                if (taskIdEl && data.current_task_id && data.current_task_id !== "None") {
                    taskIdEl.textContent += ` (${window.i18n? i18n.t('monitor.main',{id: data.current_task_id}) : 'Main: ' + data.current_task_id})`;
                }
                
                // If the task list container exists, update the task list
                if (document.getElementById('tasks-list')) {
                    updateTasksList(data.tasks);
                } else {
                    // Otherwise, only display the ID of the first task
                    if(taskIdEl) taskIdEl.textContent = data.current_task_id !== "None" ? data.current_task_id : (window.i18n? i18n.t('monitor.no_tasks') : 'no tasks');
                }
            } else {
                const taskIdEl = document.getElementById('task-id');
                if(taskIdEl) taskIdEl.textContent = (window.i18n? i18n.t('monitor.no_tasks') : 'no tasks');
            }
            
            // Update status label style, support new load states
            const statusElement = document.getElementById('task-status');
            const status = data.status || 'Idle';
            if(statusElement){ statusElement.textContent = status; statusElement.className = ''; }
            
            if(statusElement){
              if (status.toLowerCase().includes('idle') || status.toLowerCase().includes('light load')) {
                  statusElement.className = 'status idle';
              } else if (status.toLowerCase().includes('running') || status.toLowerCase().includes('medium load')) {
                  statusElement.className = 'status running';
              } else if (status.toLowerCase().includes('heavy load') || status.toLowerCase().includes('full')) {
                  statusElement.className = 'status error';
              } else if (status.toLowerCase().includes('error') || status.toLowerCase().includes('failed')) {
                  statusElement.className = 'status error';
              } else {
                  statusElement.className = 'status pending';
              }
            }
            
            // Display Docker status
            if (document.getElementById('docker-status')) {
                const dockerStatus = data.docker_status || (data.docker_available ? 'available' : 'unavailable');
                const dockerEl = document.getElementById('docker-status');
                dockerEl.textContent = dockerStatus;
                dockerEl.className = 'status ' + (dockerStatus === 'available' ? 'idle' : 'error');
            }
            
            // Update resource usage
            updateResourcesDisplay(data);
            
            const ipEl = document.getElementById('ip-address'); if(ipEl) ipEl.textContent = data.ip || 'N/A';
            const cptEl = document.getElementById('cpt-balance'); if(cptEl) cptEl.textContent = data.cpt_balance || 0;
            
            const cpuPercent = data.cpu_percent || 0;
            const memoryPercent = data.memory_percent || 0;
            
            const cpuUsageEl = document.getElementById('cpu-usage'); if(cpuUsageEl) cpuUsageEl.textContent = cpuPercent + '%';
            const memUsageEl = document.getElementById('memory-usage'); if(memUsageEl) memUsageEl.textContent = memoryPercent + '%';
            
            // Update resource cards with load status colors
            const cpuElement = document.getElementById('cpu-metric');
            const memoryElement = document.getElementById('memory-metric');
            
            if(cpuElement) cpuElement.textContent = cpuPercent + '%';
            if(memoryElement) memoryElement.textContent = memoryPercent + '%';
            
            // Adjust color based on load
            function updateLoadColor(element, percent) {
                if(!element) return;
                element.classList.remove('load-normal','load-medium','load-high');
                if (percent > 80) element.classList.add('load-high');
                else if (percent > 60) element.classList.add('load-medium');
                else element.classList.add('load-normal');
            }
            
            updateLoadColor(cpuElement && cpuElement.parentElement, cpuPercent);
            updateLoadColor(memoryElement && memoryElement.parentElement, memoryPercent);

            // Update chart data
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

        }catch(e){
          console.error('Failed to fetch /api/status:', e);
          const el = document.getElementById('task-status');
          if(el){ el.textContent = (window.i18n? i18n.t('monitor.connection_error') : 'Connection Error'); el.className = 'status error'; }
        }
    }

    async function updateLogs() {
                const logsDiv = document.getElementById('logs');
                try{
                    const res = await fetch(`${API_BASE_URL}/api/logs`);
                    if(res.status === 401){
                        setTimeout(()=>{ window.location.href = '/login'; }, 500);
                        return;
                    }
                    const data = await res.json();
          if (data.error) {
              console.error("Error from /api/logs:", data.error);
              if(logsDiv){ logsDiv.innerHTML = `<div class="text-error">${window.i18n? i18n.t('monitor.error_loading_logs',{msg: data.error}) : 'Error loading logs: ' + data.error}</div>`; }
              return;
          }

          if (!logsDiv) return;
          logsDiv.innerHTML = '';

          if (data.logs && Array.isArray(data.logs)) {
              if (data.logs.length === 0) {
                  logsDiv.innerHTML = `<div class="text-center" style="opacity: 0.7;">${window.i18n? i18n.t('monitor.no_logs') : 'Currently no log records'}</div>`;
              } else {
                  data.logs.forEach(log => {
                      const entry = document.createElement('div');
                      entry.className = 'log-entry';
                      entry.textContent = log;
                      logsDiv.appendChild(entry);
                  });
                  logsDiv.scrollTop = logsDiv.scrollHeight;
              }
          } else {
              console.warn("Invalid log data format:", data);
              logsDiv.innerHTML = `<div class="text-warning">${window.i18n? i18n.t('monitor.invalid_logs') : 'Did not receive valid log data'}</div>`;
          }
        }catch(e){
          console.error('Failed to fetch /api/logs:', e);
          if(logsDiv){ logsDiv.innerHTML = `<div class=\"text-error\">${window.i18n? i18n.t('monitor.error_loading_logs',{msg: e.message}) : ('Error loading logs: ' + e.message)}</div>`; }
        }
    }

    // Fetch realtime metrics for charts
    async function fetchMetrics(){
        try{
            const res = await fetch(`${API_BASE_URL}/api/metrics`);
            if(!res.ok) return;
            const m = await res.json();
            const now = new Date().toLocaleTimeString();

            // Update network
            if(netChart){
                netChart.data.labels.push(now);
                netChart.data.datasets[0].data.push(m.network?.rx_mbps ?? 0);
                netChart.data.datasets[1].data.push(m.network?.tx_mbps ?? 0);
                if(netChart.data.labels.length > 20){ netChart.data.labels.shift(); netChart.data.datasets.forEach(d=>d.data.shift()); }
                netChart.update('none');
            }

            // GPU util
            if(gpuUtilChart){
                const util = (m.gpu && m.gpu.util_percent != null) ? m.gpu.util_percent : 0;
                gpuUtilChart.data.labels.push(now);
                gpuUtilChart.data.datasets[0].data.push(util);
                if(gpuUtilChart.data.labels.length > 20){ gpuUtilChart.data.labels.shift(); gpuUtilChart.data.datasets[0].data.shift(); }
                gpuUtilChart.update('none');
            }

            // GPU mem
            if(gpuMemChart){
                const used = (m.gpu && m.gpu.mem_used_gb != null) ? m.gpu.mem_used_gb : 0;
                gpuMemChart.data.labels.push(now);
                gpuMemChart.data.datasets[0].data.push(used);
                if(gpuMemChart.data.labels.length > 20){ gpuMemChart.data.labels.shift(); gpuMemChart.data.datasets[0].data.shift(); }
                gpuMemChart.update('none');
            }

            // Tasks grid
            if(Array.isArray(m.tasks)){
                updateTasksGrid(m.tasks);
            }
        }catch(e){
            // swallow
        }
    }

    // Initial load
    updateStatus();
    updateLogs();
    fetchMetrics();
    
    // Regular updates
    setInterval(updateStatus, 3000);  // Update status every 3 seconds
    setInterval(updateLogs, 5000);    // Update logs every 5 seconds
    setInterval(fetchMetrics, 3000);  // Update metrics every 3 seconds

    // Global functions
    window.refreshStatus = function() {
        console.log("Manually refreshing status");
        updateStatus();
    }

    window.refreshLogs = function() {
        console.log("Manually refreshing logs");
        updateLogs();
    }
    
    // New function: update task list (legacy list id)
    function updateTasksList(tasks) {
        const tasksListEl = document.getElementById('tasks-list');
        if(!tasksListEl) return;
        tasksListEl.innerHTML = '';

        if (!tasks || tasks.length === 0) {
            tasksListEl.innerHTML = `<div class="text-center p-3">${window.i18n? i18n.t('monitor.no_tasks') : 'Currently no tasks are running'}</div>`;
            return;
        }

        tasks.forEach(task => {
            const taskEl = document.createElement('div');
            taskEl.className = 'task-item p-2 my-1 border rounded';

            // Calculate execution time
            const startTime = new Date(task.start_time);
            const now = new Date();
            const duration = Math.floor((now - startTime) / 1000); // seconds
            const hours = Math.floor(duration / 3600);
            const minutes = Math.floor((duration % 3600) / 60);
            const seconds = duration % 60;
            const durationStr = `${hours.toString().padStart(2, '0')}:${minutes.toString().padStart(2, '0')}:${seconds.toString().padStart(2, '0')}`;

            // Format resources
            const resources = task.resources || {};
            const resourcesStr = `CPU: ${resources.cpu || 0}, RAM: ${resources.memory_gb || 0}GB, GPU: ${resources.gpu || 0}`;

            taskEl.innerHTML = `
                <div><strong>ID:</strong> ${task.id}</div>
                <div><strong>Status:</strong> <span class="status ${task.status === 'Executing' ? 'running' : 'pending'}">${task.status}</span></div>
                <div><strong>Start Time:</strong> ${new Date(task.start_time).toLocaleString()}</div>
                <div><strong>Execution Time:</strong> ${durationStr}</div>
                <div><strong>Resources:</strong> ${resourcesStr}</div>
            `;

            tasksListEl.appendChild(taskEl);
        });
    }

    // New function: grid-style task cards
    function updateTasksGrid(tasks){
        const wrap = document.getElementById('tasks');
        if(!wrap) return;
        if(!tasks || tasks.length === 0){ wrap.innerHTML = `<div class="muted">${window.i18n? i18n.t('monitor.no_tasks'): 'No tasks'}</div>`; return; }
        wrap.innerHTML = '';
        tasks.forEach(task=>{
            const card = document.createElement('div');
            card.className = 'task-card';
            const statusClass = (task.status||'').toLowerCase().includes('execut') || (task.status||'').toLowerCase().includes('running') ? 'running' : (task.status||'').toLowerCase().includes('error') ? 'error' : 'pending';
            const res = task.resources || {};
            card.innerHTML = `
                <div class="task-header">
                    <div class="task-id">${task.task_id || task.id || ''}</div>
                    <div class="task-status status ${statusClass}">${task.status||'Unknown'}</div>
                </div>
                <div class="resource-bar">
                    <div class="resource-label"><span>CPU</span><span>${res.cpu??0}</span></div>
                    <div class="progress-bar"><div class="progress-fill low" style="width:${Math.min(100, (res.cpu||0)*10)}%"></div></div>
                </div>
                <div class="resource-bar">
                    <div class="resource-label"><span>MEM</span><span>${res.memory_gb??0} GB</span></div>
                    <div class="progress-bar"><div class="progress-fill medium" style="width:${Math.min(100, ((res.memory_gb||0)/ (res.memory_gb||1))*50)}%"></div></div>
                </div>
                <div class="resource-bar">
                    <div class="resource-label"><span>GPU</span><span>${res.gpu??0}</span></div>
                    <div class="progress-bar"><div class="progress-fill high" style="width:${Math.min(100, (res.gpu||0)*10)}%"></div></div>
                </div>
                <div class="muted" style="margin-top:0.5rem;">
                    <span>Start:</span> <span>${new Date(task.start_time).toLocaleString()}</span> 
                    <span style="margin-left:1rem;">Elapsed:</span> <span>${(task.elapsed??0)}s</span>
                </div>
            `;
            wrap.appendChild(card);
        });
    }

    // New function: update resource display
    function updateResourcesDisplay(data) {
        const container = document.getElementById('resource-status');
        if (!container) return;

        const availableResources = data.available_resources || {};
        const totalResources = data.total_resources || {};

        // Calculate resource usage percentage
        const cpuUsagePercent = totalResources.cpu ? Math.round(((totalResources.cpu - (availableResources.cpu || 0)) / totalResources.cpu) * 100) : 0;
        const memoryUsagePercent = totalResources.memory_gb ? Math.round(((totalResources.memory_gb - (availableResources.memory_gb || 0)) / totalResources.memory_gb) * 100) : 0;
        const gpuUsagePercent = totalResources.gpu ? Math.round(((totalResources.gpu - (availableResources.gpu || 0)) / totalResources.gpu) * 100) : 0;

        // Update progress bars
        updateProgressBar('#cpu-progress', cpuUsagePercent);
        updateProgressBar('#memory-progress', memoryUsagePercent);
        updateProgressBar('#gpu-progress', gpuUsagePercent);

        // Update value texts
        const cpuVal = document.getElementById('cpu-usage-value');
        const memVal = document.getElementById('memory-usage-value');
        const gpuVal = document.getElementById('gpu-usage-value');
        if (cpuVal) cpuVal.textContent = `${(totalResources.cpu - (availableResources.cpu || 0))}/${totalResources.cpu} (${cpuUsagePercent}%)`;
        if (memVal) {
            const usedMem = (totalResources.memory_gb - (availableResources.memory_gb || 0));
            const totalMem = totalResources.memory_gb || 0;
            memVal.textContent = `${usedMem.toFixed(1)}/${totalMem.toFixed(1)}GB (${memoryUsagePercent}%)`;
        }
        if (gpuVal) gpuVal.textContent = `${(totalResources.gpu - (availableResources.gpu || 0))}/${totalResources.gpu} (${gpuUsagePercent}%)`;
    }

    // Update progress bar
    function updateProgressBar(selector, percent) {
        const progressBar = document.querySelector(selector);
        if (!progressBar) return;
        progressBar.style.width = percent + '%';
        progressBar.classList.remove('bg-success','bg-warning','bg-danger');
        if (percent > 80) progressBar.classList.add('bg-danger');
        else if (percent > 60) progressBar.classList.add('bg-warning');
        else progressBar.classList.add('bg-success');
    }
});