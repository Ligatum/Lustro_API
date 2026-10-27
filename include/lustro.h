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
	LUSTRO_ERROR_INTERNAL_PANIC      = 6,
	/* 7..10: reported by the constructors */
	LUSTRO_ERROR_UNSUPPORTED_VERSION  = 7,
	LUSTRO_ERROR_INVALID_KIND         = 8,
	LUSTRO_ERROR_INVALID_CURSOR       = 9,
	LUSTRO_ERROR_OUT_OF_MEMORY        = 10,
	/* 11: lustro_configure; 12: lustro_configure and lustro_*_batch_lane */
	LUSTRO_ERROR_ALREADY_CONFIGURED   = 11,
	LUSTRO_ERROR_INVALID_ARGUMENT     = 12
} LustroError;

/**
 * Seed length in bytes.
 */
#define LUSTRO_SEED_LEN 32

/**
 * Output block length in bytes.
 */
#define LUSTRO_BLOCK_LEN 32

/**
 * `Hash128` length in bytes.
 */
#define LUSTRO_HASH128_LEN 16

/**
 * `Hash256` length in bytes.
 */
#define LUSTRO_HASH256_LEN 32

/**
 * Single-stream snapshot length in bytes.
 */
#define LUSTRO_SNAPSHOT_LEN 56

/**
 * Batch snapshot header length in bytes.
 */
#define LUSTRO_BATCH_SNAPSHOT_HEADER_LEN 16

/**
 * Batch snapshot length per lane in bytes.
 */
#define LUSTRO_BATCH_SNAPSHOT_LANE_LEN 48

/**
 * Native API version, returned by `lustro_api_version()`.
 */
#define LUSTRO_API_VERSION 1

/**
 * `simd` for `lustro_configure`: follow `LUSTRO_DISABLE_SIMD`.
 */
#define LUSTRO_SIMD_KEEP 0

/**
 * `simd` for `lustro_configure`: allow AVX2.
 */
#define LUSTRO_SIMD_AUTO 1

/**
 * `simd` for `lustro_configure`: scalar only.
 */
#define LUSTRO_SIMD_SCALAR 2

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
 * Returns the configured number of threads for parallel batches, 0 if batches
 * run on the calling thread. The pool is not started by this call.
 */
uintptr_t lustro_runtime_threads(void);

/**
 * Returns the stream backend as a static NUL-terminated string ("scalar" or
 * "avx2"). Never NULL; do not free.
 */
const char *lustro_runtime_backend(void);

/**
 * Sets threads and SIMD for this process, before first use. `threads`: 0 = not set,
 * 1 = no pool, larger values are capped at the logical CPU count. `simd`: one of
 * `LUSTRO_SIMD_*`. A value given here replaces the matching environment variables.
 *
 * Fixed once, by an earlier call, a batch call, `*_batch_suggested_steps` or
 * `lustro_runtime_*`. Later calls
 * return `ALREADY_CONFIGURED`, also with identical arguments. Unknown `simd` returns
 * `INVALID_ARGUMENT` and changes nothing.
 */
LustroError lustro_configure(uintptr_t threads, int32_t simd);

/**
 * Returns a static NUL-terminated description of an error code.
 * Unknown codes give "unknown error code". Never NULL; do not free.
 */
const char *lustro_strerror(int32_t code);

/**
 * Computes a 256-bit hash into `out`.
 * `out` must point to at least 32 bytes.
 *
 * # Safety
 * If `data_len > 0`, `data` must be valid for `data_len` readable bytes.
 * `out` must be valid for 32 writable bytes. The input and output regions
 * must not overlap.
 */
LustroError lustro_hash256(const uint8_t *data, uintptr_t data_len, uint8_t *out);

/**
 * Computes a 128-bit hash into `out`.
 * `out` must point to at least 16 bytes.
 *
 * # Safety
 * If `data_len > 0`, `data` must be valid for `data_len` readable bytes.
 * `out` must be valid for 16 writable bytes. The input and output regions
 * must not overlap.
 */
LustroError lustro_hash128(const uint8_t *data, uintptr_t data_len, uint8_t *out);

/**
 * Hashes `n` fixed-length messages into `out_ptr`.
 *
 * # Safety
 * If `n * message_len` is nonzero and representable, `data_ptr` must be
 * valid for `n * message_len` readable bytes.
 * If `n > 0` and the size calculation succeeds, `out_ptr` must be valid
 * for `n * 32` writable bytes.
 * The input and output regions must not overlap.
 */
LustroError lustro_hash256_many(const uint8_t *data_ptr,
                                uintptr_t n,
                                uintptr_t message_len,
                                uint8_t *out_ptr);

