package com.example.hivemindworker

import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.util.Log
import android.view.Menu
import android.view.MenuItem
import android.widget.Toast
import androidx.appcompat.app.AlertDialog
import androidx.appcompat.app.AppCompatActivity
import androidx.lifecycle.ViewModelProvider
import com.example.hivemindworker.databinding.ActivityMainBinding
import com.example.hivemindworker.model.WorkerNode
import com.example.hivemindworker.ui.LoginFragment
import com.example.hivemindworker.ui.MonitorFragment
import com.example.hivemindworker.viewmodel.WorkerViewModel
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import java.io.File
import java.util.UUID

private const val TAG = "MainActivity"
private const val NODE_PORT = 50053
private const val MASTER_ADDRESS = "10.0.0.1:50051"

class MainActivity : AppCompatActivity() {
    private lateinit var binding: ActivityMainBinding
    private lateinit var viewModel: WorkerViewModel
    private val mainScope = CoroutineScope(Dispatchers.Main)
    private val handler = Handler(Looper.getMainLooper())
    
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        binding = ActivityMainBinding.inflate(layoutInflater)
        setContentView(binding.root)
        setSupportActionBar(binding.toolbar)
        
        // 初始化ViewModel
        viewModel = ViewModelProvider(this)[WorkerViewModel::class.java]
        
        // 設置節點ID
        val nodeId = generateNodeId()
        viewModel.setNodeId(nodeId)
        
        // 初始化Worker Node
        initWorkerNode()
        
        // 顯示登入畫面
        if (savedInstanceState == null) {
            supportFragmentManager.beginTransaction()
                .replace(R.id.fragment_container, LoginFragment())
                .commit()
        }
        
        // 觀察登入狀態
        viewModel.isLoggedIn.observe(this) { isLoggedIn ->
            if (isLoggedIn) {
                // 切換到監控頁面
                supportFragmentManager.beginTransaction()
                    .replace(R.id.fragment_container, MonitorFragment())
                    .commit()
            } else {
                // 切換到登入頁面
                supportFragmentManager.beginTransaction()
                    .replace(R.id.fragment_container, LoginFragment())
                    .commit()
            }
        }
        
