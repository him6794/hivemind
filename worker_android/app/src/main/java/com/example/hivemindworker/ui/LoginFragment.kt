package com.example.hivemindworker.ui

import android.os.Bundle
import android.view.LayoutInflater
import android.view.View
import android.view.ViewGroup
import android.widget.ArrayAdapter
import android.widget.Toast
import androidx.fragment.app.Fragment
import androidx.lifecycle.ViewModelProvider
import com.example.hivemindworker.R
import com.example.hivemindworker.databinding.FragmentLoginBinding
import com.example.hivemindworker.viewmodel.WorkerViewModel

class LoginFragment : Fragment() {
    private var _binding: FragmentLoginBinding? = null
    private val binding get() = _binding!!
    
    private lateinit var viewModel: WorkerViewModel
    
    override fun onCreateView(
        inflater: LayoutInflater,
        container: ViewGroup?,
        savedInstanceState: Bundle?
    ): View {
        _binding = FragmentLoginBinding.inflate(inflater, container, false)
        return binding.root
    }
    
    override fun onViewCreated(view: View, savedInstanceState: Bundle?) {
        super.onViewCreated(view, savedInstanceState)
        
        viewModel = ViewModelProvider(requireActivity())[WorkerViewModel::class.java]
        
        // 設定地區下拉選單
        val locations = arrayOf("亞洲", "非洲", "北美", "南美", "歐洲", "大洋洲", "Unknown")
        val adapter = ArrayAdapter(requireContext(), android.R.layout.simple_spinner_dropdown_item, locations)
        binding.locationSpinner.adapter = adapter
        
        // 設定當前地區
        viewModel.location.observe(viewLifecycleOwner) { location ->
            val index = locations.indexOf(location)
            if (index >= 0) {
                binding.locationSpinner.setSelection(index)
            }
        }
        
        // 設定狀態觀察
        viewModel.status.observe(viewLifecycleOwner) { status ->
            binding.statusText.text = "狀態: $status"
        }
        
        // 設定登入按鈕
        binding.loginButton.setOnClickListener {
            val username = binding.usernameInput.text.toString()
            val password = binding.passwordInput.text.toString()
            
            if (username.isBlank() || password.isBlank()) {
                Toast.makeText(requireContext(), "請輸入用戶名和密碼", Toast.LENGTH_SHORT).show()
                return@setOnClickListener
            }
            
            // 更新地區
            val selectedLocation = binding.locationSpinner.selectedItem.toString()
            viewModel.updateLocation(selectedLocation)
            
            // 執行登入
            binding.loginButton.isEnabled = false
            binding.loginProgress.visibility = View.VISIBLE
            
            viewModel.login(username, password)
        }
        
        // 觀察登入狀態變化
        viewModel.isLoggedIn.observe(viewLifecycleOwner) { isLoggedIn ->
            binding.loginButton.isEnabled = !isLoggedIn
            binding.loginProgress.visibility = if (isLoggedIn) View.GONE else View.INVISIBLE
        }
        
        // 觀察錯誤訊息
        viewModel.errorMessage.observe(viewLifecycleOwner) { errorMsg ->
            if (errorMsg.isNotEmpty()) {
                binding.loginButton.isEnabled = true
                binding.loginProgress.visibility = View.INVISIBLE
            }
        }
    }
    
    override fun onDestroyView() {
        super.onDestroyView()
        _binding = null
    }
}
