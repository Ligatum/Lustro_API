#pragma once
#include <stdint.h>
#include <stddef.h>
#ifdef __cplusplus
extern "C" {
#endif

typedef enum {
	LUSTRO_ERROR_OK                  = 0,
	LUSTRO_ERROR_INVALID_LENGTH      = 1,
	LUSTRO_ERROR_INVALID_POINTER     = 2,
	LUSTRO_ERROR_OUTPUT_TOO_SMALL    = 3,
	LUSTRO_ERROR_ALREADY_FINALISED   = 4,
	LUSTRO_ERROR_VERIFICATION_FAILED = 5,
	LUSTRO_ERROR_INTERNAL_PANIC      = 6
} LustroError;

#define LUSTRO_API_VERSION 1

typedef struct LustroPrng LustroPrng;

typedef struct LustroPrngBatch LustroPrngBatch;

typedef struct LustroXof LustroXof;

typedef struct LustroXofBatch LustroXofBatch;

uint32_t lustro_api_version(void);

LustroError lustro_hash256(const uint8_t *data, uintptr_t data_len, uint8_t *out);

LustroError lustro_hash128(const uint8_t *data, uintptr_t data_len, uint8_t *out);

LustroError lustro_hash256_many(const uint8_t *data_ptr,
                                uintptr_t n,
                                uintptr_t message_len,
                                uint8_t *out_ptr);

LustroError lustro_hash128_many(const uint8_t *data_ptr,
                                uintptr_t n,
                                uintptr_t message_len,
                                uint8_t *out_ptr);

/**
 * Hashes `n` variable-length messages.
 * `message_ptrs[i]` must reference `message_lens[i]` bytes.
 * A null pointer is allowed when `message_lens[i] == 0`.
 */
LustroError lustro_hash256_many_var(const uint8_t *const *message_ptrs,
                                    uintptr_t n,
                                    const uintptr_t *message_lens,
                                    uint8_t *out_ptr);

/**
 * Hashes `n` variable-length messages into 128-bit digests.
 * Same pointer conventions as `lustro_hash256_many_var`.
 */
LustroError lustro_hash128_many_var(const uint8_t *const *message_ptrs,
                                    uintptr_t n,
                                    const uintptr_t *message_lens,
                                    uint8_t *out_ptr);

struct LustroPrng *lustro_prng_new(const uint8_t *seed,
                                   uint64_t stream_id_hi,
                                   uint64_t stream_id_lo);

void lustro_prng_free(struct LustroPrng *ctx);

LustroError lustro_prng_next_u64(struct LustroPrng *ctx, uint64_t *out);

LustroError lustro_prng_next_u128(struct LustroPrng *ctx, uint8_t *out);

LustroError lustro_prng_next_block(struct LustroPrng *ctx, uint8_t *out);

LustroError lustro_prng_fill(struct LustroPrng *ctx, uint8_t *out, uintptr_t out_len);

struct LustroPrng *lustro_prng_clone(const struct LustroPrng *ctx);

struct LustroPrng *lustro_prng_fork(const struct LustroPrng *ctx, uint64_t id_hi, uint64_t id_lo);

LustroError lustro_prng_export_snapshot(const struct LustroPrng *ctx, uint8_t *out);

struct LustroPrng *lustro_prng_import_snapshot(const uint8_t *bytes);

struct LustroPrngBatch *lustro_prng_batch_new(const uint8_t *seed,
                                              const uint64_t *ids_hi,
                                              const uint64_t *ids_lo,
                                              uintptr_t n);

struct LustroPrngBatch *lustro_prng_batch_new_range(const uint8_t *seed,
                                                    uint64_t first_hi,
                                                    uint64_t first_lo,
                                                    uintptr_t count);

void lustro_prng_batch_free(struct LustroPrngBatch *ctx);

uintptr_t lustro_prng_batch_len(const struct LustroPrngBatch *ctx);

LustroError lustro_prng_batch_fill_blocks(struct LustroPrngBatch *ctx,
                                          uint8_t *out,
                                          uintptr_t out_len);

LustroError lustro_prng_batch_fill_blocks_many(struct LustroPrngBatch *ctx,
                                               uint8_t *out,
                                               uintptr_t out_len,
                                               uintptr_t steps);

struct LustroPrngBatch *lustro_prng_batch_fork(const struct LustroPrngBatch *ctx,
                                               const uint64_t *ids_hi,
                                               const uint64_t *ids_lo,
                                               uintptr_t n);

struct LustroPrngBatch *lustro_prng_batch_fork_range(const struct LustroPrngBatch *ctx,
                                                     uint64_t first_hi,
                                                     uint64_t first_lo);

uintptr_t lustro_prng_batch_snapshot_size(const struct LustroPrngBatch *ctx);

LustroError lustro_prng_batch_export_snapshot(const struct LustroPrngBatch *ctx,
                                              uint8_t *out,
                                              uintptr_t out_len);

struct LustroPrngBatch *lustro_prng_batch_import_snapshot(const uint8_t *bytes, uintptr_t len);

struct LustroXof *lustro_xof_new(const uint8_t *message, uintptr_t message_len);

void lustro_xof_free(struct LustroXof *ctx);

LustroError lustro_xof_next_u64(struct LustroXof *ctx, uint64_t *out);

LustroError lustro_xof_next_u128(struct LustroXof *ctx, uint8_t *out);

LustroError lustro_xof_next_block(struct LustroXof *ctx, uint8_t *out);

LustroError lustro_xof_fill(struct LustroXof *ctx, uint8_t *out, uintptr_t out_len);

struct LustroXof *lustro_xof_clone(const struct LustroXof *ctx);

struct LustroXof *lustro_xof_fork(const struct LustroXof *ctx, uint64_t id_hi, uint64_t id_lo);

LustroError lustro_xof_export_snapshot(const struct LustroXof *ctx, uint8_t *out);

struct LustroXof *lustro_xof_import_snapshot(const uint8_t *bytes);

struct LustroXofBatch *lustro_xof_batch_new(const uint8_t *const *message_ptrs,
                                            const uintptr_t *message_lens,
                                            uintptr_t n);

void lustro_xof_batch_free(struct LustroXofBatch *ctx);

uintptr_t lustro_xof_batch_len(const struct LustroXofBatch *ctx);

LustroError lustro_xof_batch_fill_blocks(struct LustroXofBatch *ctx,
                                         uint8_t *out,
                                         uintptr_t out_len);

LustroError lustro_xof_batch_fill_blocks_many(struct LustroXofBatch *ctx,
                                              uint8_t *out,
                                              uintptr_t out_len,
                                              uintptr_t steps);

struct LustroXofBatch *lustro_xof_batch_fork(const struct LustroXofBatch *ctx,
                                             const uint64_t *ids_hi,
                                             const uint64_t *ids_lo,
                                             uintptr_t n);

struct LustroXofBatch *lustro_xof_batch_fork_range(const struct LustroXofBatch *ctx,
                                                   uint64_t first_hi,
                                                   uint64_t first_lo);

uintptr_t lustro_xof_batch_snapshot_size(const struct LustroXofBatch *ctx);

LustroError lustro_xof_batch_export_snapshot(const struct LustroXofBatch *ctx,
                                             uint8_t *out,
                                             uintptr_t out_len);

struct LustroXofBatch *lustro_xof_batch_import_snapshot(const uint8_t *bytes, uintptr_t len);

#ifdef __cplusplus
}
#endif