/**
 * Hashes `n` fixed-length messages into 128-bit digests.
 *
 * # Safety
 * If `n * message_len` is nonzero and representable, `data_ptr` must be
 * valid for `n * message_len` readable bytes.
 * If `n > 0` and the size calculation succeeds, `out_ptr` must be valid
 * for `n * 16` writable bytes.
 * The input and output regions must not overlap.
 */
LustroError lustro_hash128_many(const uint8_t *data_ptr,
                                uintptr_t n,
                                uintptr_t message_len,
                                uint8_t *out_ptr);

/**
 * Hashes `n` variable-length messages.
 * `message_ptrs[i]` must reference `message_lens[i]` bytes.
 * A null pointer is allowed when `message_lens[i] == 0`.
 *
 * # Safety
 * When `n > 0`, `message_ptrs` and `message_lens` must each be valid for
 * `n` readable elements.
 * Each `message_ptrs[i]` must be valid for `message_lens[i]` readable bytes
 * when `message_lens[i] > 0`.
 * `out_ptr` must be valid for `n * 32` writable bytes when that size is
 * representable.
 * The output region must not overlap either table or any nonempty message.
 */
LustroError lustro_hash256_many_var(const uint8_t *const *message_ptrs,
                                    uintptr_t n,
                                    const uintptr_t *message_lens,
                                    uint8_t *out_ptr);

/**
 * Hashes `n` variable-length messages into 128-bit digests.
 * Same pointer conventions as `lustro_hash256_many_var`.
 *
 * # Safety
 * When `n > 0`, `message_ptrs` and `message_lens` must each be valid for
 * `n` readable elements.
 * Each `message_ptrs[i]` must be valid for `message_lens[i]` readable bytes
 * when `message_lens[i] > 0`.
 * `out_ptr` must be valid for `n * 16` writable bytes when that size is
 * representable.
 * The output region must not overlap either table or any nonempty message.
 */
LustroError lustro_hash128_many_var(const uint8_t *const *message_ptrs,
                                    uintptr_t n,
                                    const uintptr_t *message_lens,
                                    uint8_t *out_ptr);

/**
 * Creates a PRNG context from a 32-byte seed and 128-bit stream ID.
 * `stream_id` is passed as `(hi, lo)` u64 values.
 * Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
 * for a null or misaligned `out_ctx`, or for a null `seed`.
 *
 * # Safety
 * If `seed` is non-null, it must be valid for 32 readable bytes.
 * If `out_ctx` is non-null, it must be valid for writing one pointer
 * and must not overlap any input buffer.
 */
LustroError lustro_prng_new(const uint8_t *seed,
                            uint64_t stream_id_hi,
                            uint64_t stream_id_lo,
                            struct LustroPrng **out_ctx);

/**
 * Frees a PRNG context. Passing null is safe (no-op).
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroPrng` context
 * allocated by this API. It must not have been freed already or be in use
 * concurrently.
 */
void lustro_prng_free(struct LustroPrng *ctx);

/**
 * Returns the next 32 random bits.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroPrng` context and
 * be exclusively accessible for the duration of the call.
 * If `out` is non-null, it must be valid for 4 writable bytes and must not
 * overlap the context.
 */
LustroError lustro_prng_next_u32(struct LustroPrng *ctx, uint32_t *out);

/**
 * Returns the next 64 random bits.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroPrng` context and
 * be exclusively accessible for the duration of the call.
 * If `out` is non-null, it must be valid for 8 writable bytes and must not
 * overlap the context.
 */
LustroError lustro_prng_next_u64(struct LustroPrng *ctx, uint64_t *out);

/**
 * Returns the next 128 random bits as 16 bytes (LE).
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroPrng` context and
 * be exclusively accessible for the duration of the call.
 * `out` must be valid for 16 writable bytes and must not overlap the context.
 */
LustroError lustro_prng_next_u128(struct LustroPrng *ctx, uint8_t *out);

/**
 * Writes the next full 32-byte block. Unread bytes of the current block are discarded.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroPrng` context and
 * be exclusively accessible for the duration of the call.
 * `out` must be valid for 32 writable bytes and must not overlap the context.
 */
LustroError lustro_prng_next_block(struct LustroPrng *ctx, uint8_t *out);

/**
 * Fills `out` with `out_len` random bytes.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroPrng` context and
 * be exclusively accessible for the duration of the call.
 * If `out_len > 0`, `out` must be valid for `out_len` writable bytes and
 * must not overlap the context.
 */
LustroError lustro_prng_fill(struct LustroPrng *ctx, uint8_t *out, uintptr_t out_len);

