/*
 * test_geo_repair.c - C harness for the geo_repair FFI.
 *
 * Compile against the built library and the shipped header:
 *   Linux/macOS:  gcc test_geo_repair.c -I ../../include -L ../../target/release \
 *                   -l geo_repair -o test_geo_repair
 *                 LD_LIBRARY_PATH=../../target/release ./test_geo_repair
 *   Windows/MSVC: cl test_geo_repair.c /I ..\..\include /link ..\..\target\release\geo_repair.lib
 *                 (copy geo_repair.dll next to the exe first)
 *   Windows/MinGW: gcc test_geo_repair.c -I ../../include -L ../../target/release \
 *                    -l geo_repair -o test_geo_repair.exe
 *
 * Covers: version, is_valid, make_valid (all config depths), validate
 * (error codes + reasons), validate_and_fix, the WKT surface, batch
 * semantics (parallel and sequential), null/garbage input handling, and
 * free/double-free safety. Exits non-zero on the first failure.
 *
 * WKB fixtures are hand-built little-endian POLYGONs:
 *   square: POLYGON((0 0, 10 0, 10 10, 0 10, 0 0))   - valid
 *   bowtie: POLYGON((0 0, 5 5, 5 0, 0 5, 0 0))       - self-intersecting
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>

#include "geo_repair.h"

static int failures = 0;

#define CHECK(cond, msg)                                                       \
    do {                                                                       \
        if (!(cond)) {                                                         \
            fprintf(stderr, "FAIL: %s (line %d)\n", msg, __LINE__);            \
            failures++;                                                        \
        }                                                                      \
    } while (0)

static void check_result_valid(GeoRepairResult *r, const char *what)
{
    if (!r->success) {
        fprintf(stderr, "FAIL: %s returned success=false, code=%d msg=%s\n",
                what, (int)r->error_code, r->error_msg ? r->error_msg : "(null)");
        failures++;
        return;
    }
    if (r->error_code != GeoRepairErrorCode_None) {
        fprintf(stderr, "FAIL: %s error_code=%d on success\n", what,
                (int)r->error_code);
        failures++;
    }
}

/* Little-endian WKB polygon with a single ring of (x, y) pairs. */
static uint8_t *wkb_polygon(const double *pts, size_t n_pts, size_t *out_len)
{
    size_t len = 1 + 4 + 4 + 4 + n_pts * 2 * 8;
    uint8_t *buf = (uint8_t *)malloc(len);
    size_t off = 0;
    buf[off++] = 1; /* little endian */
    buf[off++] = 3; /* POLYGON */
    buf[off++] = 0; buf[off++] = 0; buf[off++] = 0;
    buf[off++] = 1; /* one ring */
    buf[off++] = 0; buf[off++] = 0; buf[off++] = 0;
    buf[off++] = (uint8_t)(n_pts & 0xff);
    buf[off++] = (uint8_t)((n_pts >> 8) & 0xff);
    buf[off++] = (uint8_t)((n_pts >> 16) & 0xff);
    buf[off++] = (uint8_t)((n_pts >> 24) & 0xff);
    for (size_t i = 0; i < n_pts; i++) {
        uint64_t bits;
        memcpy(&bits, &pts[i * 2], 8);
        for (int b = 0; b < 8; b++) buf[off++] = (uint8_t)(bits >> (8 * b));
        memcpy(&bits, &pts[i * 2 + 1], 8);
        for (int b = 0; b < 8; b++) buf[off++] = (uint8_t)(bits >> (8 * b));
    }
    *out_len = len;
    return buf;
}

static int is_valid_wkb_bytes(const uint8_t *data, size_t len)
{
    return geo_repair_is_valid(data, len) == 1;
}

