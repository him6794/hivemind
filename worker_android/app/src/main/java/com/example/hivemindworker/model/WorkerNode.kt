package com.example.hivemindworker.model

import android.content.Context
import android.os.Build
import android.util.Log
import com.example.hivemindworker.nodepool.NodepoolGrpcClient
import com.example.hivemindworker.proto.Nodepool
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.withContext
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicReference

class WorkerNode(private val context: Context) {
    enum class LogLevel { INFO, WARNING, ERROR }
    
    private val _logs = CopyOnWriteArrayList<String>()
    val logs: List<String> get() = _logs
    
    private val _status = MutableStateFlow("初始化中")
    val status: StateFlow<String> = _status.asStateFlow()
    
    private val _isRegistered = MutableStateFlow(false)
    val isRegistered: StateFlow<Boolean> = _isRegistered.asStateFlow()
    
    private val _isLoggedIn = MutableStateFlow(false)
    val isLoggedIn: StateFlow<Boolean> = _isLoggedIn.asStateFlow()
    
    private val _currentTaskId = MutableStateFlow<String?>(null)
    val currentTaskId: StateFlow<String?> = _currentTaskId.asStateFlow()
    
    private val _cptBalance = MutableStateFlow(0)
    val cptBalance: StateFlow<Int> = _cptBalance.asStateFlow()
    
    private val _location = MutableStateFlow("Unknown")
    val location: StateFlow<String> = _location.asStateFlow()
    
    private val username = AtomicReference<String>(null)
    private val token = AtomicReference<String>(null)
    private val nodeId = AtomicReference<String>(null)
    private val isRunning = AtomicBoolean(false)
    
    // 硬體資訊
    private val _cpuCores = MutableStateFlow(0)
    val cpuCores: StateFlow<Int> = _cpuCores.asStateFlow()
    
    private val _memoryGb = MutableStateFlow(0.0)
    val memoryGb: StateFlow<Double> = _memoryGb.asStateFlow()
    
    private val _totalMemoryGb = MutableStateFlow(0.0)
    val totalMemoryGb: StateFlow<Double> = _totalMemoryGb.asStateFlow()
    
    private val _cpuScore = MutableStateFlow(0)
    val cpuScore: StateFlow<Int> = _cpuScore.asStateFlow()
    
    private val _gpuScore = MutableStateFlow(0)
    val gpuScore: StateFlow<Int> = _gpuScore.asStateFlow()
    
    private val _gpuName = MutableStateFlow("未檢測到GPU")
    val gpuName: StateFlow<String> = _gpuName.asStateFlow()
    
    private val _gpuMemoryGb = MutableStateFlow(0.0)
    val gpuMemoryGb: StateFlow<Double> = _gpuMemoryGb.asStateFlow()
    
    // gRPC客戶端
    private var grpcClient: NodepoolGrpcClient? = null
    
    // 初始化硬體資訊
    suspend fun initHardwareInfo() {
        withContext(Dispatchers.IO) {
            try {
                // 在Android上獲取CPU核心數
                _cpuCores.value = Runtime.getRuntime().availableProcessors()
                
                // 獲取記憶體資訊
                val memInfo = context.getSystemService(Context.ACTIVITY_SERVICE) as android.app.ActivityManager
                val memoryInfo = android.app.ActivityManager.MemoryInfo()
                memInfo.getMemoryInfo(memoryInfo)
                
                val availableMemoryMb = memoryInfo.availMem / (1024 * 1024)
                val totalMemoryMb = memoryInfo.totalMem / (1024 * 1024)
                
                _memoryGb.value = availableMemoryMb / 1024.0
                _totalMemoryGb.value = totalMemoryMb / 1024.0
                
                // 簡單的CPU基準測試
                _cpuScore.value = benchmarkCpu()
                
                // 檢測GPU (這在Android上較難，這裡只是簡化模擬)
                val deviceModel = Build.MODEL
                if (deviceModel.contains("Galaxy S") || deviceModel.contains("Pixel")) {
                    _gpuName.value = "Adreno/Mali GPU"
                    _gpuScore.value = 300
                    _gpuMemoryGb.value = 2.0
                }
                
                log("硬體資訊: CPU=${_cpuCores.value}核, RAM=${_memoryGb.value.format(1)}GB可用 (總共: ${_totalMemoryGb.value.format(1)}GB)")
                log("效能評分: CPU=${_cpuScore.value}, GPU=${_gpuScore.value}")
            } catch (e: Exception) {
                log("硬體檢測失敗: ${e.message}", LogLevel.ERROR)
            }
        }
    }
    
    private fun benchmarkCpu(): Int {
        return try {
            val startTime = System.currentTimeMillis()
            var result = 0L
            for (i in 0 until 1_000_000) {
                result = (result + i * i) % 987654321
            }
            val duration = System.currentTimeMillis() - startTime
            ((1_000_000 / duration.toDouble()) / 10).toInt()
        } catch (e: Exception) {
            100 // 預設值
        }
    }
    
    fun setNodeId(id: String) {
        nodeId.set(id)
        log("節點ID設定為: $id")
    }
    
    fun setLocation(newLocation: String) {
        _location.value = newLocation
        log("地區已更新為: $newLocation")
    }
    
    suspend fun initGrpc(masterAddress: String) {
        withContext(Dispatchers.IO) {
            try {
                grpcClient = NodepoolGrpcClient(masterAddress)
                log("已連接到主節點: $masterAddress")
                _status.value = "等待登入"
            } catch (e: Exception) {
                log("gRPC連接失敗: ${e.message}", LogLevel.ERROR)
                _status.value = "連接失敗"
            }
        }
    }
    
