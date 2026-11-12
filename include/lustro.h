#pragma once
#include <stdint.h>
#include <stddef.h>
#ifdef __cplusplus
extern "C" {
#endif

/* Fixed width, as in Rust (`#[repr(i32)]`); a C enum can be narrower, e.g. -fshort-enums. */
typedef int32_t LustroError;
enum {
	LUSTRO_ERROR_OK                  = 0,
	LUSTRO_ERROR_INVALID_LENGTH      = 1,
	LUSTRO_ERROR_INVALID_POINTER     = 2,
	/* 3..5: part of the ABI, not returned by V1 functions */
	LUSTRO_ERROR_OUTPUT_TOO_SMALL    = 3,
	LUSTRO_ERROR_ALREADY_FINALISED   = 4,
	LUSTRO_ERROR_VERIFICATION_FAILED = 5,
	LUSTRO_ERROR_INTERNAL_PANIC      = 6,
	/* 7..10: reported by the constructors (7 and 8 also by lustro_snapshot_kind) */
	LUSTRO_ERROR_UNSUPPORTED_VERSION  = 7,
	LUSTRO_ERROR_INVALID_KIND         = 8,
	LUSTRO_ERROR_INVALID_CURSOR       = 9,
	LUSTRO_ERROR_OUT_OF_MEMORY        = 10,
	/* 11: lustro_configure; 12: lustro_configure, lustro_*_batch_lane and
	 * lustro_*_batch_from_streams; 13: the entropy functions */
	LUSTRO_ERROR_ALREADY_CONFIGURED   = 11,
	LUSTRO_ERROR_INVALID_ARGUMENT     = 12,
	LUSTRO_ERROR_ENTROPY_UNAVAILABLE  = 13,
	/* 14: snapshot import */
	LUSTRO_ERROR_INVALID_RESERVED     = 14
};

/*
 * 128-bit values.
 * Inputs (stream IDs) are passed as two uint64_t: `hi` (upper 64 bits) first, then `lo`.
 * Arrays of IDs are parallel arrays `*_hi` and `*_lo` of the same length.
 * Outputs (`lustro_*_next_u128`, `lustro_*_position`) are 16 bytes, little-endian:
 * byte 0 is the lowest byte of `lo`, byte 15 the highest byte of `hi`.
 * The two helpers below convert between the forms. They do not depend on the host
 * byte order and do not check their pointers.
 */
static inline void lustro_u128_from_parts(uint64_t hi, uint64_t lo, uint8_t out[16])
{
	for (int i = 0; i < 8; i++) {
		out[i] = (uint8_t)(lo >> (8 * i));
		out[8 + i] = (uint8_t)(hi >> (8 * i));
	}
}

static inline void lustro_u128_to_parts(const uint8_t in[16], uint64_t *hi, uint64_t *lo)
{
	uint64_t h = 0, l = 0;
	for (int i = 7; i >= 0; i--) {
		l = (l << 8) | in[i];
		h = (h << 8) | in[8 + i];
	}
	*hi = h;
	*lo = l;
}

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
 * `*out_kind` of `lustro_snapshot_kind`: single PRNG stream.
 */
#define LUSTRO_SNAPSHOT_KIND_PRNG 1

/**
 * `*out_kind` of `lustro_snapshot_kind`: single XOF stream.
 */
#define LUSTRO_SNAPSHOT_KIND_XOF 2

/**
 * `*out_kind` of `lustro_snapshot_kind`: PRNG batch.
 */
#define LUSTRO_SNAPSHOT_KIND_PRNG_BATCH 3

/**
 * `*out_kind` of `lustro_snapshot_kind`: XOF batch.
 */
#define LUSTRO_SNAPSHOT_KIND_XOF_BATCH 4

/**
 * `hash256` of a message given in pieces. The length of the whole message enters the
 * initial state, so it is passed to `new`; `update` takes the bytes in pieces of any
 * size and `finalize` returns `hash256` of their concatenation. For a 128-bit digest
 * use `Hash256::first_128()`.
 *
 * Errors leave the hasher unchanged: `update` past the declared length rejects the
 * whole piece, and `finalize` before it is reached can be followed by more `update`.
 * `finalize` does not consume the hasher and gives the same digest on every call.
 */
typedef struct LustroHasher LustroHasher;

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
 * `LustroXof::new` of a message given in pieces. As in `LustroHasher`, the length of the
 * whole message enters the initial state and is passed to `new`; `update` takes the bytes
 * in pieces of any size and `finalize` returns the stream of their concatenation.
 *
 * Errors leave the absorber unchanged: `update` past the declared length rejects the
 * whole piece, and `finalize` before it is reached can be followed by more `update`.
 * `finalize` does not consume the absorber; every call returns the stream from its start.
 */
typedef struct LustroXofAbsorber LustroXofAbsorber;

/**
 * Independent XOF streams advanced together by `fill_blocks`.
 */
typedef struct LustroXofBatch LustroXofBatch;

/**
 * Execution settings of this process, as returned by `lustro_runtime_info`. Same fields as
 * `RuntimeInfo` in Rust. Flags are 0 or 1. The caller passes `sizeof` of this struct to
 * that function, so that fields added later do not overflow a struct built from an older
 * header.
 */
typedef struct LustroRuntimeInfo {
  /**
   * Configured pool size, not a count of active workers. 0 = batches run on the
   * calling thread.
   */
  uintptr_t threads;
  /**
   * Logical CPUs available to the process, detected at the call.
   */
  uintptr_t logical_cpus;
  /**
   * Backend for batches: "scalar" or "avx2". Static NUL-terminated string, never NULL,
   * do not free.
   */
  const char *backend;
  /**
   * 1 if the settings are fixed (by `lustro_configure` or a batch call); 0 before that.
   */
  uint8_t fixed;
  /**
   * 1 if built with the `parallel` feature.
   */
  uint8_t parallel_built;
  /**
   * 1 if the CPU has the SIMD instructions the crate uses (AVX2 today).
   */
  uint8_t simd_available;
  /**
   * 1 if the pool is built and this process can use it.
   */
  uint8_t pool_usable;
} LustroRuntimeInfo;

/**
 * Returns the native API version. It does not follow the crate version or the snapshot
 * format version.
 */
uint32_t lustro_api_version(void);

/**
 * Returns the configured pool size for parallel batches, not the number of active workers.
 * `0` means parallelism is disabled by settings or the crate was built without `parallel`.
 * This call does not start the pool and fixes nothing, like `lustro_runtime_info`. If
 * initialization fails, or this process inherited a pool-owner PID from its parent
 * through `fork`, batches fall back to the calling thread. Returns `0` also after a
 * caught internal panic.
 */
uintptr_t lustro_runtime_threads(void);

/**
 * Returns the backend for batches of streams and hashes as a static NUL-terminated
 * string ("scalar" or "avx2"). Never NULL; do not free. Fixes nothing, like
 * `lustro_runtime_info`. Returns "unknown" after a caught internal panic.
 */
const char *lustro_runtime_backend(void);

/**
 * Stores the execution settings of this process in `*out`. Like `runtime_info()` in Rust,
 * this fixes nothing: settings not yet fixed come from the environment as it is now
 * (`fixed` is 0) and may differ from the ones fixed later (see `lustro_configure`). Another
 * thread can fix them meanwhile, so the result can be stale on return. Does not start the pool.
 *
 * `out_size` is `sizeof(LustroRuntimeInfo)`. Only the fields that fit in it are written,
 * so a library newer than the header leaves the extra fields alone. Returns
 * `InvalidPointer` for a null or misaligned `out`, `InvalidLength` if `out_size` is
 * smaller than the first layout of the struct.
 *
 * # Safety
 * If `out` is non-null, it must be valid for writing `out_size` bytes.
 */
LustroError lustro_runtime_info(struct LustroRuntimeInfo *out, uintptr_t out_size);

/**
 * Sets threads and SIMD for this process, before first use. `threads`: 0 = not set,
 * 1 = no pool, larger values are capped at the logical CPU count. `simd`: one of
 * `LUSTRO_SIMD_*`. A value given here replaces the matching environment variables.
 *
 * Fixed once, by an earlier call or a batch call: a hash batch, `*_batch_fill_blocks`, or
 * `lustro_xof_batch_new` with `n > 0`. A later call returns `Ok` if the
 * arguments give the same settings as the fixed ones, else `AlreadyConfigured`. Unknown
 * `simd` returns `InvalidArgument` and changes nothing.
 *
 * The settings belong to the whole process. An application sets them early; a library that
 * uses Lustro leaves this call to the application and reads them with `lustro_runtime_info`.
 */
LustroError lustro_configure(uintptr_t threads, int32_t simd);

/**
 * Stores the kind of exported snapshot bytes in `*out_kind` (`LUSTRO_SNAPSHOT_KIND_*`).
 * Reads only the version and kind bytes, so `Ok` does not mean the import will succeed.
 * Returns `InvalidPointer` for a null `out_kind` or a null `bytes` when `len > 0`,
 * `InvalidLength` if `len < 2`, `UnsupportedVersion` or `InvalidKind` for an unknown byte.
 * Nothing is written on error.
 *
 * # Safety
 * `bytes` must be valid for `len` bytes when `len > 0`. If `out_kind` is non-null, it must
 * be valid for writing one byte and must not overlap `bytes`.
 */
LustroError lustro_snapshot_kind(const uint8_t *bytes, uintptr_t len, uint8_t *out_kind);

/**
 * Returns a static NUL-terminated description of an error code.
 * Unknown codes give "unknown error code". Never NULL; do not free.
 */
const char *lustro_strerror(int32_t code);

/**
 * Computes a 256-bit hash into `out`. Returns `InvalidLength` if `data_len` exceeds
 * isize::MAX, `InvalidPointer` for a null `out`, a null `data` with `data_len > 0`, or
 * overlapping input and output.
 *
 * # Safety
 * If `data_len > 0`, `data` must be valid for `data_len` readable bytes.
 * `out` must be valid for 32 writable bytes. The input and output regions
 * must not overlap.
 */
LustroError lustro_hash256(const uint8_t *data, uintptr_t data_len, uint8_t *out);

/**
 * Computes a 128-bit hash into `out`. Same statuses as `lustro_hash256`.
 *
 * # Safety
 * If `data_len > 0`, `data` must be valid for `data_len` readable bytes.
 * `out` must be valid for 16 writable bytes. The input and output regions
 * must not overlap.
 */
LustroError lustro_hash128(const uint8_t *data, uintptr_t data_len, uint8_t *out);

/**
 * Starts a `lustro_hash256` of a message of exactly `total_len` bytes, given in pieces
 * to `lustro_hasher_update`. Stores the new context in `*out_ctx` (null on failure) and
 * returns the status: `InvalidPointer` for a null or misaligned `out_ctx`.
 *
 * # Safety
 * If `out_ctx` is non-null, it must be valid for writing one pointer.
 */
LustroError lustro_hasher_new(uint64_t total_len, struct LustroHasher **out_ctx);

/**
 * Frees a hasher context. Passing null is safe (no-op).
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroHasher` context
 * allocated by this API. It must not have been freed already or be in use
 * concurrently.
 */
void lustro_hasher_free(struct LustroHasher *ctx);

/**
 * Absorbs the next `data_len` bytes of the message. Returns `InvalidLength` and absorbs
 * nothing if fewer than `data_len` bytes are left (or `data_len` exceeds isize::MAX);
 * `InvalidPointer` for a null `ctx`, a null `data` with `data_len > 0`, or `data`
 * overlapping the context.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroHasher` context and
 * be exclusively accessible for the duration of the call.
 * If `data_len > 0`, `data` must be valid for `data_len` readable bytes.
 */
LustroError lustro_hasher_update(struct LustroHasher *ctx, const uint8_t *data, uintptr_t data_len);

/**
 * Writes the 32-byte `lustro_hash256` of the message to `out`. Returns `InvalidLength`
 * while bytes are left (`out` is not written; more `lustro_hasher_update` calls can
 * follow) and `InvalidPointer` for a null `ctx` or `out`, or `out` overlapping the
 * context. The context is not changed: every call writes the same digest.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroHasher` context that
 * is not mutably accessed or freed during the call.
 * If `out` is non-null, it must be valid for 32 writable bytes.
 */
LustroError lustro_hasher_finalize256(const struct LustroHasher *ctx, uint8_t *out);

/**
 * Writes the number of bytes still expected before `lustro_hasher_finalize256` succeeds.
 * Returns `InvalidPointer` for a null `ctx` or `out_remaining`, or `out_remaining`
 * overlapping the context, writing nothing. `out_remaining` need not be aligned.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroHasher` context that
 * is not mutably accessed or freed during the call.
 * If `out_remaining` is non-null, it must be valid for writing one `uint64_t`.
 */
LustroError lustro_hasher_remaining(const struct LustroHasher *ctx, uint64_t *out_remaining);

/**
 * Clones the hasher context: the copy holds the same absorbed bytes and continues
 * independently. Stores the new context in `*out_ctx` (null on failure) and returns the
 * status: `InvalidPointer` for a null or misaligned `out_ctx`, or for null `ctx`. An
 * `out_ctx` overlapping the context is `InvalidPointer` too, and then `*out_ctx` is not
 * written.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroHasher` context that
 * is not mutably accessed or freed during the call.
 * If `out_ctx` is non-null, it must be valid for writing one pointer.
 */
LustroError lustro_hasher_clone(const struct LustroHasher *ctx, struct LustroHasher **out_ctx);

/**
 * Hashes `n` fixed-length messages into `out_ptr`. `n == 0` returns `Ok` and reads nothing.
 * Returns `InvalidLength` if `n * message_len` or `n * 32` overflows or exceeds isize::MAX,
 * `InvalidPointer` for a null `out_ptr`, a null `data_ptr` with a nonzero input size, or
 * overlapping input and output.
 *
 * # Safety
 * If `n * message_len` is nonzero and representable, `data_ptr` must be
 * valid for `n * message_len` readable bytes.
 * If `n > 0` and the size calculation succeeds, `out_ptr` must be valid
 * for `n * 32` writable bytes.
 * The input and output regions must not overlap.
 * The input bytes must stay valid and unmodified by any thread until the function returns;
 * they are read on the thread pool.
 */
LustroError lustro_hash256_many_fixed(const uint8_t *data_ptr,
                                      uintptr_t n,
                                      uintptr_t message_len,
                                      uint8_t *out_ptr);

/**
 * Hashes `n` fixed-length messages into 128-bit digests. Same statuses as
 * `lustro_hash256_many_fixed`, with `n * 16` output bytes.
 *
 * # Safety
 * If `n * message_len` is nonzero and representable, `data_ptr` must be
 * valid for `n * message_len` readable bytes.
 * If `n > 0` and the size calculation succeeds, `out_ptr` must be valid
 * for `n * 16` writable bytes.
 * The input and output regions must not overlap.
 * The input bytes must stay valid and unmodified by any thread until the function returns;
 * they are read on the thread pool.
 */
LustroError lustro_hash128_many_fixed(const uint8_t *data_ptr,
                                      uintptr_t n,
                                      uintptr_t message_len,
                                      uint8_t *out_ptr);

/**
 * Hashes `n` variable-length messages into 256-bit digests.
 * `message_ptrs[i]` must reference `message_lens[i]` bytes.
 * A null pointer is allowed when `message_lens[i] == 0`. `n == 0` returns `Ok` and reads
 * nothing. Returns `InvalidLength` for a size or message length that exceeds isize::MAX,
 * `InvalidPointer` for a null or misaligned table, a null `out_ptr`, a null message pointer
 * with a nonzero length, or an output that overlaps a table or a message.
 *
 * # Safety
 * When `n > 0`, `message_ptrs` and `message_lens` must each be valid for
 * `n` readable elements.
 * Each `message_ptrs[i]` must be valid for `message_lens[i]` readable bytes
 * when `message_lens[i] > 0`.
 * `out_ptr` must be valid for `n * 32` writable bytes when that size is
 * representable.
 * The output region must not overlap either table or any nonempty message.
 * The tables and the message bytes must stay valid and unmodified by any thread until the
 * function returns; they are read on the thread pool.
 */
LustroError lustro_hash256_many_var(const uint8_t *const *message_ptrs,
                                    const uintptr_t *message_lens,
                                    uintptr_t n,
                                    uint8_t *out_ptr);

/**
 * Hashes `n` variable-length messages into 128-bit digests.
 * Same pointer conventions and statuses as `lustro_hash256_many_var`.
 *
 * # Safety
 * When `n > 0`, `message_ptrs` and `message_lens` must each be valid for
 * `n` readable elements.
 * Each `message_ptrs[i]` must be valid for `message_lens[i]` readable bytes
 * when `message_lens[i] > 0`.
 * `out_ptr` must be valid for `n * 16` writable bytes when that size is
 * representable.
 * The output region must not overlap either table or any nonempty message.
 * The tables and the message bytes must stay valid and unmodified by any thread until the
 * function returns; they are read on the thread pool.
 */
LustroError lustro_hash128_many_var(const uint8_t *const *message_ptrs,
                                    const uintptr_t *message_lens,
                                    uintptr_t n,
                                    uint8_t *out_ptr);

/**
 * Creates a PRNG context from a 32-byte seed and 128-bit stream ID.
 * `stream_id` is passed as `(hi, lo)` u64 values, `hi` being the upper 64 bits.
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
 * Fills `out_seed` with 32 bytes from the operating system's entropy source.
 * Returns `InvalidPointer` for a null `out_seed`. If the source is unavailable it returns
 * `EntropyUnavailable` and `out_seed` is not written.
 *
 * # Safety
 * If `out_seed` is non-null, it must be valid for 32 writable bytes.
 */
LustroError lustro_seed_from_entropy(uint8_t *out_seed);

/**
 * As `lustro_prng_new`, with the seed taken from the operating system's entropy source.
 * Stores the new context in `*out_ctx` (null on failure). Returns `InvalidPointer` for a
 * null or misaligned `out_ctx`, `EntropyUnavailable` if the entropy source is unavailable.
 *
 * # Safety
 * If `out_ctx` is non-null, it must be valid for writing one pointer.
 */
LustroError lustro_prng_from_entropy(uint64_t stream_id_hi,
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
 * Writes the next 32 bits of the stream to `*out` (`out` need not be aligned).
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroPrng` context and
 * be exclusively accessible for the duration of the call.
 * If `out` is non-null, it must be valid for 4 writable bytes and must not
 * overlap the context.
 */
LustroError lustro_prng_next_u32(struct LustroPrng *ctx, uint32_t *out);

/**
 * Writes the next 64 bits of the stream to `*out` (`out` need not be aligned).
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroPrng` context and
 * be exclusively accessible for the duration of the call.
 * If `out` is non-null, it must be valid for 8 writable bytes and must not
 * overlap the context.
 */
LustroError lustro_prng_next_u64(struct LustroPrng *ctx, uint64_t *out);

/**
 * Writes the next 128 bits of the stream to `out` as 16 bytes (LE). `lustro_u128_to_parts` splits them
 * into `(hi, lo)`.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroPrng` context and
 * be exclusively accessible for the duration of the call.
 * `out` must be valid for 16 writable bytes and must not overlap the context.
 */
LustroError lustro_prng_next_u128(struct LustroPrng *ctx,
                                  uint8_t *out);

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
 * Fills `out` with the next `out_len` bytes of the stream.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroPrng` context and
 * be exclusively accessible for the duration of the call.
 * If `out_len > 0`, `out` must be valid for `out_len` writable bytes and
 * must not overlap the context.
 */
LustroError lustro_prng_fill(struct LustroPrng *ctx, uint8_t *out, uintptr_t out_len);

/**
 * Skips `n` bytes: the same state as `lustro_prng_fill` of `n` bytes, without writing them.
 * Costs O(n); there is no jump-ahead.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroPrng` context and
 * be exclusively accessible for the duration of the call.
 */
LustroError lustro_prng_advance_bytes(struct LustroPrng *ctx, uint64_t n);

/**
 * Skips `n` blocks: the same state as `n` calls of `lustro_prng_next_block`. Unread bytes of
 * the current block are discarded first (for `n > 0`); `n == 0` changes nothing.
 * Costs O(n); there is no jump-ahead.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroPrng` context and
 * be exclusively accessible for the duration of the call.
 */
LustroError lustro_prng_advance_blocks(struct LustroPrng *ctx, uint64_t n);

/**
 * Writes the bytes taken from the start of the stream as 16 bytes (LE); see
 * `lustro_u128_to_parts`. Bytes discarded by `lustro_prng_next_block` and
 * `lustro_prng_advance_blocks` count as taken. A fresh stream and a fork start at 0;
 * an imported snapshot continues from its own position. The value is meaningless after
 * 2^64 blocks, when the block counter wraps.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroPrng` context that
 * is not mutably accessed or freed during the call.
 * `out` must be valid for 16 writable bytes and must not overlap the context.
 */
LustroError lustro_prng_position(const struct LustroPrng *ctx, uint8_t *out);

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
 * The read position inside the current block does not affect the child.
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
 * Derives one child PRNG per identifier, as a batch: lane `i` is `lustro_prng_fork` with
 * `(ids_hi[i], ids_lo[i])`. The parent is not changed.
 * Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
 * for a null or misaligned `out_ctx`, or for null `ctx` or a null or misaligned ID array (when
 * `n > 0`), `InvalidLength` for an `n` too large to address, `OutOfMemory` if the identifiers or
 * the lane buffer cannot be allocated.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroPrng` context that
 * is not mutably accessed or freed during the call.
 * `ids_hi` and `ids_lo` must each be valid for `n` readable elements when `n > 0`.
 * If `out_ctx` is non-null, it must be valid for writing one pointer
 * and must not overlap any input buffer or the memory of `ctx`.
 */
LustroError lustro_prng_fork_batch(const struct LustroPrng *ctx,
                                   const uint64_t *ids_hi,
                                   const uint64_t *ids_lo,
                                   uintptr_t n,
                                   struct LustroPrngBatch **out_ctx);

/**
 * As `lustro_prng_fork_batch` with `count` sequential IDs starting at `first`, wrapping
 * modulo 2^128. `first` is passed as `(hi, lo)` u64 values.
 * Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
 * for a null or misaligned `out_ctx`, or for null `ctx`, `OutOfMemory` if the lane buffer cannot
 * be reserved.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroPrng` context that
 * is not mutably accessed or freed during the call.
 * If `out_ctx` is non-null, it must be valid for writing one pointer
 * and must not overlap any input buffer or the memory of `ctx`.
 */
LustroError lustro_prng_fork_batch_range(const struct LustroPrng *ctx,
                                         uint64_t first_hi,
                                         uint64_t first_lo,
                                         uintptr_t count,
                                         struct LustroPrngBatch **out_ctx);

/**
 * Derives a PRNG from the current state along a path of `n` identifiers: the same as
 * `lustro_prng_fork` for each ID in turn. IDs are passed as parallel `(hi, lo)` u64 arrays.
 * Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
 * for a null or misaligned `out_ctx`, or for null `ctx` or a null or misaligned ID array (when
 * `n > 0`), `InvalidLength` for `n == 0` or an `n` too large to address, `OutOfMemory` if the
 * identifier array cannot be allocated.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroPrng` context that
 * is not mutably accessed or freed during the call.
 * `ids_hi` and `ids_lo` must each be valid for `n` readable elements when `n > 0`.
 * If `out_ctx` is non-null, it must be valid for writing one pointer
 * and must not overlap any input buffer or the memory of `ctx`.
 */
LustroError lustro_prng_fork_path(const struct LustroPrng *ctx,
                                  const uint64_t *ids_hi,
                                  const uint64_t *ids_lo,
                                  uintptr_t n,
                                  struct LustroPrng **out_ctx);

/**
 * Derives a PRNG from a 32-byte seed along a path of `n` identifiers.
 * IDs are passed as parallel `(hi, lo)` u64 arrays.
 * Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
 * for a null or misaligned `out_ctx`, or for a null `seed` or a null or misaligned ID array (when
 * its length is nonzero), `InvalidLength` for `n == 0` or an `n` too large to address,
 * `OutOfMemory` if the identifier array cannot be allocated.
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
 * `out_len` must be 56 (`LUSTRO_SNAPSHOT_LEN`), otherwise `InvalidLength`.
 * The snapshot holds the full stream state: whoever has it can reproduce the rest of the
 * stream and its forks. Keep it as private as the seed.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroPrng` context that
 * is not mutably accessed or freed during the call.
 * `out` must be valid for `out_len` writable bytes and must not overlap the context.
 */
LustroError lustro_prng_export_snapshot(const struct LustroPrng *ctx,
                                        uint8_t *out,
                                        uintptr_t out_len);

/**
 * Restores a PRNG context from `len` snapshot bytes.
 * Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
 * for a null or misaligned `out_ctx`, or for a null `bytes` when `len > 0`; `InvalidLength`
 * when `len` is not 56 (`LUSTRO_SNAPSHOT_LEN`); `UnsupportedVersion`, `InvalidKind`,
 * `InvalidReserved` or `InvalidCursor` for a rejected snapshot.
 *
 * # Safety
 * `bytes` must be valid for `len` readable bytes when `len > 0`.
 * If `out_ctx` is non-null, it must be valid for writing one pointer
 * and must not overlap any input buffer.
 */
LustroError lustro_prng_import_snapshot(const uint8_t *bytes,
                                        uintptr_t len,
                                        struct LustroPrng **out_ctx);

/**
 * Creates a PRNG batch from `n` stream identifiers.
 * IDs are passed as parallel `(hi, lo)` u64 arrays.
 * Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
 * for a null or misaligned `out_ctx`, or for a null `seed` or a null or misaligned ID array (when
 * its length is nonzero), `InvalidLength` for an `n` too large to address, `OutOfMemory` if the
 * identifiers or the lane buffer cannot be allocated.
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
 * Creates a PRNG batch with one lane per stream, in order. Lane `i` continues
 * `streams[i]` at its next block; the streams are not changed.
 * Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
 * for a null or misaligned `out_ctx`, or for a null or misaligned `streams` array (when `n > 0`)
 * or element, `InvalidLength` for an `n` too large to address, `InvalidArgument` if a stream is
 * inside a block (part of its current block unread), `OutOfMemory` if the lane buffer cannot be
 * reserved.
 *
 * # Safety
 * When `n > 0`, `streams` must be valid for `n` readable elements, each pointing to a live
 * `LustroPrng` context that is not mutably accessed or freed during the call. The same context
 * may appear more than once.
 * If `out_ctx` is non-null, it must be valid for writing one pointer
 * and must not overlap any input buffer or the memory of the contexts.
 */
LustroError lustro_prng_batch_from_streams(const struct LustroPrng *const *streams,
                                           uintptr_t n,
                                           struct LustroPrngBatch **out_ctx);

/**
 * Returns the number of streams, or 0 for null `ctx`.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroPrngBatch` context
 * that is not mutably accessed or freed during the call.
 */
uintptr_t lustro_prng_batch_len(const struct LustroPrngBatch *ctx);

/**
 * Writes the number of streams to `*out_len`. Unlike `lustro_prng_batch_len`, a null `ctx`
 * is reported (`InvalidPointer`, also for a null `out_len`) instead of reading as an empty
 * batch. Nothing is written on error.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroPrngBatch` context
 * that is not mutably accessed or freed during the call.
 * If `out_len` is non-null, it must be valid for writing one `uintptr_t` and must not
 * overlap the context.
 */
LustroError lustro_prng_batch_get_len(const struct LustroPrngBatch *ctx, uintptr_t *out_len);

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
 * If `ctx` is non-null, it must point to a live `LustroPrngBatch` context and be exclusively
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
 * `n` is nonzero), `InvalidLength` if `n` differs from the batch length, `OutOfMemory` if the
 * identifiers or the lane buffer cannot be allocated.
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
 * `k` is nonzero), `InvalidLength` if `len() * k` overflows or `k` is too large to address,
 * `OutOfMemory` if the identifiers or the lane buffer cannot be reserved.
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
 * `OutOfMemory` if the identifiers or the lane buffer cannot be reserved.
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
 * for a null or misaligned `out_ctx`, or for null `ctx`, `OutOfMemory` if the lane buffer cannot
 * be reserved.
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
 * `out_len` must equal `lustro_prng_batch_snapshot_size(ctx)`, otherwise `InvalidLength`.
 * Writes straight into `out` and allocates nothing, so there is no `OutOfMemory`.
 * The snapshot holds every lane's state: whoever has it can reproduce the rest of each
 * stream and its forks. Keep it as private as the seed.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroPrngBatch` context
 * that is not mutably accessed or freed during the call.
 * `out` must be valid for `out_len` writable bytes and must not overlap
 * the context.
 */
LustroError lustro_prng_batch_export_snapshot(const struct LustroPrngBatch *ctx,
                                              uint8_t *out,
                                              uintptr_t out_len);

/**
 * Restores a PRNG batch from `len` snapshot bytes.
 * Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
 * for a null or misaligned `out_ctx`, or for a null `bytes` when `len > 0`; `InvalidLength`,
 * `UnsupportedVersion`, `InvalidKind` or `InvalidReserved` for a rejected snapshot;
 * `OutOfMemory` if the lane buffer cannot be allocated.
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
 * Writes the next 32 bits of the stream to `*out` (`out` need not be aligned).
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroXof` context and
 * be exclusively accessible for the duration of the call.
 * If `out` is non-null, it must be valid for 4 writable bytes and must not
 * overlap the context.
 */
