// prime_sieve.c - 高效能分段篩法實作（支援 OpenMP 多執行緒）
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <math.h>
#include <stdint.h>
#ifdef _OPENMP
#include <omp.h>
#endif

// 簡單篩法：生成 <= limit 的所有質數
static int* simple_sieve(int limit, int* count) {
    if (limit < 2) {
        *count = 0;
        return NULL;
    }
    
    char* is_prime = (char*)calloc(limit + 1, sizeof(char));
    memset(is_prime, 1, limit + 1);
    is_prime[0] = is_prime[1] = 0;
    
    int sqrt_limit = (int)sqrt(limit);
    for (int i = 2; i <= sqrt_limit; i++) {
        if (is_prime[i]) {
            for (int j = i * i; j <= limit; j += i) {
                is_prime[j] = 0;
            }
        }
    }
    
    // 計算質數數量
    int prime_count = 0;
    for (int i = 2; i <= limit; i++) {
        if (is_prime[i]) prime_count++;
    }
    
    // 分配並填充質數陣列
    int* primes = (int*)malloc(prime_count * sizeof(int));
    int idx = 0;
    for (int i = 2; i <= limit; i++) {
        if (is_prime[i]) {
            primes[idx++] = i;
        }
    }
    
    free(is_prime);
    *count = prime_count;
    return primes;
}

// 分段篩法：找出 [start, end) 範圍內的質數
// 使用回調函式逐批回傳，避免記憶體累積
// callback(primes_batch, batch_size, user_data)
typedef void (*prime_callback_t)(int* primes, int count, void* user_data);

void segmented_sieve_callback(int64_t start, int64_t end, prime_callback_t callback, void* user_data) {
    if (start >= end) return;
    if (start < 2) start = 2;
    
    // 小範圍直接用簡單篩法
    if (end - start <= 10000 && start == 2) {
        int count;
        int* primes = simple_sieve((int)(end - 1), &count);
        if (primes && count > 0) {
            callback(primes, count, user_data);
            free(primes);
        }
        return;
    }
    
    // 生成基礎質數
    int64_t limit = (int64_t)sqrt((double)end) + 1;
    int base_count;
    int* base_primes = simple_sieve((int)limit, &base_count);
    
    if (!base_primes) return;
    
    // 回報範圍內的小質數
    int small_prime_count = 0;
    int* small_primes = (int*)malloc(base_count * sizeof(int));
    for (int i = 0; i < base_count; i++) {
        if (base_primes[i] >= start && base_primes[i] < end) {
            small_primes[small_prime_count++] = base_primes[i];
        }
    }
    if (small_prime_count > 0) {
        callback(small_primes, small_prime_count, user_data);
    }
    free(small_primes);
    
    // 分段處理大範圍（每段最多 1M，避免記憶體爆炸）
    // 使用 OpenMP 並行化分段處理，用更小的分段確保負載均衡
    int64_t segment_size = 500000;  // 改為 50 萬一段，增加並行度
    if (end - start < segment_size) {
        segment_size = end - start;
    }
    
    int64_t start_seg = (start > limit) ? start : (limit + 1);
    int num_segments = (int)((end - start_seg + segment_size - 1) / segment_size);
    
    // 並行處理所有分段，dynamic scheduling 確保負載均衡
    #ifdef _OPENMP
    #pragma omp parallel for schedule(dynamic, 1)
    #endif
    for (int seg_idx = 0; seg_idx < num_segments; seg_idx++) {
        int64_t current = start_seg + seg_idx * segment_size;
        int64_t segment_end = current + segment_size;
        if (segment_end > end) segment_end = end;
        int64_t seg_len = segment_end - current;
        
        // 分配篩子
        char* is_prime = (char*)malloc(seg_len * sizeof(char));
        memset(is_prime, 1, seg_len);
        
        // 用基礎質數篩選 - 這是主要的計算熱點，也需要並行化
        for (int i = 0; i < base_count; i++) {
            int64_t p = base_primes[i];
            int64_t first_multiple = ((current + p - 1) / p) * p;
            if (first_multiple == p) first_multiple += p;
            
            // 內層循環並行化（當範圍夠大時）
            int64_t loop_len = (segment_end - first_multiple + p - 1) / p;
            if (loop_len > 10000) {
                // 大範圍用 SIMD 友好的循環
                #ifdef _OPENMP
                #pragma omp simd
                #endif
                for (int64_t j = first_multiple; j < segment_end; j += p) {
                    is_prime[j - current] = 0;
                }
            } else {
                // 小範圍直接循環
                for (int64_t j = first_multiple; j < segment_end; j += p) {
                    is_prime[j - current] = 0;
                }
            }
        }
        
        // 收集這段的質數並立即回調（不累積）
        int batch_size = 10000;
        int* batch = (int*)malloc(batch_size * sizeof(int));
        int batch_idx = 0;
        
        for (int64_t i = 0; i < seg_len; i++) {
            if (is_prime[i]) {
                batch[batch_idx++] = (int)(current + i);
                if (batch_idx >= batch_size) {
                    // 注意：callback 可能需要線程安全，這裡簡化處理
                    #ifdef _OPENMP
                    #pragma omp critical
                    #endif
                    callback(batch, batch_idx, user_data);
                    batch_idx = 0;
                }
            }
        }
        
        // 最後一批
        if (batch_idx > 0) {
            #ifdef _OPENMP
            #pragma omp critical
            #endif
            callback(batch, batch_idx, user_data);
        }
        
        free(batch);
        free(is_prime);
    }
    
    free(base_primes);
}

