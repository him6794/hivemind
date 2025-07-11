package com.example.hivemindworker

import android.content.Context
import io.grpc.ManagedChannelBuilder
import io.grpc.CallCredentials
import io.grpc.ManagedChannel
import io.grpc.Metadata
import io.grpc.stub.MetadataUtils
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import nodepool.LoginRequest
import nodepool.NodepoolGrpc
import nodepool.NodepoolOuterClass.*
import nodepool.RegisterWorkerNodeRequest
import nodepool.ReportStatusRequest
import java.io.ByteArrayOutputStream
import java.io.File
import java.io.FileOutputStream
import java.net.InetAddress
import java.net.NetworkInterface
import java.util.UUID
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.zip.ZipEntry
import java.util.zip.ZipFile
import java.util.zip.ZipOutputStream
import kotlin.math.max

class WorkerNodeViewModel : androidx.lifecycle.ViewModel() {

    // 使用 MutableStateFlow 管理內部可變狀態
    private val _status = MutableStateFlow("Initializing")
    val status: StateFlow<String> = _status.asStateFlow()

    private val _nodeId = MutableStateFlow("android-worker-${UUID.randomUUID()}")
    val nodeId: StateFlow<String> = _nodeId.asStateFlow()

    private val _currentTaskId = MutableStateFlow<String?>(null)
    val currentTaskId: StateFlow<String?> = _currentTaskId.asStateFlow()

    private val _username = MutableStateFlow<String?>(null)
    val username: StateFlow<String?> = _username.asStateFlow()

    private val _token = MutableStateFlow<String?>(null) // token 不需要對外暴露為 StateFlow

    private val _isRegistered = MutableStateFlow(false) // isRegistered 也不需要對外暴露為 StateFlow，其狀態可以通過 _status 反映

    private val _logs = MutableStateFlow(listOf<String>())
    val logs: StateFlow<List<String>> = _logs.asStateFlow()

    private val channel: ManagedChannel by lazy {
        ManagedChannelBuilder.forAddress("10.0.0.1", 50051)
            .usePlaintext()
            .keepAliveTime(30, TimeUnit.SECONDS)
            .build()
    }

    private val executor = Executors.newSingleThreadExecutor()
    private val scope = CoroutineScope(Dispatchers.IO + SupervisorJob())

    // 硬件信息 - 延迟计算
    private val _cpuCores by lazy { Runtime.getRuntime().availableProcessors() }
    val cpuCores: Int get() = _cpuCores

    private val _memoryGb by lazy { (Runtime.getRuntime().totalMemory() / 1e9).toInt() }
    val memoryGb: Int get() = _memoryGb

    private val _cpuScore by lazy { benchmarkCpu() }
    val cpuScore: Int get() = _cpuScore

    private val _localIp by lazy { getLocalIp() }
    val localIp: String get() = _localIp

    init {
        scope.launch {
            addLog("Worker initialized. ID: ${_nodeId.value}")
            // 只有註冊成功後才開始報告狀態
            _isRegistered.collect { registered ->
                if (registered) {
                    startStatusReporting()
                }
            }
        }
    }

    private fun benchmarkCpu(): Int = runCatching {
        val start = System.nanoTime()
        var result = 0L
        repeat(5_000_000) { i ->
            result = (result + i * i) % 987654321
        }
        val duration = System.nanoTime() - start
        max(1, (5_000_000_000.0 / duration).toInt())
    }.getOrElse {
        addLog("CPU benchmark failed: ${it.message}")
        5000
    }

    private fun getLocalIp(): String = runCatching {
        NetworkInterface.getNetworkInterfaces()?.toList()?.flatMap { iface ->
            iface.inetAddresses?.toList()?.mapNotNull { addr ->
                if (!addr.isLoopbackAddress && addr.hostAddress?.contains(".") == true) {
                    addr.hostAddress
                } else null
            } ?: emptyList()
        }?.firstOrNull() ?: "127.0.0.1"
    }.getOrElse {
        addLog("Failed to get local IP: ${it.message}")
        "127.0.0.1"
    }

