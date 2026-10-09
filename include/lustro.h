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
	/* 3..5: part of the ABI, not returned by V1 functions */
	LUSTRO_ERROR_OUTPUT_TOO_SMALL    = 3,
	LUSTRO_ERROR_ALREADY_FINALISED   = 4,
	LUSTRO_ERROR_VERIFICATION_FAILED = 5,
	LUSTRO_ERROR_INTERNAL_PANIC      = 6
} LustroError;

/**
 * Native API version, returned by `lustro_api_version()`.
 */
#define LUSTRO_API_VERSION 1

/**
 * PRNG stream. Cloning preserves the exact stream state and future sequence.
 */
typedef struct LustroPrng LustroPrng;

/**
 * Independent PRNG streams advanced together by `fill_blocks`.
 */
typedef struct LustroPrngBatch LustroPrngBatch;

/**
 * XOF stream. Cloning preserves the exact stream state and future sequence.
 */
typedef struct LustroXof LustroXof;

/**
 * Independent XOF streams advanced together by `fill_blocks`.
 */
typedef struct LustroXofBatch LustroXofBatch;

uint32_t lustro_api_version(void);

/**
 * Computes a 256-bit hash into `out`.
 * `out` must point to at least 32 bytes.
 */
LustroError lustro_hash256(const uint8_t *data, uintptr_t data_len, uint8_t *out);

/**
 * Computes a 128-bit hash into `out`.
 * `out` must point to at least 16 bytes.
 */
LustroError lustro_hash128(const uint8_t *data, uintptr_t data_len, uint8_t *out);

/**
 * Hashes `n` fixed-length messages into `out_ptr`.
 */
LustroError lustro_hash256_many(const uint8_t *data_ptr,
                                uintptr_t n,
                                uintptr_t message_len,
                                uint8_t *out_ptr);

/**
 * Hashes `n` fixed-length messages into 128-bit digests.
 */
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

/**
 * Creates a PRNG context from a 32-byte seed and 128-bit stream ID.
 * `stream_id` is passed as `(hi, lo)` u64 values.
 * Returns null on invalid input.
 */
struct LustroPrng *lustro_prng_new(const uint8_t *seed,
                                   uint64_t stream_id_hi,
                                   uint64_t stream_id_lo);

/**
 * Frees a PRNG context. Passing null is safe (no-op).
 */
void lustro_prng_free(struct LustroPrng *ctx);

/**
 * Returns the next 64 random bits.
 */
LustroError lustro_prng_next_u64(struct LustroPrng *ctx, uint64_t *out);

/**
 * Returns the next 128 random bits as 16 bytes (LE).
 */
LustroError lustro_prng_next_u128(struct LustroPrng *ctx, uint8_t *out);

/**
 * Writes the next full 32-byte block. Unread bytes of the current block are discarded.
 */
LustroError lustro_prng_next_block(struct LustroPrng *ctx, uint8_t *out);

/**
 * Fills `out` with `out_len` random bytes.
 */
LustroError lustro_prng_fill(struct LustroPrng *ctx, uint8_t *out, uintptr_t out_len);

/**
 * Clones the PRNG context and returns a new independent instance.
 */
struct LustroPrng *lustro_prng_clone(const struct LustroPrng *ctx);

/**
 * Derives a child PRNG from the current state and 128-bit identifier.
 * `id` is passed as `(hi, lo)` u64 values.
 * Returns null on null `ctx`.
 */
struct LustroPrng *lustro_prng_fork(const struct LustroPrng *ctx, uint64_t id_hi, uint64_t id_lo);

/**
 * Derives a PRNG from a 32-byte seed along a path of `n` identifiers.
 * IDs are passed as parallel `(hi, lo)` u64 arrays.
 * Returns null on invalid input, including `n == 0`.
 *
 * # Safety
 * `seed` must be valid for 32 bytes.
 * `ids_hi` and `ids_lo` must each be valid for `n` elements when `n > 0`.
 */
struct LustroPrng *lustro_prng_derive_path(const uint8_t *seed,
                                           const uint64_t *ids_hi,
                                           const uint64_t *ids_lo,
                                           uintptr_t n);

/**
 * Exports the current PRNG snapshot into `out`.
 * `out` must provide at least 56 writable bytes.
 */
LustroError lustro_prng_export_snapshot(const struct LustroPrng *ctx, uint8_t *out);

