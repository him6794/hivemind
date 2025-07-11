package com.example.hivemindworker.nodepool

import android.util.Log
import com.example.hivemindworker.proto.Nodepool
import io.grpc.ManagedChannel
import io.grpc.ManagedChannelBuilder
import io.grpc.Metadata
import io.grpc.stub.MetadataUtils
import java.io.Closeable
import java.util.concurrent.TimeUnit

class NodepoolGrpcClient(private val serverAddress: String) : Closeable {
    private val TAG = "NodepoolGrpcClient"
    
    // 使用模擬服務以便在Demo中測試
    private val mockService = MockNodepoolService()
    private val isDemo = true // 設置為true以使用模擬服務
    
    private val channel: ManagedChannel = if (!isDemo) {
        ManagedChannelBuilder
            .forTarget(serverAddress)
            .usePlaintext()
            .build()
    } else {
        // Demo模式下不實際建立連接
        null
    } as ManagedChannel
    
    // 這些存根在Demo模式下不會使用
    private val userStub = if (!isDemo) UserServiceGrpcKt.UserServiceCoroutineStub(channel) else null
    private val nodeStub = if (!isDemo) NodeManagerServiceGrpcKt.NodeManagerServiceCoroutineStub(channel) else null
    private val masterStub = if (!isDemo) MasterNodeServiceGrpcKt.MasterNodeServiceCoroutineStub(channel) else null
    
    suspend fun login(username: String, password: String): Nodepool.LoginResponse {
        return if (isDemo) {
            // 使用模擬服務
            Log.d(TAG, "使用模擬服務登入")
            mockService.login(username, password)
        } else {
            val request = Nodepool.LoginRequest.newBuilder()
                .setUsername(username)
                .setPassword(password)
                .build()
            
            userStub?.login(request) ?: throw IllegalStateException("gRPC尚未初始化")
        }
    }
    
    suspend fun registerWorker(
        request: Nodepool.RegisterWorkerNodeRequest, 
        token: String
    ): Nodepool.RegisterWorkerNodeResponse {
        return if (isDemo) {
            // 使用模擬服務
            Log.d(TAG, "使用模擬服務註冊節點")
            mockService.registerWorkerNode(request, token)
        } else {
            val metadata = getAuthMetadata(token)
            nodeStub?.withInterceptors(MetadataUtils.newAttachHeadersInterceptor(metadata))
                ?.registerWorkerNode(request) ?: throw IllegalStateException("gRPC尚未初始化")
        }
    }
    
    suspend fun reportStatus(
        request: Nodepool.ReportStatusRequest, 
        token: String
    ): Nodepool.ReportStatusResponse {
        return if (isDemo) {
            // 使用模擬服務
            mockService.reportStatus(request, token)
        } else {
            val metadata = getAuthMetadata(token)
            nodeStub?.withInterceptors(MetadataUtils.newAttachHeadersInterceptor(metadata))
                ?.reportStatus(request) ?: throw IllegalStateException("gRPC尚未初始化")
        }
    }
    
    suspend fun getBalance(
        username: String, 
        token: String
    ): Nodepool.GetBalanceResponse {
        return if (isDemo) {
            // 使用模擬服務
            Log.d(TAG, "使用模擬服務獲取餘額")
            mockService.getBalance(username, token)
        } else {
            val request = Nodepool.GetBalanceRequest.newBuilder()
                .setUsername(username)
                .setToken(token)
                .build()
            
            val metadata = getAuthMetadata(token)
            userStub?.withInterceptors(MetadataUtils.newAttachHeadersInterceptor(metadata))
                ?.getBalance(request) ?: throw IllegalStateException("gRPC尚未初始化")
        }
    }
    
    private fun getAuthMetadata(token: String): Metadata {
        val metadata = Metadata()
        val key = Metadata.Key.of("authorization", Metadata.ASCII_STRING_MARSHALLER)
        metadata.put(key, "Bearer $token")
        return metadata
    }
    
    override fun close() {
        shutdown()
    }
    
    fun shutdown() {
        if (!isDemo) {
            channel.shutdown().awaitTermination(5, TimeUnit.SECONDS)
        }
    }
}
