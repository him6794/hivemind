$(document).ready(function() {
    // Dynamically detect API address
    const API_BASE_URL = window.location.origin;

    // Initialize Chart.js charts
    const cpuChartCtx = document.getElementById('cpuChart').getContext('2d');
    const memoryChartCtx = document.getElementById('memoryChart').getContext('2d');

    let cpuChart = null;
    let memoryChart = null;

    try {
        // Chart.js initialize CPU chart
        cpuChart = new Chart(cpuChartCtx, {
            type: 'line',
            data: { 
                labels: [], 
                datasets: [{ 
                    label: 'CPU Usage (%)', 
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
                            text: 'Time',
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
                            text: 'CPU Usage (%)',
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

        // Chart.js initialize memory chart
        memoryChart = new Chart(memoryChartCtx, {
            type: 'line',
            data: { 
                labels: [], 
                datasets: [{ 
                    label: 'Memory Usage (%)', 
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
                            text: 'Time',
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
                            text: 'Memory Usage (%)',
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
        
        console.log("Charts initialized successfully");
    } catch (error) {
        console.error("Chart initialization failed:", error);
        $('.chart-container').html('<div class="text-center text-error">Chart initialization failed, please check the console logs.</div>');
    }

    function updateStatus() {
        $.get(`${API_BASE_URL}/api/status`, function(data) {
            if (data.error) {
                console.error("Error from /api/status:", data.error);
                $('#task-status').text('Error loading status').removeClass().addClass('status error');
                return;
            }

            // Update status display
            $('#node-id').text(data.node_id || 'N/A');
            
            // Support multi-task mode
            if (data.tasks && data.tasks.length > 0) {
                // Display task count
                $('#task-id').text(`${data.task_count} tasks running`);
                
                // Display the first task ID (for backward compatibility)
                if (data.current_task_id && data.current_task_id !== "None") {
                    $('#task-id').append(` (Main: ${data.current_task_id})`);
                }
                
                // If the task list container exists, update the task list
                if ($('#tasks-list').length) {
                    updateTasksList(data.tasks);
                } else {
                    // Otherwise, only display the ID of the first task
                    $('#task-id').text(data.current_task_id !== "None" ? data.current_task_id : "no tasks");
                }
            } else {
                $('#task-id').text("no tasks");
            }
            
            // Update status label style, support new load states
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
            
            // Display Docker status
            if ($('#docker-status').length) {
                const dockerStatus = data.docker_status || (data.docker_available ? 'available' : 'unavailable');
                $('#docker-status').text(dockerStatus);
                $('#docker-status').removeClass().addClass('status ' + 
                    (dockerStatus === 'available' ? 'idle' : 'error'));
            }
            
            // Update resource usage
            updateResourcesDisplay(data);
            
            $('#ip-address').text(data.ip || 'N/A');
            $('#cpt-balance').text(data.cpt_balance || 0);
            
            const cpuPercent = data.cpu_percent || 0;
            const memoryPercent = data.memory_percent || 0;
            
            $('#cpu-usage').text(cpuPercent + '%');
            $('#memory-usage').text(memoryPercent + '%');
            
            // Update resource cards with load status colors
            const cpuElement = $('#cpu-metric');
            const memoryElement = $('#memory-metric');
            
            cpuElement.text(cpuPercent + '%');
            memoryElement.text(memoryPercent + '%');
            
            // Adjust color based on load
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

        }).fail(function(jqXHR, textStatus, errorThrown) {
            console.error("Failed to fetch /api/status:", textStatus, errorThrown);
            $('#task-status').text('Connection Error').removeClass().addClass('status error');

            if (jqXHR.status === 401) {
                console.warn("Session expired, redirecting to login page in 3 seconds");
                setTimeout(function() {
                    window.location.href = '/login';
                }, 3000);
            }
        });
    }

    function updateLogs() {
        $.get(`${API_BASE_URL}/api/logs`, function(data) {
            console.log("Log data:", data);
            
            if (data.error) {
                console.error("Error from /api/logs:", data.error);
                $('#logs').html(`<div class="text-error">Error loading logs: ${data.error}</div>`);
                return;
            }
            
            const logsDiv = $('#logs');
            logsDiv.empty();
            
            if (data.logs && Array.isArray(data.logs)) {
                if (data.logs.length === 0) {
                    logsDiv.html('<div class="text-center" style="opacity: 0.7;">Currently no log records</div>');
                } else {
                    data.logs.forEach(log => {
                        const logEntry = $('<div>').text(log).addClass('log-entry');
                        logsDiv.append(logEntry);
                    });
                    // Auto-scroll to the bottom
                    logsDiv.scrollTop(logsDiv[0].scrollHeight);
                }
            } else {
                console.warn("Invalid log data format:", data);
                logsDiv.html('<div class="text-warning">Did not receive valid log data</div>');
            }
        }).fail(function(jqXHR, textStatus, errorThrown) {
            console.error("Failed to fetch /api/logs:", textStatus, errorThrown);
            $('#logs').html(`<div class="text-error">Error loading logs: ${textStatus} (${jqXHR.status})</div>`);
            
            if (jqXHR.status === 401) {
                console.warn("Session expired, redirecting to login page in 3 seconds");
                setTimeout(function() {
                    window.location.href = '/login';
                }, 3000);
            }
        });
    }

    // Initial load
    updateStatus();
    updateLogs();
    
    // Regular updates
    setInterval(updateStatus, 3000);  // Update status every 3 seconds
    setInterval(updateLogs, 5000);    // Update logs every 5 seconds

    // Global functions
    window.refreshStatus = function() {
        console.log("Manually refreshing status");
        updateStatus();
    }

    window.refreshLogs = function() {
        console.log("Manually refreshing logs");
        updateLogs();
    }
    
    // New function: update task list
    function updateTasksList(tasks) {
        const tasksListEl = $('#tasks-list');
        tasksListEl.empty();

        if (!tasks || tasks.length === 0) {
            tasksListEl.html('<div class="text-center p-3">Currently no tasks are running</div>');
            return;
        }

        tasks.forEach(task => {
            const taskEl = $('<div>').addClass('task-item p-2 my-1 border rounded');

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

            taskEl.html(`
                <div><strong>ID:</strong> ${task.id}</div>
                <div><strong>Status:</strong> <span class="status ${task.status === 'Executing' ? 'running' : 'pending'}">${task.status}</span></div>
                <div><strong>Start Time:</strong> ${new Date(task.start_time).toLocaleString()}</div>
                <div><strong>Execution Time:</strong> ${durationStr}</div>
                <div><strong>Resources:</strong> ${resourcesStr}</div>
            `);

            tasksListEl.append(taskEl);
        });
    }

    // New function: update resource display
    function updateResourcesDisplay(data) {
        // If resource block exists
        if ($('#resource-status').length) {
            const availableResources = data.available_resources || {};
            const totalResources = data.total_resources || {};

            // Calculate resource usage percentage
            const cpuUsagePercent = totalResources.cpu ? Math.round(((totalResources.cpu - availableResources.cpu) / totalResources.cpu) * 100) : 0;
            const memoryUsagePercent = totalResources.memory_gb ? Math.round(((totalResources.memory_gb - availableResources.memory_gb) / totalResources.memory_gb) * 100) : 0;
            const gpuUsagePercent = totalResources.gpu ? Math.round(((totalResources.gpu - availableResources.gpu) / totalResources.gpu) * 100) : 0;

            // Update progress bars
            updateProgressBar('#cpu-progress', cpuUsagePercent);
            updateProgressBar('#memory-progress', memoryUsagePercent);
            updateProgressBar('#gpu-progress', gpuUsagePercent);

            // Update values
            $('#cpu-usage-value').text(`${totalResources.cpu - availableResources.cpu}/${totalResources.cpu} (${cpuUsagePercent}%)`);
            $('#memory-usage-value').text(`${(totalResources.memory_gb - availableResources.memory_gb).toFixed(1)}/${totalResources.memory_gb.toFixed(1)}GB (${memoryUsagePercent}%)`);
            $('#gpu-usage-value').text(`${totalResources.gpu - availableResources.gpu}/${totalResources.gpu} (${gpuUsagePercent}%)`);
        }
    }

    // Update progress bar
    function updateProgressBar(selector, percent) {
        const progressBar = $(selector);
        if (progressBar.length) {
            progressBar.css('width', percent + '%');

            // Adjust color based on percentage
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