/**
 * Clones the PRNG context and returns a new independent instance.
 * Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
 * for a null or misaligned `out_ctx`, or for null `ctx`.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroPrng` context that
 * is not mutably accessed or freed during the call.
 * If `out_ctx` is non-null, it must be valid for writing one pointer
 * and must not overlap any input buffer or the memory of `ctx`.
 */
LustroError lustro_prng_clone(const struct LustroPrng *ctx, struct LustroPrng **out_ctx);

/**
 * Derives a child PRNG from the current state and 128-bit identifier.
 * `id` is passed as `(hi, lo)` u64 values.
 * Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
 * for a null or misaligned `out_ctx`, or for null `ctx`.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroPrng` context that
 * is not mutably accessed or freed during the call.
 * If `out_ctx` is non-null, it must be valid for writing one pointer
 * and must not overlap any input buffer or the memory of `ctx`.
 */
LustroError lustro_prng_fork(const struct LustroPrng *ctx,
                             uint64_t id_hi,
                             uint64_t id_lo,
                             struct LustroPrng **out_ctx);

/**
 * Derives a PRNG from a 32-byte seed along a path of `n` identifiers.
 * IDs are passed as parallel `(hi, lo)` u64 arrays.
 * Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
 * for a null or misaligned `out_ctx`, or for a null `seed` or a null or misaligned ID array (when
 * its length is nonzero), `InvalidLength` for `n == 0` or an `n` too large to address.
 *
 * # Safety
 * `seed` must be valid for 32 bytes.
 * `ids_hi` and `ids_lo` must each be valid for `n` elements when `n > 0`.
 * If `out_ctx` is non-null, it must be valid for writing one pointer
 * and must not overlap any input buffer.
 */
LustroError lustro_prng_derive_path(const uint8_t *seed,
                                    const uint64_t *ids_hi,
                                    const uint64_t *ids_lo,
                                    uintptr_t n,
                                    struct LustroPrng **out_ctx);

/**
 * Exports the current PRNG snapshot into `out`.
 * `out` must provide at least 56 writable bytes.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroPrng` context that
 * is not mutably accessed or freed during the call.
 * `out` must be valid for 56 writable bytes and must not overlap the context.
 */
LustroError lustro_prng_export_snapshot(const struct LustroPrng *ctx, uint8_t *out);

/**
 * Restores a PRNG context from a 56-byte snapshot.
 * Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
 * for a null or misaligned `out_ctx`, or for a null `bytes`; `UnsupportedVersion`, `InvalidKind`
 * or `InvalidCursor` for a rejected snapshot. The length is fixed at 56 bytes, so `InvalidLength`
 * is not reported.
 *
 * # Safety
 * If `bytes` is non-null, it must be valid for 56 readable bytes.
 * If `out_ctx` is non-null, it must be valid for writing one pointer
 * and must not overlap any input buffer.
 */
LustroError lustro_prng_import_snapshot(const uint8_t *bytes, struct LustroPrng **out_ctx);

/**
 * Creates a PRNG batch from `n` stream identifiers.
 * IDs are passed as parallel `(hi, lo)` u64 arrays.
 * Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
 * for a null or misaligned `out_ctx`, or for a null `seed` or a null or misaligned ID array (when
 * its length is nonzero), `InvalidLength` for an `n` too large to address.
 *
 * # Safety
 * `seed` must be valid for 32 readable bytes.
 * `ids_hi` and `ids_lo` must each be valid for `n` readable elements
 * when `n > 0`.
 * If `out_ctx` is non-null, it must be valid for writing one pointer
 * and must not overlap any input buffer.
 */
LustroError lustro_prng_batch_new(const uint8_t *seed,
                                  const uint64_t *ids_hi,
                                  const uint64_t *ids_lo,
                                  uintptr_t n,
                                  struct LustroPrngBatch **out_ctx);

/**
 * Creates `count` streams with sequential IDs starting at `first_stream_id`.
 * IDs wrap modulo 2^128. `first_stream_id` is passed as `(hi, lo)` u64 values.
 * Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
 * for a null or misaligned `out_ctx`, or for a null `seed`, `OutOfMemory` if the lane buffer
 * cannot be reserved (allocator refusal or capacity overflow).
 *
 * # Safety
 * `seed` must be valid for 32 readable bytes.
 * If `out_ctx` is non-null, it must be valid for writing one pointer
 * and must not overlap any input buffer.
 */
LustroError lustro_prng_batch_new_range(const uint8_t *seed,
                                        uint64_t first_hi,
                                        uint64_t first_lo,
                                        uintptr_t count,
                                        struct LustroPrngBatch **out_ctx);

/**
 * Frees a batch context. Passing null is safe (no-op).
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroPrngBatch` context
 * allocated by this API. It must not have been freed already or be in use
 * concurrently.
 */