LustroError lustro_xof_next_u32(struct LustroXof *ctx, uint32_t *out);

/**
 * Writes the next 64 bits of the stream to `*out` (`out` need not be aligned).
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroXof` context and
 * be exclusively accessible for the duration of the call.
 * If `out` is non-null, it must be valid for 8 writable bytes and must not
 * overlap the context.
 */
LustroError lustro_xof_next_u64(struct LustroXof *ctx, uint64_t *out);

/**
 * Writes the next 128 bits of the stream to `out` as 16 bytes (LE). `lustro_u128_to_parts` splits them
 * into `(hi, lo)`.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroXof` context and
 * be exclusively accessible for the duration of the call.
 * `out` must be valid for 16 writable bytes and must not overlap the context.
 */
LustroError lustro_xof_next_u128(struct LustroXof *ctx,
                                 uint8_t *out);

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
 * Fills `out` with the next `out_len` bytes of the stream.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroXof` context and
 * be exclusively accessible for the duration of the call.
 * If `out_len > 0`, `out` must be valid for `out_len` writable bytes and
 * must not overlap the context.
 */
LustroError lustro_xof_fill(struct LustroXof *ctx, uint8_t *out, uintptr_t out_len);

/**
 * Skips `n` bytes: the same state as `lustro_xof_fill` of `n` bytes, without writing them.
 * Costs O(n); there is no jump-ahead.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroXof` context and
 * be exclusively accessible for the duration of the call.
 */