    fun login(context: Context, username: String, password: String, onResult: (Boolean, String) -> Unit) {
        if (username.isBlank() || password.isBlank()) {
            onResult(false, "Username and password cannot be empty")
            return
        }

        scope.launch {
            try {
                val stub = NodepoolGrpc.newBlockingStub(channel)
                    .withDeadlineAfter(15, TimeUnit.SECONDS)

                val response = stub.login(
                    LoginRequest.newBuilder()
                        .setUsername(username)
                        .setPassword(password)
                        .build()
                )

                if (response.success && response.token.isNotBlank()) {
                    _username.value = username
                    _token.value = response.token
                    _status.value = "Logged In"
                    addLog("User $username logged in")
                    register(context)
                    onResult(true, "Login successful")
                } else {
                    _status.value = "Login Failed"
                    addLog("Login failed: ${response.message}")
                    onResult(false, response.message)
                }
            } catch (e: Exception) {
                _status.value = "Login Error"
                addLog("Login error: ${e.message}")
                onResult(false, "Network error: ${e.message}")
            }
        }
    }

    private fun register(context: Context) {
        val token = _token.value ?: run {
            addLog("Registration failed: No token")
            return
        }

        // 這裡可以考慮切換到 scope.launch 讓它也能在協程中執行，而不是單獨的 executor
        scope.launch {
            try {
                val stub = NodepoolGrpc.newBlockingStub(channel)
                    .withCallCredentials(BearerCredentials(token))
                    .withDeadlineAfter(15, TimeUnit.SECONDS)

                val request = RegisterWorkerNodeRequest.newBuilder()
                    .setNodeId(_username.value ?: _nodeId.value)
                    .setHostname(localIp)
                    .setCpuCores(cpuCores)
                    .setMemoryGb(memoryGb)
                    .setCpuScore(cpuScore)
                    .setGpuScore(0)
                    .setGpuName("Not Detected")
                    .setGpuMemoryGb(0)
                    .setLocation("Unknown")
                    .setPort(50053)
                    .build()

                val response = stub.registerWorkerNode(request)
                if (response.success) {
                    _nodeId.value = _username.value ?: _nodeId.value // 這裡更新 nodeId
                    _isRegistered.value = true
                    _status.value = "Idle"
                    addLog("Node registered successfully")
                } else {
                    _status.value = "Registration Failed"
                    addLog("Registration failed: ${response.message}")
                }
            } catch (e: Exception) {
                _status.value = "Registration Error"
                addLog("Registration error: ${e.message}")
            }
        }
    }

    private fun startStatusReporting() {
        scope.launch {
            // 使用 _isRegistered.value 而不是 collect，因為 collect 會持續監聽，這裡只需要在啟動時檢查一次
            // while (_isRegistered.value) { // 這樣寫會導致這個協程一直運行，即使 _isRegistered 變為 false
            // 更好的方式是當 _isRegistered 變為 false 時取消這個協程
            _isRegistered.collect { registered ->
                if (registered) {
                    while (true) { // 持續報告直到協程被取消或 _isRegistered 變為 false
                        try {
                            val token = _token.value ?: break // 如果token為空，則停止報告
                            val statusMsg = _currentTaskId.value?.let { "Executing: $it" } ?: _status.value

                            NodepoolGrpc.newBlockingStub(channel)
                                .withCallCredentials(BearerCredentials(token))
                                .withDeadlineAfter(10, TimeUnit.SECONDS)
                                .reportStatus(
                                    ReportStatusRequest.newBuilder()
                                        .setNodeId(_nodeId.value)
                                        .setStatusMessage(statusMsg)
                                        .build()
                                )
                            delay(5000) // 每5秒报告一次
                        } catch (e: Exception) {
                            addLog("Status report failed: ${e.message}")
                            delay(10000) // 出错时延长重试时间
                            // 如果是取消異常，則退出循環
                            if (e is CancellationException) break
                        }
                    }
                }
            }
        }
    }