    suspend fun login(username: String, password: String): Boolean {
        return withContext(Dispatchers.IO) {
            try {
                val client = grpcClient ?: throw IllegalStateException("gRPC客戶端未初始化")
                
                val response = client.login(username, password)
                if (response.success && response.token.isNotEmpty()) {
                    this@WorkerNode.username.set(username)
                    token.set(response.token)
                    _isLoggedIn.value = true
                    _status.value = "已登入"
                    log("用戶 $username 登入成功")
                    true
                } else {
                    _status.value = "登入失敗"
                    log("用戶 $username 登入失敗: ${response.message}", LogLevel.ERROR)
                    false
                }
            } catch (e: Exception) {
                log("登入錯誤: ${e.message}", LogLevel.ERROR)
                _status.value = "登入錯誤"
                false
            }
        }
    }
    
    suspend fun register(): Boolean {
        return withContext(Dispatchers.IO) {
            try {
                val currentToken = token.get() ?: return@withContext false
                val currentUsername = username.get() ?: return@withContext false
                val currentNodeId = nodeId.get() ?: return@withContext false
                
                val client = grpcClient ?: throw IllegalStateException("gRPC客戶端未初始化")
                
                val request = Nodepool.RegisterWorkerNodeRequest.newBuilder().apply {
                    nodeId = currentNodeId
                    hostname = currentUsername
                    cpuCores = _cpuCores.value
                    memoryGb = _memoryGb.value.toFloat()
                    cpuScore = _cpuScore.value
                    gpuScore = _gpuScore.value
                    gpuName = _gpuName.value
                    gpuMemoryGb = _gpuMemoryGb.value.toFloat()
                    location = _location.value
                    port = 50053 // 默認端口
                }.build()
                
                val response = client.registerWorker(request, currentToken)
                
                if (response.success) {
                    _isRegistered.value = true
                    _status.value = "閒置"
                    log("節點註冊成功")
                    true
                } else {
                    _status.value = "註冊失敗: ${response.message}"
                    log("節點註冊失敗: ${response.message}", LogLevel.ERROR)
                    false
                }
            } catch (e: Exception) {
                log("註冊錯誤: ${e.message}", LogLevel.ERROR)
                _status.value = "註冊錯誤"
                false
            }
        }
    }
    
    suspend fun sendStatusUpdate() {
        withContext(Dispatchers.IO) {
            try {
                val currentToken = token.get() ?: return@withContext
                val currentNodeId = nodeId.get() ?: return@withContext
                val client = grpcClient ?: return@withContext
                
                val statusMsg = if (_currentTaskId.value != null) {
                    "執行任務: ${_currentTaskId.value}"
                } else {
                    _status.value
                }
                
                val request = Nodepool.ReportStatusRequest.newBuilder().apply {
                    nodeId = currentNodeId
                    statusMessage = statusMsg
                }.build()
                
                client.reportStatus(request, currentToken)
                // 不需要記錄每次狀態更新，以避免日誌過多
            } catch (e: Exception) {
                // 狀態更新失敗不需要顯示錯誤，僅在偵錯模式下記錄
                Log.d("WorkerNode", "狀態更新失敗: ${e.message}")
            }
        }
    }
    
    suspend fun updateBalance() {
        withContext(Dispatchers.IO) {
            try {
                val currentToken = token.get() ?: return@withContext
                val currentUsername = username.get() ?: return@withContext
                val client = grpcClient ?: return@withContext
                
                val response = client.getBalance(currentUsername, currentToken)
                if (response.success) {
                    _cptBalance.value = response.balance
                }
            } catch (e: Exception) {
                Log.d("WorkerNode", "餘額更新失敗: ${e.message}")
            }
        }
    }
    
    suspend fun fetchVpnConfig(): String {
        return withContext(Dispatchers.IO) {
            try {
                log("正在從主節點請求VPN配置...")
                // 這裡應該實現向主伺服器請求VPN配置的邏輯
                // 由於API可能不同，這裡僅模擬一個空實現
                ""
            } catch (e: Exception) {
                log("獲取VPN配置失敗: ${e.message}", LogLevel.ERROR)
                ""
            }
        }
    }
    
    fun logout() {
        username.set(null)
        token.set(null)
        _isLoggedIn.value = false
        _isRegistered.value = false
        _status.value = "等待登入"
        _currentTaskId.value = null
        _cptBalance.value = 0
        log("已登出")
    }
    
    fun log(message: String, level: LogLevel = LogLevel.INFO) {
        val timestamp = SimpleDateFormat("yyyy-MM-dd HH:mm:ss", Locale.getDefault()).format(Date())
        val logEntry = "[$timestamp] ${level.name} - $message"
        _logs.add(0, logEntry) // 添加到列表開頭
        
        // 保持日誌不超過100條
        while (_logs.size > 100) {
            _logs.removeAt(_logs.size - 1)
        }
        
        // 同時輸出到logcat
        when (level) {
            LogLevel.INFO -> Log.i("WorkerNode", message)
            LogLevel.WARNING -> Log.w("WorkerNode", message)
            LogLevel.ERROR -> Log.e("WorkerNode", message)
        }
    }
    
    fun cleanup() {
        isRunning.set(false)
        grpcClient?.shutdown()
        grpcClient = null
    }
    
    private fun Double.format(digits: Int) = String.format("%.${digits}f", this)
}