LustroError lustro_xof_advance_bytes(struct LustroXof *ctx, uint64_t n);

/**
 * Skips `n` blocks: the same state as `n` calls of `lustro_xof_next_block`. Unread bytes of
 * the current block are discarded first (for `n > 0`); `n == 0` changes nothing.
 * Costs O(n); there is no jump-ahead.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroXof` context and
 * be exclusively accessible for the duration of the call.
 */
LustroError lustro_xof_advance_blocks(struct LustroXof *ctx, uint64_t n);

/**
 * Writes the bytes taken from the start of the stream as 16 bytes (LE); see
 * `lustro_u128_to_parts`. Bytes discarded by `lustro_xof_next_block` and
 * `lustro_xof_advance_blocks` count as taken. A fresh stream and a fork start at 0;
 * an imported snapshot continues from its own position. The value is meaningless after
 * 2^64 blocks, when the block counter wraps.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroXof` context that
 * is not mutably accessed or freed during the call.
 * `out` must be valid for 16 writable bytes and must not overlap the context.
 */
LustroError lustro_xof_position(const struct LustroXof *ctx, uint8_t *out);

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
 * The read position inside the current block does not affect the child.
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
 * Derives one child XOF per identifier, as a batch: lane `i` is `lustro_xof_fork` with
 * `(ids_hi[i], ids_lo[i])`. The parent is not changed.
 * Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
 * for a null or misaligned `out_ctx`, or for null `ctx` or a null or misaligned ID array (when
 * `n > 0`), `InvalidLength` for an `n` too large to address, `OutOfMemory` if the identifiers or
 * the lane buffer cannot be allocated.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroXof` context that
 * is not mutably accessed or freed during the call.
 * `ids_hi` and `ids_lo` must each be valid for `n` readable elements when `n > 0`.
 * If `out_ctx` is non-null, it must be valid for writing one pointer
 * and must not overlap any input buffer or the memory of `ctx`.
 */
LustroError lustro_xof_fork_batch(const struct LustroXof *ctx,
                                  const uint64_t *ids_hi,
                                  const uint64_t *ids_lo,
                                  uintptr_t n,
                                  struct LustroXofBatch **out_ctx);

/**
 * As `lustro_xof_fork_batch` with `count` sequential IDs starting at `first`, wrapping
 * modulo 2^128. `first` is passed as `(hi, lo)` u64 values.
 * Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
 * for a null or misaligned `out_ctx`, or for null `ctx`, `OutOfMemory` if the lane buffer cannot
 * be reserved.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroXof` context that
 * is not mutably accessed or freed during the call.
 * If `out_ctx` is non-null, it must be valid for writing one pointer
 * and must not overlap any input buffer or the memory of `ctx`.
 */