// 直接在 C 端計算 [start, end) 的質數數量與最大質數（不回傳完整 primes）
// out_max_prime: 若沒有質數則回傳 0
int64_t count_primes_max_parallel(int64_t start, int64_t end, int64_t* out_max_prime, int num_threads) {
    if (out_max_prime) {
        *out_max_prime = 0;
    }
    if (start >= end) return 0;
    if (start < 2) start = 2;

#ifdef _OPENMP
    if (num_threads > 0) {
        omp_set_num_threads(num_threads);
    }
#endif

    // 小範圍直接用 callback 版本就好（避免重寫太多特殊分支）
    if (end - start <= 10000 && start == 2) {
        int count = 0;
        int* primes = simple_sieve((int)(end - 1), &count);
        if (!primes || count <= 0) {
            if (primes) free(primes);
            return 0;
        }
        if (out_max_prime) {
            *out_max_prime = (int64_t)primes[count - 1];
        }
        free(primes);
        return (int64_t)count;
    }

    int64_t limit = (int64_t)sqrt((double)end) + 1;
    int base_count = 0;
    int* base_primes = simple_sieve((int)limit, &base_count);
    if (!base_primes) return 0;

    int64_t total_count = 0;
    int64_t max_prime = 0;

    // base_primes 內落在 [start, end) 的直接計入
    for (int i = 0; i < base_count; i++) {
        int p = base_primes[i];
        if ((int64_t)p >= start && (int64_t)p < end) {
            total_count += 1;
            if ((int64_t)p > max_prime) max_prime = (int64_t)p;
        }
    }

    int64_t segment_size = 500000;
    if (end - start < segment_size) {
        segment_size = end - start;
    }

    int64_t start_seg = (start > limit) ? start : (limit + 1);
    if (start_seg < start) start_seg = start;
    if (start_seg >= end) {
        free(base_primes);
        if (out_max_prime) *out_max_prime = max_prime;
        return total_count;
    }

    int num_segments = (int)((end - start_seg + segment_size - 1) / segment_size);

    int64_t seg_count_total = 0;
    int64_t seg_max_total = 0;

#ifdef _OPENMP
    #pragma omp parallel for schedule(dynamic, 1) reduction(+:seg_count_total) reduction(max:seg_max_total)
#endif
    for (int seg_idx = 0; seg_idx < num_segments; seg_idx++) {
        int64_t current = start_seg + (int64_t)seg_idx * segment_size;
        int64_t segment_end = current + segment_size;
        if (segment_end > end) segment_end = end;
        int64_t seg_len = segment_end - current;
        if (seg_len <= 0) continue;

        char* is_prime = (char*)malloc((size_t)seg_len * sizeof(char));
        if (!is_prime) continue;
        memset(is_prime, 1, (size_t)seg_len);

        for (int i = 0; i < base_count; i++) {
            int64_t p = (int64_t)base_primes[i];
            int64_t first_multiple = ((current + p - 1) / p) * p;
            if (first_multiple == p) first_multiple += p;
            for (int64_t j = first_multiple; j < segment_end; j += p) {
                is_prime[j - current] = 0;
            }
        }

        int64_t local_count = 0;
        int64_t local_max = 0;
        for (int64_t i = 0; i < seg_len; i++) {
            if (is_prime[i]) {
                local_count += 1;
                local_max = current + i;
            }
        }

        seg_count_total += local_count;
        if (local_max > seg_max_total) seg_max_total = local_max;

        free(is_prime);
    }

    total_count += seg_count_total;
    if (seg_max_total > max_prime) max_prime = seg_max_total;

    free(base_primes);
    if (out_max_prime) *out_max_prime = max_prime;
    return total_count;
}