int main(void)
{
    static const double square_pts[] = {0, 0, 10, 0, 10, 10, 0, 10, 0, 0};
    static const double bowtie_pts[] = {0, 0, 5, 5, 5, 0, 0, 5, 0, 0};
    size_t sq_len, bt_len;
    uint8_t *sq = wkb_polygon(square_pts, 5, &sq_len);
    uint8_t *bt = wkb_polygon(bowtie_pts, 5, &bt_len);
    uint8_t garbage[] = {0x01, 0x02, 0x03, 0x04};

    /* --- Version --- */
    const char *v = geo_repair_version();
    CHECK(v != NULL && strlen(v) > 0, "version must be non-empty");
    CHECK(strchr(v, '.') != NULL, "version must be semver-shaped");

    /* --- is_valid --- */
    CHECK(is_valid_wkb_bytes(sq, sq_len) == 1, "square must be valid");
    CHECK(is_valid_wkb_bytes(bt, bt_len) == 0, "bowtie must be invalid");
    CHECK(geo_repair_is_valid(garbage, sizeof(garbage)) == 0,
          "garbage must not crash is_valid");
    CHECK(geo_repair_is_valid(NULL, 0) == 0, "null input must not crash is_valid");

    /* --- make_valid --- */
    GeoRepairResult r = geo_repair_make_valid(bt, bt_len);
    check_result_valid(&r, "geo_repair_make_valid(bowtie)");
    CHECK(r.wkb_len > 0, "repaired output must have bytes");
    CHECK(is_valid_wkb_bytes(r.wkb_data, r.wkb_len) == 1,
          "repaired bowtie must be valid");
    geo_repair_free_result(&r);

    r = geo_repair_make_valid(sq, sq_len);
    check_result_valid(&r, "geo_repair_make_valid(square)");
    CHECK(is_valid_wkb_bytes(r.wkb_data, r.wkb_len) == 1,
          "valid square must stay valid");
    geo_repair_free_result(&r);

    /* --- make_valid with config (Auto/Arrange/Structure) --- */
    for (uint8_t method = 0; method <= 2; method++) {
        r = geo_repair_make_valid_with_config(bt, bt_len, 0, method);
        check_result_valid(&r, "make_valid_with_config");
        CHECK(is_valid_wkb_bytes(r.wkb_data, r.wkb_len) == 1,
              "configured repair must be valid");
        geo_repair_free_result(&r);
    }

    r = geo_repair_make_valid_with_config_full(bt, bt_len, 0, 1, 0, 0);
    check_result_valid(&r, "make_valid_with_config_full");
    geo_repair_free_result(&r);

    /* --- Error paths --- */
    r = geo_repair_make_valid(NULL, 0);
    CHECK(!r.success && r.error_code == GeoRepairErrorCode_InvalidInput,
          "null input must be InvalidInput");
    CHECK(r.error_msg != NULL, "error must carry a message");
    geo_repair_free_result(&r);

    r = geo_repair_make_valid(garbage, sizeof(garbage));
    CHECK(!r.success && r.error_code == GeoRepairErrorCode_Parse,
          "garbage must be Parse");
    geo_repair_free_result(&r);

    /* --- validate --- */
    r = geo_repair_validate(sq, sq_len);
    CHECK(r.success && r.error_code == GeoRepairErrorCode_None,
          "valid geometry must validate clean");
    CHECK(r.wkb_len == 0 && r.wkb_data == NULL, "valid validate must be empty");
    CHECK(r.error_msg == NULL, "valid validate must have no message");
    geo_repair_free_result(&r);

    r = geo_repair_validate(bt, bt_len);
    CHECK(!r.success && r.error_code == GeoRepairErrorCode_InvalidGeometry,
          "invalid geometry must report InvalidGeometry");
    CHECK(r.error_msg != NULL && strlen(r.error_msg) > 0,
          "invalid geometry must carry reasons");
    geo_repair_free_result(&r);

    /* --- validate_reason (alias of validate) --- */
    {
        GeoRepairResult a = geo_repair_validate(bt, bt_len);
        GeoRepairResult b = geo_repair_validate_reason(bt, bt_len);
        CHECK(a.success == b.success && a.error_code == b.error_code,
              "validate_reason must match validate");
        CHECK(strcmp(a.error_msg, b.error_msg) == 0,
              "validate_reason message must match validate");
        geo_repair_free_result(&a);
        geo_repair_free_result(&b);
    }

    /* --- validate_and_fix --- */
    r = geo_repair_validate_and_fix(bt, bt_len);
    CHECK(r.success, "validate_and_fix(bowtie)");
    CHECK(r.error_code == GeoRepairErrorCode_InvalidGeometry,
          "repaired invalid input must report InvalidGeometry");
    CHECK(r.error_msg != NULL, "repaired invalid input must carry reasons");
    CHECK(is_valid_wkb_bytes(r.wkb_data, r.wkb_len) == 1,
          "validate_and_fix output must be valid");
    geo_repair_free_result(&r);

    r = geo_repair_validate_and_fix(sq, sq_len);
    check_result_valid(&r, "validate_and_fix(square)");
    CHECK(r.error_code == GeoRepairErrorCode_None, "valid input: no error code");
    CHECK(r.error_msg == NULL, "valid input: no message");
    geo_repair_free_result(&r);

    r = geo_repair_validate_and_fix_with_config(bt, bt_len, 0, 2);
    CHECK(r.success, "validate_and_fix_with_config");
    CHECK(r.error_code == GeoRepairErrorCode_InvalidGeometry,
          "repaired invalid input must report InvalidGeometry");
    CHECK(is_valid_wkb_bytes(r.wkb_data, r.wkb_len) == 1,
          "configured validate_and_fix must be valid");
    geo_repair_free_result(&r);

    /* --- Double-free safety --- */
    r = geo_repair_make_valid(bt, bt_len);
    CHECK(r.success, "make_valid for double-free test");
    geo_repair_free_result(&r);
    geo_repair_free_result(&r); /* must be a no-op */
    geo_repair_free_result(NULL); /* must be a no-op */

    /* --- WKT surface --- */
    {
        GeoRepairStringResult sr = geo_repair_make_valid_wkt(
            "POLYGON((0 0, 5 5, 5 0, 0 5, 0 0))");
        CHECK(sr.success && sr.error_code == GeoRepairErrorCode_None,
              "make_valid_wkt must succeed");
        CHECK(sr.data != NULL && sr.len > 0, "WKT output must have data");
        CHECK(geo_repair_is_valid_wkt(sr.data) == 1,
              "repaired WKT must be valid");
        geo_repair_free_string_result(&sr);

        sr = geo_repair_make_valid_wkt_with_config(
            "POLYGON((0 0, 5 5, 5 0, 0 5, 0 0))", 0, 1);
        CHECK(sr.success, "make_valid_wkt_with_config must succeed");
        geo_repair_free_string_result(&sr);

        CHECK(geo_repair_is_valid_wkt("POLYGON((0 0, 10 0, 10 10, 0 10, 0 0))") == 1,
              "valid WKT must pass is_valid_wkt");
        CHECK(geo_repair_is_valid_wkt("POLYGON((0 0, 5 5, 5 0, 0 5, 0 0))") == 0,
              "bowtie WKT must fail is_valid_wkt");
        CHECK(geo_repair_is_valid_wkt(NULL) == 0, "null WKT must not crash");

        sr = geo_repair_validate_wkt("POLYGON((0 0, 10 0, 10 10, 0 10, 0 0))");
        CHECK(sr.success && sr.error_code == GeoRepairErrorCode_None,
              "valid WKT must validate clean");
        geo_repair_free_string_result(&sr);

        sr = geo_repair_validate_wkt("POLYGON((0 0, 5 5, 5 0, 0 5, 0 0))");
        CHECK(!sr.success && sr.error_code == GeoRepairErrorCode_InvalidGeometry,
              "invalid WKT must report InvalidGeometry");
        geo_repair_free_string_result(&sr);

        sr = geo_repair_validate_and_fix_wkt("POLYGON((0 0, 5 5, 5 0, 0 5, 0 0))");
        CHECK(sr.success, "validate_and_fix_wkt must succeed");
        CHECK(sr.error_code == GeoRepairErrorCode_InvalidGeometry,
              "repaired WKT must report InvalidGeometry");
        CHECK(geo_repair_is_valid_wkt(sr.data) == 1,
              "validate_and_fix_wkt output must be valid");
        geo_repair_free_string_result(&sr);

        sr = geo_repair_make_valid_wkt("NOTVALID");
        CHECK(!sr.success && sr.error_code == GeoRepairErrorCode_Parse,
              "bad WKT must be Parse");
        geo_repair_free_string_result(&sr);

        /* string double-free safety */
        sr = geo_repair_make_valid_wkt("POINT(1 2)");
        geo_repair_free_string_result(&sr);
        geo_repair_free_string_result(&sr);
        geo_repair_free_string_result(NULL);
    }

    /* --- Batch --- */
    {
        GeoRepairWkbBuffer inputs[3] = {
            {sq, sq_len},
            {bt, bt_len},
            {garbage, sizeof(garbage)},
        };
        for (int parallel = 0; parallel <= 1; parallel++) {
            GeoRepairBatchResult br =
                geo_repair_make_valid_batch(inputs, 3, parallel);
            CHECK(br.success, "batch must succeed");
            CHECK(br.error_code == GeoRepairErrorCode_None,
                  "batch must have no error code");
            CHECK(br.count == 3, "batch must have 3 items");
            CHECK(br.items != NULL, "batch must have items");
            CHECK(is_valid_wkb_bytes(br.items[0].wkb_data, br.items[0].wkb_len) == 1,
                  "batch item 0 (square) must be valid");
            CHECK(br.items[1].success, "batch item 1 (bowtie) must repair");
            CHECK(is_valid_wkb_bytes(br.items[1].wkb_data, br.items[1].wkb_len) == 1,
                  "batch item 1 output must be valid");
            CHECK(!br.items[2].success &&
                      br.items[2].error_code == GeoRepairErrorCode_Parse,
                  "batch item 2 (garbage) must be per-item Parse");
            geo_repair_free_batch_result(&br);
        }

        GeoRepairBatchResult br = geo_repair_make_valid_batch(NULL, 3, 0);
        CHECK(!br.success && br.error_code == GeoRepairErrorCode_InvalidInput,
              "null batch inputs must be InvalidInput");
        geo_repair_free_batch_result(&br);

        br = geo_repair_make_valid_batch(NULL, 0, 0);
        CHECK(br.success && br.count == 0, "empty batch must succeed");
        geo_repair_free_batch_result(&br);

        /* batch double-free safety */
        br = geo_repair_make_valid_batch(inputs, 1, 0);
        CHECK(br.success, "batch for double-free test");
        geo_repair_free_batch_result(&br);
        geo_repair_free_batch_result(&br);
    }

    free(sq);
    free(bt);

    if (failures == 0) {
        printf("ALL C FFI TESTS PASSED\n");
        return 0;
    }
    fprintf(stderr, "%d C FFI TEST(S) FAILED\n", failures);
    return 1;
}