LustroError lustro_xof_fork_batch_range(const struct LustroXof *ctx,
                                        uint64_t first_hi,
                                        uint64_t first_lo,
                                        uintptr_t count,
                                        struct LustroXofBatch **out_ctx);

/**
 * Derives an XOF from the current state along a path of `n` identifiers: the same as
 * `lustro_xof_fork` for each ID in turn. IDs are passed as parallel `(hi, lo)` u64 arrays.
 * Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
 * for a null or misaligned `out_ctx`, or for null `ctx` or a null or misaligned ID array (when
 * `n > 0`), `InvalidLength` for `n == 0` or an `n` too large to address, `OutOfMemory` if the
 * identifier array cannot be allocated.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroXof` context that
 * is not mutably accessed or freed during the call.
 * `ids_hi` and `ids_lo` must each be valid for `n` readable elements when `n > 0`.
 * If `out_ctx` is non-null, it must be valid for writing one pointer
 * and must not overlap any input buffer or the memory of `ctx`.
 */
LustroError lustro_xof_fork_path(const struct LustroXof *ctx,
                                 const uint64_t *ids_hi,
                                 const uint64_t *ids_lo,
                                 uintptr_t n,
                                 struct LustroXof **out_ctx);

/**
 * Derives an XOF from a message along a path of `n` identifiers.
 * IDs are passed as parallel `(hi, lo)` u64 arrays.
 * Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
 * for a null or misaligned `out_ctx`, or for a null `message` when `message_len > 0` or a null or
 * misaligned ID array (when `n > 0`), `InvalidLength` for `n == 0` or a length too large to
 * address, `OutOfMemory` if the identifier array cannot be allocated.
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
 * `out_len` must be 56 (`LUSTRO_SNAPSHOT_LEN`), otherwise `InvalidLength`.
 * The snapshot holds the full stream state: whoever has it can reproduce the rest of the
 * stream and its forks. Keep it as private as the message.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroXof` context that
 * is not mutably accessed or freed during the call.
 * `out` must be valid for `out_len` writable bytes and must not overlap the context.
 */
