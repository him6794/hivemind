"""
計算能力與網路頻寬分析
i7-8700 CPU @ 3.20GHz × 40台
"""

# ==================== 硬體規格 ====================
CPU_MODEL = "Intel i7-8700"
CPU_CORES = 6
CPU_THREADS = 12
CPU_BASE_FREQ = 3.2  # GHz
NUM_MACHINES = 40

NETWORK_BANDWIDTH = 100  # Mbps (總共)
NETWORK_PER_MACHINE = NETWORK_BANDWIDTH / NUM_MACHINES  # Mbps

# ==================== 性能基準 ====================
# 根據實測：單線程處理 100萬個數字 (10^6) 約 0.169秒
BENCHMARK_RANGE = 1_000_000  # 10^6
BENCHMARK_TIME_SINGLE = 0.169  # 秒
BENCHMARK_PRIMES_FOUND = 78_498  # 找到的質數數量

# 多線程效率（考慮開銷，通常是核心數的 0.7-0.85 倍）
MULTITHREAD_EFFICIENCY = 0.75
EFFECTIVE_THREADS = CPU_THREADS * MULTITHREAD_EFFICIENCY

# ==================== 計算能力估算 ====================
def calculate_computing_power():
    """計算1小時內能處理的數字範圍"""
    
    # 單台機器的處理能力
    single_machine_speed = BENCHMARK_RANGE / BENCHMARK_TIME_SINGLE  # 數字/秒
    multi_machine_speed = single_machine_speed * EFFECTIVE_THREADS  # 考慮多線程
    
    # 所有機器的總處理能力
    total_speed = multi_machine_speed * NUM_MACHINES  # 數字/秒
    
    # 1小時能處理的數字
    numbers_per_hour = total_speed * 3600
    
    # 對應的10的幾次方
    import math
    power_of_10 = math.log10(numbers_per_hour)
    
    print("=" * 70)
    print("計算能力分析")
    print("=" * 70)
    print(f"CPU型號: {CPU_MODEL}")
    print(f"配置: {CPU_CORES}核{CPU_THREADS}線程 @ {CPU_BASE_FREQ}GHz")
    print(f"機器數量: {NUM_MACHINES} 台")
    print(f"多線程效率: {MULTITHREAD_EFFICIENCY*100:.0f}%")
    print(f"有效並行線程: {EFFECTIVE_THREADS:.1f} 線程/台")
    print()
    
    print("處理速度:")
    print(f"  單線程: {single_machine_speed:,.0f} 數字/秒")
    print(f"  單台機器 ({CPU_THREADS}線程): {multi_machine_speed:,.0f} 數字/秒")
    print(f"  集群總計 ({NUM_MACHINES}台): {total_speed:,.0f} 數字/秒")
    print()
    
    print("1小時處理能力:")
    print(f"  處理數字數量: {numbers_per_hour:,.0f}")
    print(f"  相當於: 10^{power_of_10:.2f}")
    print(f"  約等於: 1 到 10^{int(power_of_10)}")
    print()
    
    # 預估找到的質數數量（基於質數定理）
    # π(n) ≈ n / ln(n)
    primes_density = BENCHMARK_PRIMES_FOUND / BENCHMARK_RANGE
    estimated_primes = numbers_per_hour * primes_density
    
    print(f"預估找到質數: {estimated_primes:,.0f} 個")
    print()
    
    return {
        'total_speed': total_speed,
        'numbers_per_hour': numbers_per_hour,
        'power_of_10': power_of_10,
        'estimated_primes': estimated_primes
    }

