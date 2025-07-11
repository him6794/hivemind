package com.example.hivemindworker.viewmodel

import android.content.Context
import androidx.lifecycle.LiveData
import androidx.lifecycle.MutableLiveData
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.example.hivemindworker.model.WorkerNode
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch

class WorkerViewModel : ViewModel() {
    private lateinit var workerNode: WorkerNode
    
    // LiveData for UI interactions
    private val _errorMessage = MutableLiveData<String>()
    val errorMessage: LiveData<String> = _errorMessage
    
    // StateFlows from WorkerNode
    lateinit var status: StateFlow<String>
    lateinit var isRegistered: StateFlow<Boolean>
    lateinit var isLoggedIn: StateFlow<Boolean>
    lateinit var currentTaskId: StateFlow<String?>
    lateinit var cptBalance: StateFlow<Int>
    lateinit var location: StateFlow<String>
    lateinit var cpuCores: StateFlow<Int>
    lateinit var memoryGb: StateFlow<Double>
    lateinit var totalMemoryGb: StateFlow<Double>
    lateinit var cpuScore: StateFlow<Int>
    lateinit var gpuScore: StateFlow<Int>
    lateinit var gpuName: StateFlow<String>
    lateinit var gpuMemoryGb: StateFlow<Double>
    
    // Logs LiveData
    private val _logs = MutableLiveData<List<String>>()
    val logs: LiveData<List<String>> = _logs
    
    fun initHardwareInfo(context: Context) {
        if (!::workerNode.isInitialized) {
            workerNode = WorkerNode(context)
            
            // 將所有StateFlows連接到ViewModel
            status = workerNode.status.stateIn(
                viewModelScope, SharingStarted.Eagerly, "初始化中"
            )
            isRegistered = workerNode.isRegistered.stateIn(
                viewModelScope, SharingStarted.Eagerly, false
            )
            isLoggedIn = workerNode.isLoggedIn.stateIn(
                viewModelScope, SharingStarted.Eagerly, false
            )
            currentTaskId = workerNode.currentTaskId.stateIn(
                viewModelScope, SharingStarted.Eagerly, null
            )
            cptBalance = workerNode.cptBalance.stateIn(
                viewModelScope, SharingStarted.Eagerly, 0
            )
            location = workerNode.location.stateIn(
                viewModelScope, SharingStarted.Eagerly, "Unknown"
            )
            cpuCores = workerNode.cpuCores.stateIn(
                viewModelScope, SharingStarted.Eagerly, 0
            )
            memoryGb = workerNode.memoryGb.stateIn(
                viewModelScope, SharingStarted.Eagerly, 0.0
            )
            totalMemoryGb = workerNode.totalMemoryGb.stateIn(
                viewModelScope, SharingStarted.Eagerly, 0.0
            )
            cpuScore = workerNode.cpuScore.stateIn(
                viewModelScope, SharingStarted.Eagerly, 0
            )
            gpuScore = workerNode.gpuScore.stateIn(
                viewModelScope, SharingStarted.Eagerly, 0
            )
            gpuName = workerNode.gpuName.stateIn(
                viewModelScope, SharingStarted.Eagerly, "未檢測到GPU"
            )
            gpuMemoryGb = workerNode.gpuMemoryGb.stateIn(
                viewModelScope, SharingStarted.Eagerly, 0.0
            )
        }
        
        viewModelScope.launch {
            workerNode.initHardwareInfo()
            refreshLogs()
        }
    }
    
    fun setNodeId(nodeId: String) {
        if (::workerNode.isInitialized) {
            workerNode.setNodeId(nodeId)
        }
    }
    
    fun initGrpc(masterAddress: String) {
        viewModelScope.launch {
            workerNode.initGrpc(masterAddress)
            refreshLogs()
        }
    }
    
    fun login(username: String, password: String) {
        viewModelScope.launch {
            try {
                val success = workerNode.login(username, password)
                if (success) {
                    if (workerNode.register()) {
                        refreshLogs()
                    } else {
                        _errorMessage.value = "登入成功但節點註冊失敗"
                    }
                } else {
                    _errorMessage.value = "登入失敗，請檢查用戶名和密碼"
                }
            } catch (e: Exception) {
                _errorMessage.value = "登入錯誤: ${e.message}"
            }
        }
    }
    
    fun logout() {
        viewModelScope.launch {
            workerNode.logout()
            refreshLogs()
        }
    }
    
    fun updateLocation(newLocation: String) {
        viewModelScope.launch {
            workerNode.setLocation(newLocation)
            
            // 如果已註冊，需要重新註冊以更新地區資訊
            if (isRegistered.value && isLoggedIn.value) {
                workerNode.register()
            }
            
            refreshLogs()
        }
    }
    
    fun sendStatusUpdate() {
        viewModelScope.launch {
            workerNode.sendStatusUpdate()
        }
    }
    
    fun updateBalance() {
        viewModelScope.launch {
            workerNode.updateBalance()
        }
    }
    
    fun log(message: String, level: WorkerNode.LogLevel = WorkerNode.LogLevel.INFO) {
        workerNode.log(message, level)
        refreshLogs()
    }
    
    fun clearErrorMessage() {
        _errorMessage.value = ""
    }
    
    fun fetchVpnConfig(): String {
        var config = ""
        viewModelScope.launch {
            config = workerNode.fetchVpnConfig()
        }
        return config
    }
    
    fun cleanup() {
        if (::workerNode.isInitialized) {
            workerNode.cleanup()
        }
    }
    
    private fun refreshLogs() {
        if (::workerNode.isInitialized) {
            _logs.value = workerNode.logs
        }
    }
}
