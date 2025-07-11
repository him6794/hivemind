package com.example.hivemindworker.ui

import android.view.LayoutInflater
import android.view.ViewGroup
import androidx.recyclerview.widget.DiffUtil
import androidx.recyclerview.widget.ListAdapter
import androidx.recyclerview.widget.RecyclerView
import com.example.hivemindworker.databinding.ItemLogBinding

class LogsAdapter : ListAdapter<String, LogsAdapter.LogViewHolder>(LogDiffCallback()) {

    override fun onCreateViewHolder(parent: ViewGroup, viewType: Int): LogViewHolder {
        val binding = ItemLogBinding.inflate(
            LayoutInflater.from(parent.context),
            parent,
            false
        )
        return LogViewHolder(binding)
    }

    override fun onBindViewHolder(holder: LogViewHolder, position: Int) {
        holder.bind(getItem(position))
    }

    class LogViewHolder(private val binding: ItemLogBinding) : RecyclerView.ViewHolder(binding.root) {
        fun bind(log: String) {
            binding.logText.text = log
            
            // 根據日誌級別設置不同的顏色
            when {
                log.contains("ERROR") -> {
                    binding.logText.setTextColor(0xFFE57373.toInt()) // 紅色
                }
                log.contains("WARNING") -> {
                    binding.logText.setTextColor(0xFFFFB74D.toInt()) // 橙色
                }
                else -> {
                    binding.logText.setTextColor(0xFF000000.toInt()) // 黑色
                }
            }
        }
    }

    private class LogDiffCallback : DiffUtil.ItemCallback<String>() {
        override fun areItemsTheSame(oldItem: String, newItem: String): Boolean {
            return oldItem == newItem
        }

        override fun areContentsTheSame(oldItem: String, newItem: String): Boolean {
            return oldItem == newItem
        }
    }
}
