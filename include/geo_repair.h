/*
 * geo_repair.h — C API for the geo-repair library (WKB-based).
 *
 * Build the shared library with:
 *   cargo build --release --features ffi
 * then link against target/release/geo_repair.{dll,so,dylib} and include
 * this header.
 *
 * All functions are panic-safe: a Rust panic inside the library is caught
 * and surfaced as an error in GeoRepairResult. Every GeoRepairResult must
 * be released with geo_repair_free_result when no longer needed.
 */
#ifndef GEO_REPAIR_H
#define GEO_REPAIR_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct {
    bool      success;    /* true: wkb_data/wkb_len valid; false: error_msg set */
    uint8_t*  wkb_data;   /* output WKB buffer (allocated; free via geo_repair_free_result) */
    size_t    wkb_len;    /* length of wkb_data in bytes */
    char*     error_msg;  /* NUL-terminated error string when success == false */
} GeoRepairResult;

/* --- Version --- */
/* Returns a static NUL-terminated version string; do NOT free. */
const char* geo_repair_version(void);

/* --- Repair --- */
/* Repair a WKB geometry with default configuration (Auto method). */
GeoRepairResult geo_repair_make_valid(const uint8_t* wkb_data, size_t wkb_len);

/* poly_method: 0 = Auto, 1 = Arrange, 2 = Structure. */
GeoRepairResult geo_repair_make_valid_with_config(
    const uint8_t* wkb_data, size_t wkb_len,
    bool keep_collapsed, uint8_t poly_method);

/* fill_rule: 0 = EvenOdd, 1 = NonZero. epsg_code <= 0 means unknown CRS. */
GeoRepairResult geo_repair_make_valid_with_config_full(
    const uint8_t* wkb_data, size_t wkb_len,
    bool keep_collapsed, uint8_t poly_method,
    uint8_t fill_rule, int32_t epsg_code);

/* --- Validation --- */
/* Returns 1 if the WKB geometry is OGC-valid, 0 otherwise. */
uint8_t geo_repair_is_valid(const uint8_t* wkb_data, size_t wkb_len);

/* success == true (wkb_len == 0) when valid; error_msg set when invalid. */
GeoRepairResult geo_repair_validate(const uint8_t* wkb_data, size_t wkb_len);

/* error_msg carries the violation reason; success == false when invalid. */
GeoRepairResult geo_repair_validate_reason(const uint8_t* wkb_data, size_t wkb_len);

/* --- Combined validate + fix --- */
/* Returns fixed WKB on success. error_msg is NULL when the input was valid,
 * or contains validation errors when the input was repaired. */
GeoRepairResult geo_repair_validate_and_fix(const uint8_t* wkb_data, size_t wkb_len);
GeoRepairResult geo_repair_validate_and_fix_with_config(
    const uint8_t* wkb_data, size_t wkb_len,
    bool keep_collapsed, uint8_t poly_method);

/* --- Memory management --- */
/* Releases the buffers owned by a result and zeroes the struct.
 * Double-free is harmless (the struct is zeroed). */
void geo_repair_free_result(GeoRepairResult* result);

#ifdef __cplusplus
}
#endif

#endif /* GEO_REPAIR_H */