/**
 * Restores a PRNG context from a 56-byte snapshot.
 * Returns null on invalid input or decoding failure.
 */
struct LustroPrng *lustro_prng_import_snapshot(const uint8_t *bytes);

/**
 * Creates a PRNG batch from `n` stream identifiers.
 * IDs are passed as parallel `(hi, lo)` u64 arrays.
 * Returns null on invalid input.
 *
 * # Safety
 * `ids_hi` and `ids_lo` must each be valid for `n` elements when `n > 0`.
 */
struct LustroPrngBatch *lustro_prng_batch_new(const uint8_t *seed,
                                              const uint64_t *ids_hi,
                                              const uint64_t *ids_lo,
                                              uintptr_t n);

/**
 * Creates `count` streams with sequential IDs starting at `first_stream_id`.
 * IDs wrap modulo 2^128. `first_stream_id` is passed as `(hi, lo)` u64 values.
 * Returns null on invalid input or allocation failure.
 */
struct LustroPrngBatch *lustro_prng_batch_new_range(const uint8_t *seed,
                                                    uint64_t first_hi,
                                                    uint64_t first_lo,
                                                    uintptr_t count);

/**
 * Frees a batch context. Passing null is safe (no-op).
 */
void lustro_prng_batch_free(struct LustroPrngBatch *ctx);

/**
 * Returns the number of streams, or 0 for null `ctx`.
 */
uintptr_t lustro_prng_batch_len(const struct LustroPrngBatch *ctx);

/**
 * Returns the suggested `steps` for `fill_blocks` at the batch length,
 * or 0 for null `ctx`. Speed hint only.
 */
uintptr_t lustro_prng_batch_suggested_steps(const struct LustroPrngBatch *ctx);

/**
 * Advances all streams by `steps` stream steps.
 * `out_len` must equal `batch_len * steps * 32`.
 * Output is step-major: the block of `lane` at `step` starts at
 * byte offset `(step * batch_len + lane) * 32`.
 */
LustroError lustro_prng_batch_fill_blocks(struct LustroPrngBatch *ctx,
                                          uint8_t *out,
                                          uintptr_t out_len,
                                          uintptr_t steps);

/**
 * Derives one child PRNG per lane from `n` child identifiers.
 * IDs are passed as parallel `(hi, lo)` u64 arrays.
 * `n` must equal the batch length.
 * Returns null on invalid input.
 *
 * # Safety
 * `ids_hi` and `ids_lo` must each be valid for `n` elements when `n > 0`.
 */
struct LustroPrngBatch *lustro_prng_batch_fork(const struct LustroPrngBatch *ctx,
                                               const uint64_t *ids_hi,
                                               const uint64_t *ids_lo,
                                               uintptr_t n);

/**
 * Derives `k` children per lane. Output is parent-major:
 * child `j` of lane `i` is at index `i * k + j`.
 * IDs are passed as parallel `(hi, lo)` u64 arrays, one per child (length `k`).
 * Returns null on invalid input, `len() * k` overflow, or allocation failure.
 *
 * # Safety
 * `ids_hi` and `ids_lo` must each be valid for `k` elements when `k > 0`.
 */
struct LustroPrngBatch *lustro_prng_batch_fork_many(const struct LustroPrngBatch *ctx,
                                                    const uint64_t *ids_hi,
                                                    const uint64_t *ids_lo,
                                                    uintptr_t k);

/**
 * Derives one lane per root by walking each root along a path
 * of `n_path` identifiers.
 * Root and path IDs are passed as parallel `(hi, lo)` u64 arrays.
 * `n_path` must be nonzero; `n_roots` may be zero.
 * Returns null on invalid input, including an empty path.
 *
 * # Safety
 * `seed` must be valid for 32 bytes.
 * `roots_hi` and `roots_lo` must each be valid for `n_roots` elements when
 * `n_roots > 0`. `path_hi` and `path_lo` must each be valid for `n_path`
 * elements when `n_path > 0`.
 */
struct LustroPrngBatch *lustro_prng_batch_derive_path(const uint8_t *seed,
                                                      const uint64_t *roots_hi,
                                                      const uint64_t *roots_lo,
                                                      uintptr_t n_roots,
                                                      const uint64_t *path_hi,
                                                      const uint64_t *path_lo,
                                                      uintptr_t n_path);

