/*
 * geo_repair.h - C API for the geo-repair library.
 *
 * Validate and repair invalid OGC GIS geometries (GEOS ST_MakeValid
 * parity). The C surface covers WKB (the GIS-native binary format) and
 * WKT (text), single geometries and parallel batches.
 *
 * Build the shared/static library with:
 *   cargo build --release --features ffi
 * then link against target/release/geo_repair.{dll,so,dylib} (or
 * libgeo_repair.a for static linking) and include this header.
 *
 * Panic safety: all functions are panic-safe. A Rust panic inside the
 * library is caught and surfaced as error_code = GeoRepairErrorCode_Panic.
 * The library MUST be built with panic=unwind for containment to work;
 * the shipped release profile uses unwind.
 *
 * Memory ownership: every result owns its buffers. Call the matching
 * geo_repair_free_* function when the result is no longer needed.
 * Double-free is harmless (the struct is zeroed).
 *
 * ABI stability: the struct layouts and GeoRepairErrorCode values are
 * fixed from 0.14.2. Adding codes/functions is additive; renumbering or
 * removing is a breaking ABI change.
 */
#ifndef GEO_REPAIR_H
#define GEO_REPAIR_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Programmatic error classification for every result. Fixed 1-byte width
 * (uint8_t) to match the Rust #[repr(u8)] field exactly - a C enum is
 * int-sized (4 bytes) and would misalign every struct on all platforms. */
#define GeoRepairErrorCode_None 0          /* operation succeeded */
#define GeoRepairErrorCode_Parse 1         /* input WKB/WKT could not be parsed */
#define GeoRepairErrorCode_InvalidInput 2  /* null pointer or invalid length */
#define GeoRepairErrorCode_InvalidGeometry 3 /* validation found violations */
#define GeoRepairErrorCode_Encode 4        /* output could not be encoded */
#define GeoRepairErrorCode_Panic 5         /* internal panic caught */
typedef uint8_t GeoRepairErrorCode;

/* WKB operation result. On success (success == true) wkb_data/wkb_len are
 * valid and error_msg is NULL. On failure error_code classifies the error
 * and error_msg carries the message. The validate_and_fix functions set
 * error_code = InvalidGeometry and error_msg even on success when the
 * input was invalid and had to be repaired. Free with
 * geo_repair_free_result. */
typedef struct {
    bool             success;
    GeoRepairErrorCode error_code;
    uint8_t*         wkb_data;  /* output WKB buffer (owned) */
    size_t           wkb_len;
    char*            error_msg; /* NUL-terminated, owned, may be NULL */
} GeoRepairResult;

/* WKT operation result. Same semantics as GeoRepairResult, with the
 * output in data/len as a NUL-terminated WKT string. Free with
 * geo_repair_free_string_result. */
typedef struct {
    bool             success;
    GeoRepairErrorCode error_code;
    char*            data;      /* output WKT string (owned, NUL-terminated) */
    size_t           len;       /* string length excluding the NUL */
    char*            error_msg; /* NUL-terminated, owned, may be NULL */
} GeoRepairStringResult;

/* Input buffer for the batch WKB API. */
typedef struct {
    const uint8_t*  data;
    size_t          len;
} GeoRepairWkbBuffer;

/* Batch WKB operation result. The call succeeds (success == true) when
 * every input was processed; per-item outcomes live in items[]. A
 * per-item parse failure has success == false and
 * error_code == GeoRepairErrorCode_Parse (the batch does not fail as a
 * whole). Free with geo_repair_free_batch_result. */
typedef struct {
    bool             success;
    GeoRepairErrorCode error_code;
    GeoRepairResult* items;    /* array of count results (owned) */
    size_t           count;
    char*            error_msg; /* NUL-terminated, owned, may be NULL */
} GeoRepairBatchResult;

/* --- Version --- */
/* Returns a static NUL-terminated version string; do NOT free. */
const char* geo_repair_version(void);