LustroError lustro_xof_export_snapshot(const struct LustroXof *ctx,
                                       uint8_t *out,
                                       uintptr_t out_len);

/**
 * Restores an XOF context from `len` snapshot bytes.
 * Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
 * for a null or misaligned `out_ctx`, or for a null `bytes` when `len > 0`; `InvalidLength`
 * when `len` is not 56 (`LUSTRO_SNAPSHOT_LEN`); `UnsupportedVersion`, `InvalidKind`,
 * `InvalidReserved` or `InvalidCursor` for a rejected snapshot.
 *
 * # Safety
 * `bytes` must be valid for `len` readable bytes when `len > 0`.
 * If `out_ctx` is non-null, it must be valid for writing one pointer
 * and must not overlap any input buffer.
 */
LustroError lustro_xof_import_snapshot(const uint8_t *bytes,
                                       uintptr_t len,
                                       struct LustroXof **out_ctx);

/**
 * Starts a `lustro_xof_new` of a message of exactly `total_len` bytes, given in pieces
 * to `lustro_xof_absorber_update`. Stores the new context in `*out_ctx` (null on failure)
 * and returns the status: `InvalidPointer` for a null or misaligned `out_ctx`.
 *
 * # Safety
 * If `out_ctx` is non-null, it must be valid for writing one pointer.
 */