        // 觀察錯誤訊息
        viewModel.errorMessage.observe(this) { errorMsg ->
            if (errorMsg.isNotEmpty()) {
                Toast.makeText(this, errorMsg, Toast.LENGTH_LONG).show()
                viewModel.clearErrorMessage()
            }
        }
    }
    
    override fun onCreateOptionsMenu(menu: Menu): Boolean {
        menuInflater.inflate(R.menu.main_menu, menu)
        return true
    }
    
    override fun onOptionsItemSelected(item: MenuItem): Boolean {
        return when (item.itemId) {
            R.id.action_logout -> {
                viewModel.logout()
                true
            }
            R.id.action_settings -> {
                showSettingsDialog()
                true
            }
            R.id.action_vpn -> {
                setupVpn()
                true
            }
            else -> super.onOptionsItemSelected(item)
        }
    }
    
    private fun initWorkerNode() {
        mainScope.launch {
            try {
                // 檢測硬體資訊
                viewModel.initHardwareInfo(applicationContext)
                
                // 嘗試連線VPN
                checkVpnConnection()
                
                // 初始化gRPC
                viewModel.initGrpc(MASTER_ADDRESS)
                
                // 開始定期更新狀態
                startStatusUpdates()
                
                // 記錄初始化成功
                viewModel.log("Worker Node 初始化完成")
            } catch (e: Exception) {
                Log.e(TAG, "初始化Worker Node失敗", e)
                viewModel.log("初始化失敗: ${e.message}", WorkerNode.LogLevel.ERROR)
            }
        }
    }
    
    private fun generateNodeId(): String {
        val deviceName = android.os.Build.MODEL.replace("\\s+".toRegex(), "-")
        return "android-worker-$deviceName-${UUID.randomUUID().toString().substring(0, 8)}"
    }
    
    private fun checkVpnConnection() {
        mainScope.launch {
            try {
                viewModel.log("檢查VPN連線狀態...")
                // 在實際應用中，這裡應該檢查VPN狀態
                val isVpnConnected = false
                
                if (!isVpnConnected) {
                    showVpnPrompt()
                }
            } catch (e: Exception) {
                viewModel.log("VPN檢查失敗: ${e.message}", WorkerNode.LogLevel.WARNING)
            }
        }
    }
    
    private fun showVpnPrompt() {
        AlertDialog.Builder(this)
            .setTitle("需要VPN連線")
            .setMessage("Hivemind Worker需要連接到VPN才能正常運作。要嘗試自動連接嗎？")
            .setPositiveButton("是") { _, _ -> setupVpn() }
            .setNegativeButton("否") { _, _ -> 
                viewModel.log("用戶跳過VPN設定，部分功能可能無法使用", WorkerNode.LogLevel.WARNING)
            }
            .show()
    }
    
    private fun setupVpn() {
        mainScope.launch {
            try {
                viewModel.log("正在嘗試設定VPN...")
                
                // 這裡應該向主伺服器請求VPN配置
                val vpnConfigFile = downloadVpnConfig()
                
                if (vpnConfigFile != null) {
                    // 開啟VPN應用
                    openVpnApp(vpnConfigFile)
                } else {
                    viewModel.log("無法取得VPN配置", WorkerNode.LogLevel.ERROR)
                }
            } catch (e: Exception) {
                viewModel.log("VPN設定失敗: ${e.message}", WorkerNode.LogLevel.ERROR)
            }
        }
    }
    
    private suspend fun downloadVpnConfig(): File? {
        // 這裡應該實現向主伺服器請求VPN配置並儲存到檔案
        // 簡化示例:
        return try {
            val configContent = viewModel.fetchVpnConfig()
            if (configContent.isNotEmpty()) {
                val file = File(getExternalFilesDir(null), "hivemind_vpn.conf")
                file.writeText(configContent)
                file
            } else {
                null
            }
        } catch (e: Exception) {
            Log.e(TAG, "下載VPN配置失敗", e)
            null
        }
    }
    
    private fun openVpnApp(configFile: File) {
        try {
            // 開啟VPN應用的Intent
            // 這裡假設使用OpenVPN，實際應用可能需要調整
            val intent = Intent(Intent.ACTION_VIEW)
            intent.setDataAndType(Uri.fromFile(configFile), "application/x-openvpn-profile")
            startActivity(intent)
        } catch (e: Exception) {
            viewModel.log("無法開啟VPN應用: ${e.message}", WorkerNode.LogLevel.ERROR)
            Toast.makeText(this, "請安裝OpenVPN應用以連接到Hivemind網絡", Toast.LENGTH_LONG).show()
        }
    }
    
    private fun startStatusUpdates() {
        // 每5秒更新一次狀態
        handler.postDelayed(object : Runnable {
            override fun run() {
                if (viewModel.isRegistered.value == true) {
                    mainScope.launch {
                        viewModel.sendStatusUpdate()
                        viewModel.updateBalance()
                    }
                }
                handler.postDelayed(this, 5000)
            }
        }, 5000)
    }
    
    private fun showSettingsDialog() {
        val locations = arrayOf("亞洲", "非洲", "北美", "南美", "歐洲", "大洋洲", "Unknown")
        val currentLocation = viewModel.location.value ?: "Unknown"
        val currentIndex = locations.indexOf(currentLocation)
        
        AlertDialog.Builder(this)
            .setTitle("設定地區")
            .setSingleChoiceItems(locations, currentIndex) { dialog, which ->
                val newLocation = locations[which]
                viewModel.updateLocation(newLocation)
                dialog.dismiss()
                Toast.makeText(this, "地區已更新為: $newLocation", Toast.LENGTH_SHORT).show()
            }
            .setNegativeButton("取消", null)
            .show()
    }
    
    override fun onDestroy() {
        // 清理資源
        handler.removeCallbacksAndMessages(null)
        viewModel.cleanup()
        super.onDestroy()
    }
}