void lustro_prng_batch_free(struct LustroPrngBatch *ctx);

/**
 * Clones the batch context and returns a new independent instance.
 * Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
 * for a null or misaligned `out_ctx`, or for null `ctx`, `OutOfMemory` if the lane buffer cannot
 * be reserved.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroPrngBatch` context
 * that is not mutably accessed or freed during the call.
 * If `out_ctx` is non-null, it must be valid for writing one pointer
 * and must not overlap any input buffer or the memory of `ctx`.
 */
LustroError lustro_prng_batch_clone(const struct LustroPrngBatch *ctx,
                                    struct LustroPrngBatch **out_ctx);

/**
 * Copies lane `index` of the batch as a single stream, positioned at the lane's next block.
 * Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
 * for a null or misaligned `out_ctx`, or for null `ctx`, `InvalidArgument` if `index >= len`.
 * The batch is not advanced and later calls on it do not affect the copy.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroPrngBatch` context
 * that is not mutably accessed or freed during the call.
 * If `out_ctx` is non-null, it must be valid for writing one pointer
 * and must not overlap any input buffer or the memory of `ctx`.
 */
LustroError lustro_prng_batch_lane(const struct LustroPrngBatch *ctx,
                                   uintptr_t index,
                                   struct LustroPrng **out_ctx);

/**
 * Returns the number of streams, or 0 for null `ctx`.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroPrngBatch` context
 * that is not mutably accessed or freed during the call.
 */
uintptr_t lustro_prng_batch_len(const struct LustroPrngBatch *ctx);

/**
 * Returns the suggested `steps` for `fill_blocks` at the batch length,
 * or 0 for null `ctx` or a caught panic. Speed hint only.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroPrngBatch` context
 * that is not mutably accessed or freed during the call.
 */
uintptr_t lustro_prng_batch_suggested_steps(const struct LustroPrngBatch *ctx);

/**
 * Advances all streams by `steps` stream steps.
 * `out_len` must equal `batch_len * steps * 32`.
 * Output is step-major: the block of `lane` at `step` starts at
 * byte offset `(step * batch_len + lane) * 32`.
 *
 * # Safety
 * `ctx` must point to a live `LustroPrngBatch` context and be exclusively
 * accessible for the duration of the call.
 * When the expected output length is nonzero, `out` must be valid for
 * that many writable bytes and must not overlap the context.
 */
LustroError lustro_prng_batch_fill_blocks(struct LustroPrngBatch *ctx,
                                          uint8_t *out,
                                          uintptr_t out_len,
                                          uintptr_t steps);

/**
 * Derives one child PRNG per lane from `n` child identifiers.
 * IDs are passed as parallel `(hi, lo)` u64 arrays.
 * `n` must equal the batch length.
 * Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
 * for a null or misaligned `out_ctx`, or for null `ctx` or a null or misaligned ID array (when
 * `n`/`k` is nonzero), `InvalidLength` if `n` differs from the batch length.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroPrngBatch` context
 * that is not mutably accessed or freed during the call.
 * `ids_hi` and `ids_lo` must each be valid for `n` readable elements
 * when `n > 0`.
 * If `out_ctx` is non-null, it must be valid for writing one pointer
 * and must not overlap any input buffer or the memory of `ctx`.
 */
LustroError lustro_prng_batch_fork(const struct LustroPrngBatch *ctx,
                                   const uint64_t *ids_hi,
                                   const uint64_t *ids_lo,
                                   uintptr_t n,
                                   struct LustroPrngBatch **out_ctx);

/**
 * Derives `k` children per lane. Output is parent-major:
 * child `j` of lane `i` is at index `i * k + j`.
 * IDs are passed as parallel `(hi, lo)` u64 arrays, one per child (length `k`).
 * Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
 * for a null or misaligned `out_ctx`, or for null `ctx` or a null or misaligned ID array (when
 * `n`/`k` is nonzero), `InvalidLength` if `len() * k` overflows or `k` is too large to address,
 * `OutOfMemory` if the lane buffer cannot be reserved.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroPrngBatch` context
 * that is not mutably accessed or freed during the call.
 * `ids_hi` and `ids_lo` must each be valid for `k` readable elements
 * when `k > 0`.
 * If `out_ctx` is non-null, it must be valid for writing one pointer
 * and must not overlap any input buffer or the memory of `ctx`.
 */
LustroError lustro_prng_batch_fork_many(const struct LustroPrngBatch *ctx,
                                        const uint64_t *ids_hi,
                                        const uint64_t *ids_lo,
                                        uintptr_t k,
                                        struct LustroPrngBatch **out_ctx);