// 導出給 Python 用的簡化介面：只計數
int64_t count_primes(int64_t start, int64_t end) {
    int64_t count = 0;
    
    void counter_callback(int* primes, int n, void* data) {
        int64_t* total = (int64_t*)data;
        *total += n;
    }
    
    segmented_sieve_callback(start, end, counter_callback, &count);
    return count;
}

// 導出給 Python 用的介面：回傳完整陣列（小心記憶體）
// 只用於小範圍測試
int* get_primes(int64_t start, int64_t end, int* out_count) {
    typedef struct {
        int* array;
        int capacity;
        int size;
    } prime_array_t;
    
    void collector_callback(int* primes, int n, void* data) {
        prime_array_t* arr = (prime_array_t*)data;
        if (arr->size + n > arr->capacity) {
            arr->capacity = (arr->size + n) * 2;
            arr->array = (int*)realloc(arr->array, arr->capacity * sizeof(int));
        }
        memcpy(arr->array + arr->size, primes, n * sizeof(int));
        arr->size += n;
    }
    
    prime_array_t result = {NULL, 0, 0};
    result.capacity = 10000;
    result.array = (int*)malloc(result.capacity * sizeof(int));
    
    segmented_sieve_callback(start, end, collector_callback, &result);
    
    *out_count = result.size;
    return result.array;
}

// 釋放陣列記憶體
void free_primes(int* primes) {
    free(primes);
}

// 多執行緒版本：使用 OpenMP 平行計算多個範圍
// ranges: [(start1, end1), (start2, end2), ...]
// count: 範圍數量
// num_threads: 執行緒數（0 = 自動）
int64_t count_primes_parallel(int64_t* ranges, int range_count, int num_threads) {
    if (range_count <= 0) return 0;
    
#ifdef _OPENMP
    if (num_threads > 0) {
        omp_set_num_threads(num_threads);
    }
#endif
    
    int64_t total_count = 0;
    
#ifdef _OPENMP
    #pragma omp parallel for reduction(+:total_count) schedule(dynamic)
#endif
    for (int i = 0; i < range_count; i++) {
        int64_t start = ranges[i * 2];
        int64_t end = ranges[i * 2 + 1];
        int64_t local_count = count_primes(start, end);
        total_count += local_count;
    }
    
    return total_count;
}

// 多執行緒版本：回傳所有質數（用於分散式計算單一區塊）
// 使用 OpenMP 加速單一大範圍的分段計算
int* get_primes_parallel(int64_t start, int64_t end, int* out_count, int num_threads) {
#ifdef _OPENMP
    if (num_threads > 0) {
        omp_set_num_threads(num_threads);
    }
#endif
    
    // 直接呼叫 get_primes（內部已經有 OpenMP 並行化）
    // 避免多層嵌套造成執行緒數稀釋
    return get_primes(start, end, out_count);
}