# ==================== 網路頻寬分析 ====================
def calculate_network_capacity(computing_result):
    """分析網路頻寬是否足夠"""
    
    print("=" * 70)
    print("網路頻寬分析")
    print("=" * 70)
    print(f"總頻寬: {NETWORK_BANDWIDTH} Mbps")
    print(f"每台機器分配: {NETWORK_PER_MACHINE:.2f} Mbps = {NETWORK_PER_MACHINE/8:.2f} MB/s")
    print()
    
    # 任務數據大小估算
    task_download_size = 200  # bytes (JSON任務描述)
    
    # 計算每個任務的結果大小
    # 假設處理 10^10 個數字（標準區塊大小）
    chunk_size = 10_000_000_000  # 10^10
    primes_in_chunk = chunk_size * (BENCHMARK_PRIMES_FOUND / BENCHMARK_RANGE)
    
    # JSON格式：每個質數約10-15 bytes (含逗號、換行)
    bytes_per_prime = 12
    result_size = primes_in_chunk * bytes_per_prime  # bytes
    result_size_mb = result_size / (1024 * 1024)  # MB
    
    # 計算處理時間
    processing_time = chunk_size / computing_result['total_speed']  # 秒
    
    # 上傳時間
    bandwidth_bytes_per_sec = (NETWORK_PER_MACHINE / 8) * (1024 * 1024)  # bytes/s
    upload_time = result_size / bandwidth_bytes_per_sec  # 秒
    
    # 計算網路使用率
    network_utilization = (upload_time / processing_time) * 100
    
    print(f"單個任務 (10^10 數字):")
    print(f"  下載大小: {task_download_size} bytes")
    print(f"  結果大小: {result_size_mb:.2f} MB ({primes_in_chunk:,.0f} 個質數)")
    print(f"  處理時間: {processing_time:.1f} 秒")
    print(f"  上傳時間: {upload_time:.1f} 秒")
    print()
    
    print(f"網路使用率: {network_utilization:.1f}%")
    
    if network_utilization < 50:
        status = "非常充裕"
        color = "綠色"
    elif network_utilization < 80:
        status = "足夠"
        color = "黃色"
    elif network_utilization < 100:
        status = "接近飽和"
        color = "橙色"
    else:
        status = "不足，會成為瓶頸"
        color = "紅色"
    
    print(f"狀態: {status} ({color})")
    print()
    
    # 建議
    if network_utilization > 80:
        print("建議:")
        print("  1. 增加網路頻寬")
        print("  2. 壓縮結果數據")
        print("  3. 減少結果上傳頻率（批次處理）")
        print("  4. 只上傳質數數量統計，不上傳完整質數列表")
    else:
        print("結論: 100Mbps 頻寬對於 40 台機器來說完全足夠！")
        print(f"       即使所有機器同時上傳，仍有 {100-network_utilization:.1f}% 的頻寬餘裕")
    
    print()
    
    return {
        'network_utilization': network_utilization,
        'upload_time': upload_time,
        'processing_time': processing_time,
        'result_size_mb': result_size_mb
    }

# ==================== 具體計算範例 ====================
def calculate_specific_ranges():
    """計算不同次方需要的時間"""
    
    print("=" * 70)
    print("具體範圍計算時間估算")
    print("=" * 70)
    
    computing = calculate_computing_power()
    total_speed = computing['total_speed']
    
    test_cases = [
        (10, "10^10", 10**10),
        (12, "10^12 (1兆)", 10**12),
        (15, "10^15 (1千兆)", 10**15),
        (20, "10^20", 10**20),
        (30, "10^30", 10**30),
        (50, "10^50", 10**50),
        (100, "10^100", 10**100),
    ]
    
    for power, name, value in test_cases:
        time_seconds = value / total_speed
        time_minutes = time_seconds / 60
        time_hours = time_minutes / 60
        time_days = time_hours / 24
        time_years = time_days / 365
        
        print(f"\n範圍: 1 到 {name}")
        
        if time_seconds < 60:
            print(f"  時間: {time_seconds:.2f} 秒")
        elif time_minutes < 60:
            print(f"  時間: {time_minutes:.2f} 分鐘")
        elif time_hours < 24:
            print(f"  時間: {time_hours:.2f} 小時")
        elif time_days < 365:
            print(f"  時間: {time_days:.2f} 天")
        else:
            print(f"  時間: {time_years:.2f} 年")

# ==================== 主程式 ====================
if __name__ == "__main__":
    import sys
    
    print("\n")
    print("█" * 70)
    print("█" + " " * 68 + "█")
    print("█" + "    HiveMind 分散式質數計算系統 - 容量分析報告".center(68) + "█")
    print("█" + " " * 68 + "█")
    print("█" * 70)
    print("\n")
    
    # 計算處理能力
    computing_result = calculate_computing_power()
    
    # 計算網路容量
    network_result = calculate_network_capacity(computing_result)
    
    # 具體範例
    calculate_specific_ranges()
    
    print("\n")
    print("=" * 70)
    print("總結")
    print("=" * 70)
    print(f"✓ 40台 i7-8700 機器每小時可處理約 10^{int(computing_result['power_of_10'])} 個數字")
    print(f"✓ 100Mbps VPN 頻寬使用率約 {network_result['network_utilization']:.1f}%")
    
    if network_result['network_utilization'] < 80:
        print(f"✓ 網路頻寬完全足夠，還有 {100-network_result['network_utilization']:.1f}% 餘裕")
    else:
        print(f"⚠ 網路可能成為瓶頸，建議優化數據傳輸")
    
    print("=" * 70)
    print("\n")
