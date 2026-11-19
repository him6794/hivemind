#ifndef HIVEMIND_FFI_H
#define HIVEMIND_FFI_H

#include <stdbool.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct HivemindFfiClient HivemindFfiClient;

/** Returns a static null-terminated library version string (do NOT free). */
const char* hivemind_ffi_version(void);

/**
 * Probes local host CPU/SIMD/OS/memory capabilities and returns a heap-allocated
 * JSON string. Caller MUST release it via `hivemind_ffi_free_string`.
 */
char* hivemind_ffi_probe_hardware_json(void);

/**
 * Creates a client handle targeting `base_url` with bearer credential `api_key`.
 * Returns NULL if arguments are null or invalid. Release with `hivemind_ffi_destroy_client`.
 */
HivemindFfiClient* hivemind_ffi_create_client(const char* base_url, const char* api_key);

/** Destroys a client handle previously created by `hivemind_ffi_create_client`. */
void hivemind_ffi_destroy_client(HivemindFfiClient* client);

/**
 * Builds an OpenAI-compatible chat completion JSON request envelope with Hivemind
 * routing headers serialized as JSON. Caller MUST release via `hivemind_ffi_free_string`.
 */
char* hivemind_ffi_build_chat_envelope_json(
    const HivemindFfiClient* client,
    const char* model_id,
    const char* prompt,
    uint32_t max_tokens,
    bool stream,
    bool own_devices_only
);

/**
 * Evaluates mobile/tablet background-sharing safety rules (charging, Wi-Fi,
 * battery percent threshold, thermal ceiling). Returns 1 if safe to accept tasks, 0 otherwise.
 */
int32_t hivemind_ffi_mobile_guard_allows_sharing(
    bool is_charging,
    bool is_wifi,
    uint8_t battery_percent,
    uint8_t min_battery_percent,
    float temperature_celsius,
    float max_temperature_celsius
);

/** Releases a heap-allocated string returned by `hivemind_ffi_*`. */
void hivemind_ffi_free_string(char* ptr);

#ifdef __cplusplus
}
#endif

#endif /* HIVEMIND_FFI_H */