/**
 * Derives one child PRNG per lane with sequential IDs starting at `first`.
 * IDs wrap modulo 2^128. `first` is passed as `(hi, lo)` u64 values.
 * Returns null on null `ctx`.
 */
struct LustroPrngBatch *lustro_prng_batch_fork_range(const struct LustroPrngBatch *ctx,
                                                     uint64_t first_hi,
                                                     uint64_t first_lo);

/**
 * Returns the snapshot size in bytes, or 0 for null `ctx`.
 * Size: `16 + batch_len * 48`.
 */
uintptr_t lustro_prng_batch_snapshot_size(const struct LustroPrngBatch *ctx);

/**
 * Exports the current batch snapshot.
 * `out_len` must equal `lustro_prng_batch_snapshot_size(ctx)`.
 */
LustroError lustro_prng_batch_export_snapshot(const struct LustroPrngBatch *ctx,
                                              uint8_t *out,
                                              uintptr_t out_len);

/**
 * Restores a PRNG batch from `len` snapshot bytes.
 * Returns null on invalid input or decoding failure.
 *
 * # Safety
 * `bytes` must be valid for `len` bytes when `len > 0`.
 */
struct LustroPrngBatch *lustro_prng_batch_import_snapshot(const uint8_t *bytes, uintptr_t len);

/**
 * Creates an XOF context by absorbing a message.
 * Returns null on invalid input.
 */
struct LustroXof *lustro_xof_new(const uint8_t *message, uintptr_t message_len);

/**
 * Frees an XOF context. Passing null is safe (no-op).
 */
void lustro_xof_free(struct LustroXof *ctx);

/**
 * Returns the next 64 output bits.
 */
LustroError lustro_xof_next_u64(struct LustroXof *ctx, uint64_t *out);

/**
 * Returns the next 128 output bits as 16 bytes (LE).
 */
LustroError lustro_xof_next_u128(struct LustroXof *ctx, uint8_t *out);

/**
 * Writes the next full 32-byte block. Unread bytes of the current block are discarded.
 */
LustroError lustro_xof_next_block(struct LustroXof *ctx, uint8_t *out);

/**
 * Fills `out` with `out_len` output bytes.
 */
LustroError lustro_xof_fill(struct LustroXof *ctx, uint8_t *out, uintptr_t out_len);

/**
 * Clones the XOF context and returns a new independent instance.
 */
struct LustroXof *lustro_xof_clone(const struct LustroXof *ctx);

/**
 * Derives a child XOF from the current state and 128-bit identifier.
 * `id` is passed as `(hi, lo)` u64 values.
 * Returns null on null `ctx`.
 */
struct LustroXof *lustro_xof_fork(const struct LustroXof *ctx, uint64_t id_hi, uint64_t id_lo);

/**
 * Derives an XOF from a message along a path of `n` identifiers.
 * IDs are passed as parallel `(hi, lo)` u64 arrays.
 * Returns null on invalid input, including `n == 0`.
 *
 * # Safety
 * `message` must be valid for `message_len` bytes when `message_len > 0`.
 * `ids_hi` and `ids_lo` must each be valid for `n` elements when `n > 0`.
 */
struct LustroXof *lustro_xof_derive_path(const uint8_t *message,
                                         uintptr_t message_len,
                                         const uint64_t *ids_hi,
                                         const uint64_t *ids_lo,
                                         uintptr_t n);

/**
 * Exports the current XOF snapshot into `out`.
 * `out` must provide at least 56 writable bytes.
 */
LustroError lustro_xof_export_snapshot(const struct LustroXof *ctx, uint8_t *out);

/**
 * Restores an XOF context from a 56-byte snapshot.
 * Returns null on invalid input or decoding failure.
 */
struct LustroXof *lustro_xof_import_snapshot(const uint8_t *bytes);

/**
 * Creates an XOF batch from `n` messages.
 * Messages are passed as parallel pointer/length arrays.
 * Returns null on invalid input.
 *
 * # Safety
 * `message_ptrs` and `message_lens` must each be valid for `n` elements.
 * Each `message_ptrs[i]` must be valid for `message_lens[i]` bytes if
 * `message_lens[i] > 0`.
 */
struct LustroXofBatch *lustro_xof_batch_new(const uint8_t *const *message_ptrs,
                                            const uintptr_t *message_lens,
                                            uintptr_t n);

/**
 * Frees a batch context. Passing null is safe (no-op).
 */
void lustro_xof_batch_free(struct LustroXofBatch *ctx);