/**
 * Derives one lane per root by walking each root along a path
 * of `n_path` identifiers.
 * Root and path IDs are passed as parallel `(hi, lo)` u64 arrays.
 * `n_path` must be nonzero; `n_roots` may be zero.
 * Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
 * for a null or misaligned `out_ctx`, or for a null `seed` or a null or misaligned ID array (when
 * its length is nonzero), `InvalidLength` for `n_path == 0` or a count too large to address,
 * `OutOfMemory` if the lane buffer cannot be reserved.
 *
 * # Safety
 * `seed` must be valid for 32 bytes.
 * `roots_hi` and `roots_lo` must each be valid for `n_roots` elements when
 * `n_roots > 0`. `path_hi` and `path_lo` must each be valid for `n_path`
 * elements when `n_path > 0`.
 * If `out_ctx` is non-null, it must be valid for writing one pointer
 * and must not overlap any input buffer.
 */
LustroError lustro_prng_batch_derive_path(const uint8_t *seed,
                                          const uint64_t *roots_hi,
                                          const uint64_t *roots_lo,
                                          uintptr_t n_roots,
                                          const uint64_t *path_hi,
                                          const uint64_t *path_lo,
                                          uintptr_t n_path,
                                          struct LustroPrngBatch **out_ctx);

/**
 * Derives one child PRNG per lane with sequential IDs starting at `first`.
 * IDs wrap modulo 2^128. `first` is passed as `(hi, lo)` u64 values.
 * Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
 * for a null or misaligned `out_ctx`, or for null `ctx`.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroPrngBatch` context
 * that is not mutably accessed or freed during the call.
 * If `out_ctx` is non-null, it must be valid for writing one pointer
 * and must not overlap any input buffer or the memory of `ctx`.
 */
LustroError lustro_prng_batch_fork_range(const struct LustroPrngBatch *ctx,
                                         uint64_t first_hi,
                                         uint64_t first_lo,
                                         struct LustroPrngBatch **out_ctx);

/**
 * Returns the snapshot size in bytes, or 0 for null `ctx`.
 * Size: `16 + batch_len * 48`.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroPrngBatch` context
 * that is not mutably accessed or freed during the call.
 */
uintptr_t lustro_prng_batch_snapshot_size(const struct LustroPrngBatch *ctx);

/**
 * Exports the current batch snapshot.
 * `out_len` must equal `lustro_prng_batch_snapshot_size(ctx)`.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroPrngBatch` context
 * that is not mutably accessed or freed during the call.
 * `out` must be valid for `out_len` writable bytes and must not overlap
 * the context. `out_len` must equal the expected snapshot size.
 */
LustroError lustro_prng_batch_export_snapshot(const struct LustroPrngBatch *ctx,
                                              uint8_t *out,
                                              uintptr_t out_len);

/**
 * Restores a PRNG batch from `len` snapshot bytes.
 * Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
 * for a null or misaligned `out_ctx`, or for a null `bytes` when `len > 0`; `InvalidLength`,
 * `UnsupportedVersion` or `InvalidKind` for a rejected snapshot.
 *
 * # Safety
 * `bytes` must be valid for `len` bytes when `len > 0`.
 * If `out_ctx` is non-null, it must be valid for writing one pointer
 * and must not overlap any input buffer.
 */
LustroError lustro_prng_batch_import_snapshot(const uint8_t *bytes,
                                              uintptr_t len,
                                              struct LustroPrngBatch **out_ctx);

/**
 * Creates an XOF context by absorbing a message.
 * Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
 * for a null or misaligned `out_ctx`, or for a null `message` when `message_len > 0`,
 * `InvalidLength` for a `message_len` too large to address.
 *
 * # Safety
 * If `message_len > 0`, `message` must be valid for `message_len` readable
 * bytes.
 * If `out_ctx` is non-null, it must be valid for writing one pointer
 * and must not overlap any input buffer.
 */
LustroError lustro_xof_new(const uint8_t *message,
                           uintptr_t message_len,
                           struct LustroXof **out_ctx);

/**
 * Frees an XOF context. Passing null is safe (no-op).
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroXof` context
 * allocated by this API. It must not have been freed already or be in use
 * concurrently.
 */
void lustro_xof_free(struct LustroXof *ctx);

/**
 * Returns the next 32 output bits.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroXof` context and
 * be exclusively accessible for the duration of the call.
 * If `out` is non-null, it must be valid for 4 writable bytes and must not
 * overlap the context.
 */
LustroError lustro_xof_next_u32(struct LustroXof *ctx, uint32_t *out);