    fun executeTask(context: Context, taskId: String, taskZip: ByteArray) {
        scope.launch { // 改為在協程中執行
            _currentTaskId.value = taskId
            _status.value = "Executing: $taskId"

            val workspace = File(context.filesDir, "task_$taskId").apply { mkdirs() }

            try {
                // 解压任务包
                File(workspace, "task.zip").apply {
                    FileOutputStream(this).use { it.write(taskZip) }
                    ZipFile(this).use { zip ->
                        zip.entries().toList().forEach { entry ->
                            val dest = File(workspace, entry.name)
                            if (entry.isDirectory) dest.mkdirs()
                            else zip.getInputStream(entry).use { input ->
                                FileOutputStream(dest).use { output ->
                                    input.copyTo(output)
                                }
                            }
                        }
                    }
                }

                // 执行任务脚本
                val script = File(workspace, "run_task.sh")
                if (script.exists()) {
                    script.setExecutable(true)
                    val process = ProcessBuilder("bash", script.absolutePath)
                        .directory(workspace)
                        .redirectErrorStream(true)
                        .start()

                    val output = process.inputStream.bufferedReader().use { it.readText() }
                    addLog("Task $taskId output:\n$output")

                    val success = process.waitFor() == 0

                    // 打包结果
                    val resultZip = createResultZip(taskId, workspace, success)

                    // 发送结果
                    _token.value?.let { token ->
                        // ReturnTaskResultRequest
                        NodepoolGrpc.newBlockingStub(channel)
                            .withCallCredentials(BearerCredentials(token))
                            .returnTaskResult(
                                ReturnTaskResultRequest.newBuilder()
                                    .setTaskId(taskId)
                                    .setResultZip(com.google.protobuf.ByteString.copyFrom(resultZip))
                                    .build()
                            )

                        // TaskCompletedRequest
                        NodepoolGrpc.newBlockingStub(channel)
                            .withCallCredentials(BearerCredentials(token))
                            .taskCompleted(
                                TaskCompletedRequest.newBuilder()
                                    .setTaskId(taskId)
                                    .setNodeId(_nodeId.value)
                                    .setSuccess(success)
                                    .build()
                            )
                    }

                    addLog("Task $taskId ${if (success) "completed" else "failed"}")
                } else {
                    addLog("Error: run_task.sh not found")
                }
            } catch (e: Exception) {
                addLog("Task execution failed: ${e.message}")
            } finally {
                workspace.deleteRecursively()
                _currentTaskId.value = null
                _status.value = "Idle"
            }
        }
    }

    private fun createResultZip(taskId: String, workspace: File, success: Boolean): ByteArray {
        return ByteArrayOutputStream().use { byteStream ->
            ZipOutputStream(byteStream).use { zip ->
                // 添加日志文件
                zip.putNextEntry(ZipEntry("execution_log.txt"))
                zip.write("Task: $taskId\nStatus: ${if (success) "SUCCESS" else "FAILED"}\n".toByteArray())
                zip.closeEntry()

                // 添加所有结果文件
                workspace.walk()
                    .filter { it.isFile }
                    .forEach { file ->
                        val relativePath = file.relativeTo(workspace).path
                        zip.putNextEntry(ZipEntry(relativePath))
                        file.inputStream().use { it.copyTo(zip) }
                        zip.closeEntry()
                    }
            }
            byteStream.toByteArray()
        }
    }

    private fun addLog(message: String) {
        // 更新 _logs 的值，並限制日誌數量
        _logs.value = (_logs.value + "${java.time.LocalTime.now()} - $message").takeLast(200)
    }

    override fun onCleared() {
        super.onCleared()
        scope.cancel("ViewModel cleared")
        executor.shutdownNow()
        channel.shutdownNow()
    }
}