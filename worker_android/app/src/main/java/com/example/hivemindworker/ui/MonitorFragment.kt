package com.example.hivemindworker.ui

import android.os.Bundle
import android.view.LayoutInflater
import android.view.View
import android.view.ViewGroup
import androidx.fragment.app.Fragment
import androidx.lifecycle.ViewModelProvider
import androidx.recyclerview.widget.LinearLayoutManager
import com.example.hivemindworker.R
import com.example.hivemindworker.databinding.FragmentMonitorBinding
import com.example.hivemindworker.viewmodel.WorkerViewModel
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

class MonitorFragment : Fragment() {
    private var _binding: FragmentMonitorBinding? = null
    private val binding get() = _binding!!
    
    private lateinit var viewModel: WorkerViewModel
    private lateinit var logsAdapter: LogsAdapter
    
    override fun onCreateView(
        inflater: LayoutInflater,
        container: ViewGroup?,
        savedInstanceState: Bundle?
    ): View {
        _binding = FragmentMonitorBinding.inflate(inflater, container, false)
        return binding.root
    }
    
    override fun onViewCreated(view: View, savedInstanceState: Bundle?) {
        super.onViewCreated(view, savedInstanceState)
        
        viewModel = ViewModelProvider(requireActivity())[WorkerViewModel::class.java]
        
        // 設定日誌列表
        logsAdapter = LogsAdapter()
        binding.logsRecyclerView.apply {
            layoutManager = LinearLayoutManager(context)
            adapter = logsAdapter
        }
        
        // 更新節點資訊
        updateNodeInfo()
        
        // 觀察狀態變化
        viewModel.status.observe(viewLifecycleOwner) { status ->
            binding.statusValue.text = status
        }
        
        // 觀察任務ID變化
        viewModel.currentTaskId.observe(viewLifecycleOwner) { taskId ->
            binding.taskIdValue.text = taskId ?: "無"
        }
        
        // 觀察CPT餘額變化
        viewModel.cptBalance.observe(viewLifecycleOwner) { balance ->
            binding.balanceValue.text = balance.toString()
        }
        
        // 觀察日誌變化
        viewModel.logs.observe(viewLifecycleOwner) { logs ->
            logsAdapter.submitList(logs)
            binding.logsRecyclerView.scrollToPosition(0)
        }
    }
    
    private fun updateNodeInfo() {
        binding.nodeIdValue.text = viewModel.nodeId.value ?: "未知"
        binding.cpuCoresValue.text = "${viewModel.cpuCores.value} 核心"
        binding.memoryValue.text = "${viewModel.memoryGb.value.format(1)}GB / ${viewModel.totalMemoryGb.value.format(1)}GB"
        binding.cpuScoreValue.text = viewModel.cpuScore.value.toString()
        binding.gpuScoreValue.text = viewModel.gpuScore.value.toString()
        binding.gpuNameValue.text = viewModel.gpuName.value
        binding.locationValue.text = viewModel.location.value
        
        // 更新登入時間
        val currentTime = SimpleDateFormat("yyyy-MM-dd HH:mm:ss", Locale.getDefault()).format(Date())
        binding.loginTimeValue.text = currentTime
    }
    
    private fun Double.format(digits: Int) = String.format("%.${digits}f", this)
    
    override fun onDestroyView() {
        super.onDestroyView()
        _binding = null
    }
}