/**
 * Returns the next 64 output bits.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroXof` context and
 * be exclusively accessible for the duration of the call.
 * If `out` is non-null, it must be valid for 8 writable bytes and must not
 * overlap the context.
 */
LustroError lustro_xof_next_u64(struct LustroXof *ctx, uint64_t *out);

/**
 * Returns the next 128 output bits as 16 bytes (LE).
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroXof` context and
 * be exclusively accessible for the duration of the call.
 * `out` must be valid for 16 writable bytes and must not overlap the context.
 */
LustroError lustro_xof_next_u128(struct LustroXof *ctx, uint8_t *out);

/**
 * Writes the next full 32-byte block. Unread bytes of the current block are discarded.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroXof` context and
 * be exclusively accessible for the duration of the call.
 * `out` must be valid for 32 writable bytes and must not overlap the context.
 */
LustroError lustro_xof_next_block(struct LustroXof *ctx, uint8_t *out);

/**
 * Fills `out` with `out_len` output bytes.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroXof` context and
 * be exclusively accessible for the duration of the call.
 * If `out_len > 0`, `out` must be valid for `out_len` writable bytes and
 * must not overlap the context.
 */
LustroError lustro_xof_fill(struct LustroXof *ctx, uint8_t *out, uintptr_t out_len);

/**
 * Clones the XOF context and returns a new independent instance.
 * Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
 * for a null or misaligned `out_ctx`, or for null `ctx`.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroXof` context that
 * is not mutably accessed or freed during the call.
 * If `out_ctx` is non-null, it must be valid for writing one pointer
 * and must not overlap any input buffer or the memory of `ctx`.
 */
LustroError lustro_xof_clone(const struct LustroXof *ctx, struct LustroXof **out_ctx);

/**
 * Derives a child XOF from the current state and 128-bit identifier.
 * `id` is passed as `(hi, lo)` u64 values.
 * Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
 * for a null or misaligned `out_ctx`, or for null `ctx`.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroXof` context that
 * is not mutably accessed or freed during the call.
 * If `out_ctx` is non-null, it must be valid for writing one pointer
 * and must not overlap any input buffer or the memory of `ctx`.
 */
LustroError lustro_xof_fork(const struct LustroXof *ctx,
                            uint64_t id_hi,
                            uint64_t id_lo,
                            struct LustroXof **out_ctx);

/**
 * Derives an XOF from a message along a path of `n` identifiers.
 * IDs are passed as parallel `(hi, lo)` u64 arrays.
 * Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
 * for a null or misaligned `out_ctx`, or for a null `message` when `message_len > 0` or a null or
 * misaligned ID array (when `n > 0`), `InvalidLength` for `n == 0` or a length too large to
 * address.
 *
 * # Safety
 * `message` must be valid for `message_len` bytes when `message_len > 0`.
 * `ids_hi` and `ids_lo` must each be valid for `n` elements when `n > 0`.
 * If `out_ctx` is non-null, it must be valid for writing one pointer
 * and must not overlap any input buffer.
 */
LustroError lustro_xof_derive_path(const uint8_t *message,
                                   uintptr_t message_len,
                                   const uint64_t *ids_hi,
                                   const uint64_t *ids_lo,
                                   uintptr_t n,
                                   struct LustroXof **out_ctx);

/**
 * Exports the current XOF snapshot into `out`.
 * `out` must provide at least 56 writable bytes.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroXof` context that
 * is not mutably accessed or freed during the call.
 * `out` must be valid for 56 writable bytes and must not overlap the context.
 */
LustroError lustro_xof_export_snapshot(const struct LustroXof *ctx, uint8_t *out);

/**
 * Restores an XOF context from a 56-byte snapshot.
 * Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
 * for a null or misaligned `out_ctx`, or for a null `bytes`; `UnsupportedVersion`, `InvalidKind`
 * or `InvalidCursor` for a rejected snapshot. The length is fixed at 56 bytes, so `InvalidLength`
 * is not reported.
 *
 * # Safety
 * If `bytes` is non-null, it must be valid for 56 readable bytes.
 * If `out_ctx` is non-null, it must be valid for writing one pointer
 * and must not overlap any input buffer.
 */
LustroError lustro_xof_import_snapshot(const uint8_t *bytes, struct LustroXof **out_ctx);

/**
 * Creates an XOF batch from `n` messages.
 * Messages are passed as parallel pointer/length arrays.
 * Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
 * for a null or misaligned `out_ctx`, or for a null or misaligned pointer/length array (when
 * `n > 0`) or a null message pointer with a nonzero length, `InvalidLength` for a length too large
 * to address.
 *
 * # Safety
 * When `n > 0`, `message_ptrs` and `message_lens` must each be valid for
 * `n` readable elements.
 * Each `message_ptrs[i]` must be valid for `message_lens[i]` readable bytes
 * when `message_lens[i] > 0`.
 * If `out_ctx` is non-null, it must be valid for writing one pointer
 * and must not overlap any input buffer.
 */