LustroError lustro_xof_absorber_new(uint64_t total_len, struct LustroXofAbsorber **out_ctx);

/**
 * Frees an absorber context. Passing null is safe (no-op).
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroXofAbsorber` context
 * allocated by this API. It must not have been freed already or be in use
 * concurrently.
 */
void lustro_xof_absorber_free(struct LustroXofAbsorber *ctx);

/**
 * Absorbs the next `data_len` bytes of the message. Returns `InvalidLength` and absorbs
 * nothing if fewer than `data_len` bytes are left (or `data_len` exceeds isize::MAX);
 * `InvalidPointer` for a null `ctx`, a null `data` with `data_len > 0`, or `data`
 * overlapping the context.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroXofAbsorber` context and
 * be exclusively accessible for the duration of the call.
 * If `data_len > 0`, `data` must be valid for `data_len` readable bytes.
 */
LustroError lustro_xof_absorber_update(struct LustroXofAbsorber *ctx,
                                       const uint8_t *data,
                                       uintptr_t data_len);

/**
 * Creates the XOF context `lustro_xof_new` would give for the whole message, at the start
 * of the stream; free it with `lustro_xof_free`. Stores it in `*out_ctx` (null on failure)
 * and returns the status: `InvalidLength` while bytes are left (more
 * `lustro_xof_absorber_update` calls can follow), `InvalidPointer` for a null or
 * misaligned `out_ctx`, or for null `ctx`. An `out_ctx` overlapping the context is
 * `InvalidPointer` too, and then `*out_ctx` is not written. The absorber is not changed:
 * every call gives the same stream.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroXofAbsorber` context that
 * is not mutably accessed or freed during the call.
 * If `out_ctx` is non-null, it must be valid for writing one pointer.
 */