/* --- WKB: Repair --- */
/* poly_method: 0 = Auto, 1 = Arrange, 2 = Structure.
 * fill_rule: 0 = EvenOdd, 1 = NonZero. epsg_code <= 0 means unknown CRS. */
GeoRepairResult geo_repair_make_valid(const uint8_t* wkb_data, size_t wkb_len);
GeoRepairResult geo_repair_make_valid_with_config(
    const uint8_t* wkb_data, size_t wkb_len,
    bool keep_collapsed, uint8_t poly_method);
GeoRepairResult geo_repair_make_valid_with_config_full(
    const uint8_t* wkb_data, size_t wkb_len,
    bool keep_collapsed, uint8_t poly_method,
    uint8_t fill_rule, int32_t epsg_code);

/* --- WKB: Validation --- */
/* Returns 1 if the WKB geometry is OGC-valid, 0 otherwise (0 on parse
 * failure too). */
uint8_t geo_repair_is_valid(const uint8_t* wkb_data, size_t wkb_len);

/* success == true (wkb_len == 0) when valid; when invalid,
 * error_code == InvalidGeometry and error_msg carries the violation
 * reasons. */
GeoRepairResult geo_repair_validate(const uint8_t* wkb_data, size_t wkb_len);

/* Alias of geo_repair_validate, kept for callers that want the name to
 * state that error_msg carries the reasons. */
GeoRepairResult geo_repair_validate_reason(const uint8_t* wkb_data, size_t wkb_len);

/* --- WKB: Combined validate + fix --- */
/* Returns fixed WKB on success. error_msg is NULL and error_code == None
 * when the input was valid; when the input was repaired,
 * error_code == InvalidGeometry and error_msg carries the reasons. */
GeoRepairResult geo_repair_validate_and_fix(const uint8_t* wkb_data, size_t wkb_len);
GeoRepairResult geo_repair_validate_and_fix_with_config(
    const uint8_t* wkb_data, size_t wkb_len,
    bool keep_collapsed, uint8_t poly_method);

/* --- WKB: Batch --- */
/* Repair count WKB geometries. parallel != 0 enables the rayon batch
 * when the library was built with the parallel feature (sequential
 * otherwise). Per-item parse failures surface per item; the batch
 * succeeds. Free with geo_repair_free_batch_result. */
GeoRepairBatchResult geo_repair_make_valid_batch(
    const GeoRepairWkbBuffer* inputs, size_t count, int parallel);

/* --- WKT: Repair --- */
GeoRepairStringResult geo_repair_make_valid_wkt(const char* wkt);
GeoRepairStringResult geo_repair_make_valid_wkt_with_config(
    const char* wkt, bool keep_collapsed, uint8_t poly_method);
GeoRepairStringResult geo_repair_make_valid_wkt_with_config_full(
    const char* wkt, bool keep_collapsed, uint8_t poly_method,
    uint8_t fill_rule, int32_t epsg_code);

/* --- WKT: Validation --- */
/* Returns 1 if the WKT geometry is OGC-valid, 0 otherwise (0 on parse
 * failure too). */
uint8_t geo_repair_is_valid_wkt(const char* wkt);

/* success == true when valid; when invalid, error_code ==
 * InvalidGeometry and error_msg carries the violation reasons. */
GeoRepairStringResult geo_repair_validate_wkt(const char* wkt);

/* --- WKT: Combined validate + fix --- */
GeoRepairStringResult geo_repair_validate_and_fix_wkt(const char* wkt);
GeoRepairStringResult geo_repair_validate_and_fix_wkt_with_config(
    const char* wkt, bool keep_collapsed, uint8_t poly_method);

/* --- Memory management --- */
/* Releases the buffers owned by a result and zeroes the struct.
 * Double-free is harmless (the struct is zeroed). */
void geo_repair_free_result(GeoRepairResult* result);
void geo_repair_free_string_result(GeoRepairStringResult* result);
void geo_repair_free_batch_result(GeoRepairBatchResult* result);

#ifdef __cplusplus
}
#endif

#endif /* GEO_REPAIR_H */