LustroError lustro_xof_batch_new(const uint8_t *const *message_ptrs,
                                 const uintptr_t *message_lens,
                                 uintptr_t n,
                                 struct LustroXofBatch **out_ctx);

/**
 * Frees a batch context. Passing null is safe (no-op).
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroXofBatch` context
 * allocated by this API. It must not have been freed already or be in use
 * concurrently.
 */
void lustro_xof_batch_free(struct LustroXofBatch *ctx);

/**
 * Clones the batch context and returns a new independent instance.
 * Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
 * for a null or misaligned `out_ctx`, or for null `ctx`, `OutOfMemory` if the lane buffer cannot
 * be reserved.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroXofBatch` context
 * that is not mutably accessed or freed during the call.
 * If `out_ctx` is non-null, it must be valid for writing one pointer
 * and must not overlap any input buffer or the memory of `ctx`.
 */
LustroError lustro_xof_batch_clone(const struct LustroXofBatch *ctx,
                                   struct LustroXofBatch **out_ctx);

/**
 * Copies lane `index` of the batch as a single stream, positioned at the lane's next block.
 * Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
 * for a null or misaligned `out_ctx`, or for null `ctx`, `InvalidArgument` if `index >= len`.
 * The batch is not advanced and later calls on it do not affect the copy.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroXofBatch` context
 * that is not mutably accessed or freed during the call.
 * If `out_ctx` is non-null, it must be valid for writing one pointer
 * and must not overlap any input buffer or the memory of `ctx`.
 */
LustroError lustro_xof_batch_lane(const struct LustroXofBatch *ctx,
                                  uintptr_t index,
                                  struct LustroXof **out_ctx);

/**
 * Returns the number of streams, or 0 for null `ctx`.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroXofBatch` context
 * that is not mutably accessed or freed during the call.
 */
uintptr_t lustro_xof_batch_len(const struct LustroXofBatch *ctx);

/**
 * Returns the suggested `steps` for `fill_blocks` at the batch length,
 * or 0 for null `ctx` or a caught panic. Speed hint only.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroXofBatch` context
 * that is not mutably accessed or freed during the call.
 */
uintptr_t lustro_xof_batch_suggested_steps(const struct LustroXofBatch *ctx);

/**
 * Advances all streams by `steps` stream steps.
 * `out_len` must equal `batch_len * steps * 32`.
 * Output is step-major: the block of `lane` at `step` starts at
 * byte offset `(step * batch_len + lane) * 32`.
 *
 * # Safety
 * `ctx` must point to a live `LustroXofBatch` context and be exclusively
 * accessible for the duration of the call.
 * When the expected output length is nonzero, `out` must be valid for
 * that many writable bytes and must not overlap the context.
 */
LustroError lustro_xof_batch_fill_blocks(struct LustroXofBatch *ctx,
                                         uint8_t *out,
                                         uintptr_t out_len,
                                         uintptr_t steps);

/**
 * Derives one child XOF per lane from `n` child identifiers.
 * IDs are passed as parallel `(hi, lo)` u64 arrays.
 * `n` must equal the batch length.
 * Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
 * for a null or misaligned `out_ctx`, or for null `ctx` or a null or misaligned ID array (when
 * `n`/`k` is nonzero), `InvalidLength` if `n` differs from the batch length.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroXofBatch` context
 * that is not mutably accessed or freed during the call.
 * `ids_hi` and `ids_lo` must each be valid for `n` readable elements
 * when `n > 0`.
 * If `out_ctx` is non-null, it must be valid for writing one pointer
 * and must not overlap any input buffer or the memory of `ctx`.
 */
LustroError lustro_xof_batch_fork(const struct LustroXofBatch *ctx,
                                  const uint64_t *ids_hi,
                                  const uint64_t *ids_lo,
                                  uintptr_t n,
                                  struct LustroXofBatch **out_ctx);

/**
 * Derives `k` children per lane. Output is parent-major:
 * child `j` of lane `i` is at index `i * k + j`.
 * IDs are passed as parallel `(hi, lo)` u64 arrays, one per child (length `k`).
 * Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
 * for a null or misaligned `out_ctx`, or for null `ctx` or a null or misaligned ID array (when
 * `n`/`k` is nonzero), `InvalidLength` if `len() * k` overflows or `k` is too large to address,
 * `OutOfMemory` if the lane buffer cannot be reserved.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroXofBatch` context
 * that is not mutably accessed or freed during the call.
 * `ids_hi` and `ids_lo` must each be valid for `k` readable elements
 * when `k > 0`.
 * If `out_ctx` is non-null, it must be valid for writing one pointer
 * and must not overlap any input buffer or the memory of `ctx`.
 */