LustroError lustro_xof_absorber_finalize(const struct LustroXofAbsorber *ctx,
                                         struct LustroXof **out_ctx);

/**
 * Writes the number of bytes still expected before `lustro_xof_absorber_finalize`
 * succeeds. Returns `InvalidPointer` for a null `ctx` or `out_remaining`, or
 * `out_remaining` overlapping the context, writing nothing. `out_remaining` need not be
 * aligned.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroXofAbsorber` context that
 * is not mutably accessed or freed during the call.
 * If `out_remaining` is non-null, it must be valid for writing one `uint64_t`.
 */
LustroError lustro_xof_absorber_remaining(const struct LustroXofAbsorber *ctx,
                                          uint64_t *out_remaining);

/**
 * Clones the absorber context: the copy holds the same absorbed bytes and continues
 * independently. Stores the new context in `*out_ctx` (null on failure) and returns the
 * status: `InvalidPointer` for a null or misaligned `out_ctx`, or for null `ctx`. An
 * `out_ctx` overlapping the context is `InvalidPointer` too, and then `*out_ctx` is not
 * written.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroXofAbsorber` context that
 * is not mutably accessed or freed during the call.
 * If `out_ctx` is non-null, it must be valid for writing one pointer.
 */
LustroError lustro_xof_absorber_clone(const struct LustroXofAbsorber *ctx,
                                      struct LustroXofAbsorber **out_ctx);

