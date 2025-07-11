package com.example.hivemindworker.nodepool

import android.util.Log
import com.example.hivemindworker.proto.Nodepool
import java.util.UUID
import kotlin.random.Random

/**
 * 模擬服務器響應，用於Demo測試
 */
class MockNodepoolService {
    private val TAG = "MockNodepoolService"
    
    // 模擬用戶數據
    private val validUsers = mapOf(
        "demo" to "password",
        "user" to "123456",
        "test" to "test"
    )
    
    // 已註冊的令牌
    private val tokens = mutableMapOf<String, String>()
    
    // 模擬用戶餘額
    private val userBalances = mutableMapOf<String, Int>()
    
    fun login(username: String, password: String): Nodepool.LoginResponse {
        Log.d(TAG, "模擬登入: $username")
        
        val expectedPassword = validUsers[username]
        
        return if (expectedPassword != null && expectedPassword == password) {
            val token = UUID.randomUUID().toString()
            tokens[username] = token
            
            // 初始化餘額
            if (!userBalances.containsKey(username)) {
                userBalances[username] = Random.nextInt(1000, 5000)
            }
            
            Nodepool.LoginResponse.newBuilder()
                .setSuccess(true)
                .setToken(token)
                .setMessage("登入成功")
                .build()
        } else {
            Nodepool.LoginResponse.newBuilder()
                .setSuccess(false)
                .setMessage("用戶名或密碼錯誤")
                .build()
        }
    }
    
    fun getBalance(username: String, token: String): Nodepool.GetBalanceResponse {
        Log.d(TAG, "模擬獲取餘額: $username")
        
        val storedToken = tokens[username]
        
        return if (storedToken != null && storedToken == token) {
            val balance = userBalances[username] ?: 0
            
            // 每次查詢餘額時隨機增加一些，模擬獲得收益
            userBalances[username] = balance + Random.nextInt(0, 10)
            
            Nodepool.GetBalanceResponse.newBuilder()
                .setSuccess(true)
                .setBalance(userBalances[username] ?: 0)
                .build()
        } else {
            Nodepool.GetBalanceResponse.newBuilder()
                .setSuccess(false)
                .setMessage("身份驗證失敗")
                .build()
        }
    }
    
    fun registerWorkerNode(request: Nodepool.RegisterWorkerNodeRequest, token: String): Nodepool.RegisterWorkerNodeResponse {
        Log.d(TAG, "模擬註冊節點: ${request.nodeId}")
        
        // 檢查令牌是否有效
        val isValidToken = tokens.values.contains(token)
        
        return if (isValidToken) {
            Nodepool.RegisterWorkerNodeResponse.newBuilder()
                .setSuccess(true)
                .setMessage("節點註冊成功")
                .build()
        } else {
            Nodepool.RegisterWorkerNodeResponse.newBuilder()
                .setSuccess(false)
                .setMessage("身份驗證失敗")
                .build()
        }
    }
    
    fun reportStatus(request: Nodepool.ReportStatusRequest, token: String): Nodepool.ReportStatusResponse {
        // 實際項目中會記錄狀態，這裡只模擬響應
        return Nodepool.ReportStatusResponse.newBuilder()
            .setSuccess(true)
            .build()
    }
}