/**
 * Returns the number of streams, or 0 for null `ctx`.
 */
uintptr_t lustro_xof_batch_len(const struct LustroXofBatch *ctx);

/**
 * Returns the suggested `steps` for `fill_blocks` at the batch length,
 * or 0 for null `ctx`. Speed hint only.
 */
uintptr_t lustro_xof_batch_suggested_steps(const struct LustroXofBatch *ctx);

/**
 * Advances all streams by `steps` stream steps.
 * `out_len` must equal `batch_len * steps * 32`.
 * Output is step-major: the block of `lane` at `step` starts at
 * byte offset `(step * batch_len + lane) * 32`.
 */
LustroError lustro_xof_batch_fill_blocks(struct LustroXofBatch *ctx,
                                         uint8_t *out,
                                         uintptr_t out_len,
                                         uintptr_t steps);

/**
 * Derives one child XOF per lane from `n` child identifiers.
 * IDs are passed as parallel `(hi, lo)` u64 arrays.
 * `n` must equal the batch length.
 * Returns null on invalid input.
 *
 * # Safety
 * `ids_hi` and `ids_lo` must each be valid for `n` elements when n > 0.
 */
struct LustroXofBatch *lustro_xof_batch_fork(const struct LustroXofBatch *ctx,
                                             const uint64_t *ids_hi,
                                             const uint64_t *ids_lo,
                                             uintptr_t n);

/**
 * Derives `k` children per lane. Output is parent-major:
 * child `j` of lane `i` is at index `i * k + j`.
 * IDs are passed as parallel `(hi, lo)` u64 arrays, one per child (length `k`).
 * Returns null on invalid input, `len() * k` overflow, or allocation failure.
 *
 * # Safety
 * `ids_hi` and `ids_lo` must each be valid for `k` elements when `k > 0`.
 */
struct LustroXofBatch *lustro_xof_batch_fork_many(const struct LustroXofBatch *ctx,
                                                  const uint64_t *ids_hi,
                                                  const uint64_t *ids_lo,
                                                  uintptr_t k);

/**
 * Derives one lane per message by walking each stream along a path
 * of `n_path` identifiers. Messages are passed as parallel pointer/length
 * arrays; path IDs as parallel `(hi, lo)` u64 arrays.
 * `n_path` must be nonzero; `n_messages` may be zero.
 * Returns null on invalid input, including an empty path.
 *
 * # Safety
 * `message_ptrs` and `message_lens` must each be valid for `n_messages`
 * elements when `n_messages > 0`. Each `message_ptrs[i]` must be valid for
 * `message_lens[i]` bytes if `message_lens[i] > 0`.
 * `path_hi` and `path_lo` must each be valid for `n_path` elements when
 * `n_path > 0`.
 */
struct LustroXofBatch *lustro_xof_batch_derive_path(const uint8_t *const *message_ptrs,
                                                    const uintptr_t *message_lens,
                                                    uintptr_t n_messages,
                                                    const uint64_t *path_hi,
                                                    const uint64_t *path_lo,
                                                    uintptr_t n_path);

/**
 * Derives one child XOF per lane with sequential IDs starting at `first`.
 * IDs wrap modulo 2^128. `first` is passed as `(hi, lo)` u64 values.
 * Returns null on null `ctx`.
 */
struct LustroXofBatch *lustro_xof_batch_fork_range(const struct LustroXofBatch *ctx,
                                                   uint64_t first_hi,
                                                   uint64_t first_lo);

/**
 * Returns the snapshot size in bytes, or 0 for null `ctx`.
 * Size: `16 + batch_len * 48`.
 */
uintptr_t lustro_xof_batch_snapshot_size(const struct LustroXofBatch *ctx);

/**
 * Exports the current batch snapshot.
 * `out_len` must equal `lustro_xof_batch_snapshot_size(ctx)`.
 */
LustroError lustro_xof_batch_export_snapshot(const struct LustroXofBatch *ctx,
                                             uint8_t *out,
                                             uintptr_t out_len);

/**
 * Restores an XOF batch from `len` snapshot bytes.
 * Returns null on invalid input or decoding failure.
 *
 * # Safety
 * `bytes` must be valid for `len` bytes if len > 0.
 */
struct LustroXofBatch *lustro_xof_batch_import_snapshot(const uint8_t *bytes, uintptr_t len);

#ifdef __cplusplus
}
#endif