LustroError lustro_xof_batch_fork_many(const struct LustroXofBatch *ctx,
                                       const uint64_t *ids_hi,
                                       const uint64_t *ids_lo,
                                       uintptr_t k,
                                       struct LustroXofBatch **out_ctx);

/**
 * Derives one lane per message by walking each stream along a path
 * of `n_path` identifiers. Messages are passed as parallel pointer/length
 * arrays; path IDs as parallel `(hi, lo)` u64 arrays.
 * `n_path` must be nonzero; `n_messages` may be zero.
 * Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
 * for a null or misaligned `out_ctx`, or for a null or misaligned argument array (when its length
 * is nonzero) or a null message pointer with a nonzero length, `InvalidLength` for `n_path == 0`
 * or a length too large to address, `OutOfMemory` if the lane buffer cannot be reserved.
 *
 * # Safety
 * `message_ptrs` and `message_lens` must each be valid for `n_messages`
 * elements when `n_messages > 0`. Each `message_ptrs[i]` must be valid for
 * `message_lens[i]` bytes if `message_lens[i] > 0`.
 * `path_hi` and `path_lo` must each be valid for `n_path` elements when
 * `n_path > 0`.
 * If `out_ctx` is non-null, it must be valid for writing one pointer
 * and must not overlap any input buffer.
 */
LustroError lustro_xof_batch_derive_path(const uint8_t *const *message_ptrs,
                                         const uintptr_t *message_lens,
                                         uintptr_t n_messages,
                                         const uint64_t *path_hi,
                                         const uint64_t *path_lo,
                                         uintptr_t n_path,
                                         struct LustroXofBatch **out_ctx);

/**
 * Derives one child XOF per lane with sequential IDs starting at `first`.
 * IDs wrap modulo 2^128. `first` is passed as `(hi, lo)` u64 values.
 * Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
 * for a null or misaligned `out_ctx`, or for null `ctx`.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroXofBatch` context
 * that is not mutably accessed or freed during the call.
 * If `out_ctx` is non-null, it must be valid for writing one pointer
 * and must not overlap any input buffer or the memory of `ctx`.
 */
LustroError lustro_xof_batch_fork_range(const struct LustroXofBatch *ctx,
                                        uint64_t first_hi,
                                        uint64_t first_lo,
                                        struct LustroXofBatch **out_ctx);

/**
 * Returns the snapshot size in bytes, or 0 for null `ctx`.
 * Size: `16 + batch_len * 48`.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroXofBatch` context
 * that is not mutably accessed or freed during the call.
 */
uintptr_t lustro_xof_batch_snapshot_size(const struct LustroXofBatch *ctx);

/**
 * Exports the current batch snapshot.
 * `out_len` must equal `lustro_xof_batch_snapshot_size(ctx)`.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroXofBatch` context
 * that is not mutably accessed or freed during the call.
 * `out` must be valid for `out_len` writable bytes and must not overlap
 * the context. `out_len` must equal the expected snapshot size.
 */
LustroError lustro_xof_batch_export_snapshot(const struct LustroXofBatch *ctx,
                                             uint8_t *out,
                                             uintptr_t out_len);

/**
 * Restores an XOF batch from `len` snapshot bytes.
 * Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
 * for a null or misaligned `out_ctx`, or for a null `bytes` when `len > 0`; `InvalidLength`,
 * `UnsupportedVersion` or `InvalidKind` for a rejected snapshot.
 *
 * # Safety
 * `bytes` must be valid for `len` bytes if len > 0.
 * If `out_ctx` is non-null, it must be valid for writing one pointer
 * and must not overlap any input buffer.
 */
LustroError lustro_xof_batch_import_snapshot(const uint8_t *bytes,
                                             uintptr_t len,
                                             struct LustroXofBatch **out_ctx);

/**
 * Byte offset of the block for `step` and `lane` in the output of
 * `lustro_*_batch_fill_blocks`: `(step * batch_len + lane) * LUSTRO_BLOCK_LEN`.
 * Preconditions: `step < steps`, `lane < batch_len`, and
 * `steps * batch_len * LUSTRO_BLOCK_LEN` fits in `size_t`.
 * The arguments are not validated and overflow is not checked.
 */
static inline size_t lustro_batch_block_offset(size_t step, size_t batch_len, size_t lane)
{
	return (step * batch_len + lane) * LUSTRO_BLOCK_LEN;
}

#ifdef __cplusplus
}
#endif