/**
 * Creates an XOF batch from `n` messages.
 * Messages are passed as parallel pointer/length arrays.
 * Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
 * for a null or misaligned `out_ctx`, or for a null or misaligned pointer/length array (when
 * `n > 0`) or a null message pointer with a nonzero length, `InvalidLength` for a length too large
 * to address, `OutOfMemory` if the message table, the lane buffer or the digest buffer cannot be
 * allocated.
 * Fixes the runtime settings (see `lustro_configure`) when `n > 0`.
 *
 * # Safety
 * When `n > 0`, `message_ptrs` and `message_lens` must each be valid for
 * `n` readable elements.
 * Each `message_ptrs[i]` must be valid for `message_lens[i]` readable bytes
 * when `message_lens[i] > 0`.
 * The tables and the message bytes must stay valid and unmodified by any thread until the
 * function returns; they are read on the thread pool.
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
 * Creates an XOF batch with one lane per stream, in order. Lane `i` continues
 * `streams[i]` at its next block; the streams are not changed.
 * Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
 * for a null or misaligned `out_ctx`, or for a null or misaligned `streams` array (when `n > 0`)
 * or element, `InvalidLength` for an `n` too large to address, `InvalidArgument` if a stream is
 * inside a block (part of its current block unread), `OutOfMemory` if the lane buffer cannot be
 * reserved.
 *
 * # Safety
 * When `n > 0`, `streams` must be valid for `n` readable elements, each pointing to a live
 * `LustroXof` context that is not mutably accessed or freed during the call. The same context
 * may appear more than once.
 * If `out_ctx` is non-null, it must be valid for writing one pointer
 * and must not overlap any input buffer or the memory of the contexts.
 */
LustroError lustro_xof_batch_from_streams(const struct LustroXof *const *streams,
                                          uintptr_t n,
                                          struct LustroXofBatch **out_ctx);

/**
 * Returns the number of streams, or 0 for null `ctx`.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroXofBatch` context
 * that is not mutably accessed or freed during the call.
 */
uintptr_t lustro_xof_batch_len(const struct LustroXofBatch *ctx);

/**
 * Writes the number of streams to `*out_len`. Unlike `lustro_xof_batch_len`, a null `ctx`
 * is reported (`InvalidPointer`, also for a null `out_len`) instead of reading as an empty
 * batch. Nothing is written on error.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroXofBatch` context
 * that is not mutably accessed or freed during the call.
 * If `out_len` is non-null, it must be valid for writing one `uintptr_t` and must not
 * overlap the context.
 */
LustroError lustro_xof_batch_get_len(const struct LustroXofBatch *ctx, uintptr_t *out_len);

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
 * If `ctx` is non-null, it must point to a live `LustroXofBatch` context and be exclusively
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
 * `n` is nonzero), `InvalidLength` if `n` differs from the batch length, `OutOfMemory` if the
 * identifiers or the lane buffer cannot be allocated.
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
 * `k` is nonzero), `InvalidLength` if `len() * k` overflows or `k` is too large to address,
 * `OutOfMemory` if the identifiers or the lane buffer cannot be reserved.
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
 * or a length too large to address, `OutOfMemory` if the input tables or the lane buffer cannot
 * be reserved.
 *
 * # Safety
 * `message_ptrs` and `message_lens` must each be valid for `n_messages`
 * elements when `n_messages > 0`. Each `message_ptrs[i]` must be valid for
 * `message_lens[i]` bytes if `message_lens[i] > 0`.
 * `path_hi` and `path_lo` must each be valid for `n_path` elements when
 * `n_path > 0`.
 * The tables and the message bytes must stay valid and unmodified by any thread until the
 * function returns.
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
 * for a null or misaligned `out_ctx`, or for null `ctx`, `OutOfMemory` if the lane buffer cannot
 * be reserved.
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
 * `out_len` must equal `lustro_xof_batch_snapshot_size(ctx)`, otherwise `InvalidLength`.
 * Writes straight into `out` and allocates nothing, so there is no `OutOfMemory`.
 * The snapshot holds every lane's state: whoever has it can reproduce the rest of each
 * stream and its forks. Keep it as private as the messages.
 *
 * # Safety
 * If `ctx` is non-null, it must point to a live `LustroXofBatch` context
 * that is not mutably accessed or freed during the call.
 * `out` must be valid for `out_len` writable bytes and must not overlap
 * the context.
 */
LustroError lustro_xof_batch_export_snapshot(const struct LustroXofBatch *ctx,
                                             uint8_t *out,
                                             uintptr_t out_len);

/**
 * Restores an XOF batch from `len` snapshot bytes.
 * Stores the new context in `*out_ctx` (null on failure) and returns the status: `InvalidPointer`
 * for a null or misaligned `out_ctx`, or for a null `bytes` when `len > 0`; `InvalidLength`,
 * `UnsupportedVersion`, `InvalidKind` or `InvalidReserved` for a rejected snapshot;
 * `OutOfMemory` if the lane buffer cannot be allocated.
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
