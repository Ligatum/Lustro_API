// ==============================================================
// LUSTRO CORE V1 (SSOT) AVX2 REFERENCE IMPLEMENTATION/BACKEND
// ==============================================================

// 4 independent states per vector (one per 64-bit lane).

use std::arch::x86_64::*;

use crate::core::{PHI_64, PHI_64_ROT_17, PHI_64_ROT_23};

use crate::dispatch::StreamLane;

struct MulConst64 {
    lo32: __m256i,
    hi32: __m256i,
}

struct SimdConsts {
    phi: U64x4,
    one: U64x4,
    c64: __m256i,
    mask32: __m256i,
    mask63: __m256i,
    c127: __m256i,
    c113: __m256i,
    phi_mul: MulConst64,
    phi17_mul: MulConst64,
}

// ======================================================
// SIMD TYPES
// ======================================================

// 256-BIT VECTOR OF 4x U64 LANES
#[derive(Clone, Copy)]
#[repr(transparent)]
struct U64x4(__m256i);

// 4 parallel 128-bit states as split hi/lo 64-bit vectors: value = hi << 64 | lo.
#[derive(Clone, Copy)]
struct U128x4 {
    hi: U64x4,
    lo: U64x4,
}

// ======================================================
// SIMD CONSTANTS
// ======================================================

#[inline(always)]
unsafe fn make_consts() -> SimdConsts {
    let phi = splat(PHI_64);
    let mask32 = _mm256_set1_epi64x(0xFFFFFFFF);
    let phi_val = phi.0;
    let phi17_val = _mm256_set1_epi64x(PHI_64_ROT_17 as i64);

    SimdConsts {
        phi,
        one: splat(1),
        c64: _mm256_set1_epi64x(64),
        mask32,
        mask63: _mm256_set1_epi64x(63),
        c127: _mm256_set1_epi64x(127),
        c113: _mm256_set1_epi64x(113),
        phi_mul: MulConst64 {
            lo32: _mm256_and_si256(phi_val, mask32),
            hi32: _mm256_srli_epi64(phi_val, 32),
        },
        phi17_mul: MulConst64 {
            lo32: _mm256_and_si256(phi17_val, mask32),
            hi32: _mm256_srli_epi64(phi17_val, 32),
        },
    }
}

// ======================================================
// BASIC OPS
// ======================================================

#[inline(always)]
unsafe fn splat(x: u64) -> U64x4 {
    U64x4(_mm256_set1_epi64x(x as i64))
}

#[inline(always)]
unsafe fn add(a: U64x4, b: U64x4) -> U64x4 {
    U64x4(_mm256_add_epi64(a.0, b.0))
}

#[inline(always)]
unsafe fn xor(a: U64x4, b: U64x4) -> U64x4 {
    U64x4(_mm256_xor_si256(a.0, b.0))
}

#[inline(always)]
unsafe fn or(a: U64x4, b: U64x4) -> U64x4 {
    U64x4(_mm256_or_si256(a.0, b.0))
}

// ======================================================
// FULL 64-BIT MUL (AVX2)
// ======================================================

// FULL 64-BIT MULTIPLICATION VIA 32-BIT PARTIAL PRODUCTS (CONSTANT SECOND OPERAND)
#[inline(always)]
unsafe fn mul_u64x4_const(a: U64x4, c: &MulConst64) -> U64x4 {
    let lo = _mm256_mul_epu32(a.0, c.lo32);

    let a_hi = _mm256_srli_epi64(a.0, 32);

    let mid1 = _mm256_mul_epu32(a_hi, c.lo32);
    let mid2 = _mm256_mul_epu32(a.0, c.hi32);

    let mid = _mm256_add_epi64(mid1, mid2);
    let mid = _mm256_slli_epi64(mid, 32);

    U64x4(_mm256_add_epi64(lo, mid))
}

// x * (x | 1) MOD 2^64 (KEY SCHEDULE)
#[inline(always)]
unsafe fn mul_x_or_one(x: U64x4, consts: &SimdConsts) -> U64x4 {
    let x_hi = _mm256_srli_epi64(x.0, 32);
    let lo = _mm256_mul_epu32(x.0, x.0);
    let cross = _mm256_mul_epu32(x.0, x_hi);
    let sq = _mm256_add_epi64(lo, _mm256_slli_epi64(_mm256_add_epi64(cross, cross), 32));
    // all ones when x is even
    let even = _mm256_sub_epi64(_mm256_and_si256(x.0, consts.one.0), consts.one.0);
    U64x4(_mm256_add_epi64(sq, _mm256_and_si256(x.0, even)))
}

// ======================================================
// ROTATIONS 64-BIT
// ======================================================

// FIXED ROTATE LEFT (U64 LANES)
#[inline(always)]
unsafe fn rotl64_imm<const R: u32>(x: U64x4) -> U64x4 {
    let l = match R {
        11 => _mm256_slli_epi64(x.0, 11),
        13 => _mm256_slli_epi64(x.0, 13),
        17 => _mm256_slli_epi64(x.0, 17),
        19 => _mm256_slli_epi64(x.0, 19),
        21 => _mm256_slli_epi64(x.0, 21),
        23 => _mm256_slli_epi64(x.0, 23),
        27 => _mm256_slli_epi64(x.0, 27),
        29 => _mm256_slli_epi64(x.0, 29),
        31 => _mm256_slli_epi64(x.0, 31),
        32 => _mm256_slli_epi64(x.0, 32),
        33 => _mm256_slli_epi64(x.0, 33),
        37 => _mm256_slli_epi64(x.0, 37),
        40 => _mm256_slli_epi64(x.0, 40),
        41 => _mm256_slli_epi64(x.0, 41),
        43 => _mm256_slli_epi64(x.0, 43),
        47 => _mm256_slli_epi64(x.0, 47),
        _ => _mm256_sll_epi64(x.0, _mm_cvtsi32_si128(R as i32)),
    };

    let r = match R {
        11 => _mm256_srli_epi64(x.0, 53),
        13 => _mm256_srli_epi64(x.0, 51),
        17 => _mm256_srli_epi64(x.0, 47),
        19 => _mm256_srli_epi64(x.0, 45),
        21 => _mm256_srli_epi64(x.0, 43),
        23 => _mm256_srli_epi64(x.0, 41),
        27 => _mm256_srli_epi64(x.0, 37),
        29 => _mm256_srli_epi64(x.0, 35),
        31 => _mm256_srli_epi64(x.0, 33),
        32 => _mm256_srli_epi64(x.0, 32),
        33 => _mm256_srli_epi64(x.0, 31),
        37 => _mm256_srli_epi64(x.0, 27),
        40 => _mm256_srli_epi64(x.0, 24),
        41 => _mm256_srli_epi64(x.0, 23),
        43 => _mm256_srli_epi64(x.0, 21),
        47 => _mm256_srli_epi64(x.0, 17),
        _ => _mm256_srl_epi64(x.0, _mm_cvtsi32_si128((64 - R) as i32)),
    };

    U64x4(_mm256_or_si256(l, r))
}

// VARIABLE ROTATE LEFT - PER-LANE, BOUNDED INPUT
// PRECONDITION: every lane of `r` is in 0..=63.
// AVX2 variable shifts do not reduce shift counts modulo 64.
// lane_mix_rotate passes odd keys in 1..=63.
#[inline(always)]
unsafe fn rotl64_var_bounded(x: U64x4, r: U64x4, consts: &SimdConsts) -> U64x4 {
    U64x4(_mm256_or_si256(
        _mm256_sllv_epi64(x.0, r.0),
        _mm256_srlv_epi64(x.0, _mm256_sub_epi64(consts.c64, r.0)),
    ))
}

// ======================================================
// U128 OPS
// ======================================================

// 128-BIT ADD WITH MANUAL CARRY PROPAGATION
#[inline(always)]
unsafe fn add_u128(a: U128x4, b: U128x4, consts: &SimdConsts) -> U128x4 {
    let lo = _mm256_add_epi64(a.lo.0, b.lo.0);

    let sign = _mm256_set1_epi64x(0x8000000000000000u64 as i64);

    let a_lo_f = _mm256_xor_si256(a.lo.0, sign);
    let lo_f = _mm256_xor_si256(lo, sign);

    let carry_mask = _mm256_cmpgt_epi64(a_lo_f, lo_f);

    let carry = _mm256_and_si256(carry_mask, consts.one.0);

    let hi = _mm256_add_epi64(_mm256_add_epi64(a.hi.0, b.hi.0), carry);

    U128x4 {
        hi: U64x4(hi),
        lo: U64x4(lo),
    }
}

#[inline(always)]
unsafe fn xor_u128(a: U128x4, b: U128x4) -> U128x4 {
    U128x4 {
        hi: U64x4(_mm256_xor_si256(a.hi.0, b.hi.0)),
        lo: U64x4(_mm256_xor_si256(a.lo.0, b.lo.0)),
    }
}

// ======================================================
// ROTATIONS 128-BIT
// HARDCODED: PURE AVX2 IMM SHIFTS
// ======================================================

#[inline(always)]
unsafe fn rotl_u128_17(x: U128x4) -> U128x4 {
    let hi_l = _mm256_slli_epi64(x.hi.0, 17);
    let lo_l = _mm256_slli_epi64(x.lo.0, 17);

    let hi_r = _mm256_srli_epi64(x.lo.0, 47);
    let lo_r = _mm256_srli_epi64(x.hi.0, 47);

    U128x4 {
        hi: U64x4(_mm256_or_si256(hi_l, hi_r)),
        lo: U64x4(_mm256_or_si256(lo_l, lo_r)),
    }
}

#[inline(always)]
unsafe fn rotl_u128_37(x: U128x4) -> U128x4 {
    let hi_l = _mm256_slli_epi64(x.hi.0, 37);
    let lo_l = _mm256_slli_epi64(x.lo.0, 37);

    let hi_r = _mm256_srli_epi64(x.lo.0, 27);
    let lo_r = _mm256_srli_epi64(x.hi.0, 27);

    U128x4 {
        hi: U64x4(_mm256_or_si256(hi_l, hi_r)),
        lo: U64x4(_mm256_or_si256(lo_l, lo_r)),
    }
}

#[inline(always)]
unsafe fn rotl_u128_42(x: U128x4) -> U128x4 {
    let hi_l = _mm256_slli_epi64(x.hi.0, 42);
    let lo_l = _mm256_slli_epi64(x.lo.0, 42);

    let hi_r = _mm256_srli_epi64(x.lo.0, 22);
    let lo_r = _mm256_srli_epi64(x.hi.0, 22);

    U128x4 {
        hi: U64x4(_mm256_or_si256(hi_l, hi_r)),
        lo: U64x4(_mm256_or_si256(lo_l, lo_r)),
    }
}

#[inline(always)]
unsafe fn rotl_u128_43(x: U128x4) -> U128x4 {
    let hi_l = _mm256_slli_epi64(x.hi.0, 43);
    let lo_l = _mm256_slli_epi64(x.lo.0, 43);

    let hi_r = _mm256_srli_epi64(x.lo.0, 21);
    let lo_r = _mm256_srli_epi64(x.hi.0, 21);

    U128x4 {
        hi: U64x4(_mm256_or_si256(hi_l, hi_r)),
        lo: U64x4(_mm256_or_si256(lo_l, lo_r)),
    }
}

// VARIABLE 128-BIT ROTATION, r in 0..=127
#[inline(always)]
unsafe fn rotl_u128_var(x: U128x4, r: U64x4, consts: &SimdConsts) -> U128x4 {
    // ── GROUP 1: r-dependent (Early Kill) ──
    let r_mod = _mm256_and_si256(r.0, consts.mask63);
    let mask = _mm256_cmpgt_epi64(r.0, consts.mask63);

    // ── PATH A ──
    let tmp_hi = {
        let inv_a = _mm256_sub_epi64(consts.c64, r_mod);

        let hi_l = _mm256_sllv_epi64(x.hi.0, r_mod);
        let lo_r = _mm256_srlv_epi64(x.lo.0, inv_a);

        _mm256_or_si256(hi_l, lo_r)
    };

    // ── PATH B ──
    let tmp_lo = {
        let inv_b = _mm256_sub_epi64(consts.c64, r_mod);

        let lo_l = _mm256_sllv_epi64(x.lo.0, r_mod);
        let hi_r = _mm256_srlv_epi64(x.hi.0, inv_b);

        _mm256_or_si256(lo_l, hi_r)
    };

    // ── FINAL ──
    let final_hi = _mm256_blendv_epi8(tmp_hi, tmp_lo, mask);
    let final_lo = _mm256_blendv_epi8(tmp_lo, tmp_hi, mask);

    // r >= 64: swap
    U128x4 {
        hi: U64x4(final_hi),
        lo: U64x4(final_lo),
    }
}

// ======================================================
// MEMORY: AoS <-> SoA (VECTOR TRANSPOSE)
// ======================================================

// 4x4 TRANSPOSE: 4 lanes of [w0, w1, w2, w3] in memory -> 4 vectors (w0..w3),
// vector j holds word j of every lane.
#[inline(always)]
unsafe fn load_soa(chunk: &[u64]) -> (U64x4, U64x4, U64x4, U64x4) {
    let p = chunk.as_ptr() as *const __m256i;

    let r0 = _mm256_loadu_si256(p.add(0));
    let r1 = _mm256_loadu_si256(p.add(1));
    let r2 = _mm256_loadu_si256(p.add(2));
    let r3 = _mm256_loadu_si256(p.add(3));

    let t0 = _mm256_unpacklo_epi64(r0, r1);
    let t1 = _mm256_unpackhi_epi64(r0, r1);
    let t2 = _mm256_unpacklo_epi64(r2, r3);
    let t3 = _mm256_unpackhi_epi64(r2, r3);

    let a = _mm256_permute2x128_si256(t0, t2, 0x20);
    let b = _mm256_permute2x128_si256(t1, t3, 0x20);
    let c = _mm256_permute2x128_si256(t0, t2, 0x31);
    let d = _mm256_permute2x128_si256(t1, t3, 0x31);

    (U64x4(a), U64x4(b), U64x4(c), U64x4(d))
}

// INVERSE OF load_soa: writes 4 lanes as [w0, w1, w2, w3] each
// (16 u64 = 4 contiguous 32-byte blocks).
#[inline(always)]
unsafe fn store_soa_ptr(w0: U64x4, w1: U64x4, w2: U64x4, w3: U64x4, p: *mut u64) {
    let t0 = _mm256_unpacklo_epi64(w0.0, w1.0);
    let t1 = _mm256_unpackhi_epi64(w0.0, w1.0);
    let t2 = _mm256_unpacklo_epi64(w2.0, w3.0);
    let t3 = _mm256_unpackhi_epi64(w2.0, w3.0);

    let r0 = _mm256_permute2x128_si256(t0, t2, 0x20);
    let r1 = _mm256_permute2x128_si256(t1, t3, 0x20);
    let r2 = _mm256_permute2x128_si256(t0, t2, 0x31);
    let r3 = _mm256_permute2x128_si256(t1, t3, 0x31);

    let p = p as *mut __m256i;
    _mm256_storeu_si256(p.add(0), r0);
    _mm256_storeu_si256(p.add(1), r1);
    _mm256_storeu_si256(p.add(2), r2);
    _mm256_storeu_si256(p.add(3), r3);
}

// ======================================================
// IDM CORE
// ======================================================

// SIMD IMPLEMENTATION OF IDM MIXING CORE
#[inline(always)]
unsafe fn idm_core(
    mut a: U64x4,
    mut b: U64x4,
    mut c: U64x4,
    mut d: U64x4,
    consts: &SimdConsts,
) -> (U64x4, U64x4, U64x4, U64x4) {
    a = add(a, b);
    c = add(c, d);
    b = xor(b, rotl64_imm::<11>(a));
    d = xor(d, rotl64_imm::<19>(c));

    a = add(a, consts.phi);
    let phi23 = splat(PHI_64_ROT_23);
    c = add(c, phi23);
    a = xor(a, rotl64_imm::<37>(c));
    d = xor(d, rotl64_imm::<43>(b));

    b = add(b, a);
    d = add(d, c);
    a = xor(a, rotl64_imm::<31>(b));
    c = xor(c, rotl64_imm::<47>(d));

    let ac = xor(a, c);

    let m = mul_u64x4_const(ac, &consts.phi_mul);

    let mix0 = ac;
    let mix0_rot = rotl64_imm::<32>(mix0);
    let mix0_x = xor(mix0, mix0_rot);

    b = xor(b, m);
    d = xor(d, rotl64_imm::<33>(m));

    let m0 = mul_u64x4_const(mix0_x, &consts.phi_mul);

    let bd = xor(b, d);
    let m1 = mul_u64x4_const(bd, &consts.phi17_mul);

    a = xor(a, m0);
    c = xor(c, rotl64_imm::<23>(m0));
    b = xor(b, m1);
    d = xor(d, rotl64_imm::<41>(m1));

    a = add(a, b);
    d = xor(d, rotl64_imm::<33>(a));
    b = xor(b, rotl64_imm::<21>(xor(c, d)));

    a = add(a, d);
    c = add(c, b);
    d = xor(d, rotl64_imm::<41>(a));
    b = xor(b, rotl64_imm::<23>(c));

    b = add(b, a);
    d = add(d, c);
    a = xor(a, rotl64_imm::<17>(b));
    c = xor(c, rotl64_imm::<31>(d));

    (a, b, c, d)
}

// ======================================================
// LANE PREPARE
// ======================================================

#[inline(always)]
unsafe fn lane_prepare(s0_xor: U128x4, s1_xor: U128x4, consts: &SimdConsts) -> (U64x4, U64x4) {
    // Build the first working stream.
    let v0 = {
        let t = add_u128(s0_xor, s1_xor, consts);
        let swapped = U128x4 { hi: t.lo, lo: t.hi };
        let rotated = rotl_u128_37(t);
        let mix = xor_u128(swapped, rotated);

        xor_u128(t, mix).lo
    };

    // Build the second working stream.
    let v1 = {
        let rotated_s1 = rotl_u128_42(s1_xor);
        let t = xor_u128(s0_xor, rotated_s1);
        let swapped = U128x4 { hi: t.lo, lo: t.hi };
        let rotated = rotl_u128_43(t);
        let mix = xor_u128(swapped, rotated);

        xor_u128(t, mix).lo
    };

    let mut v0 = v0;
    let mut v1 = v1;

    // Stage 1
    v0 = add(v0, rotl64_imm::<31>(v1));
    v1 = xor(v1, rotl64_imm::<27>(v0));

    // Stage 2
    v0 = add(v0, rotl64_imm::<17>(v1));
    v1 = xor(v1, rotl64_imm::<29>(v0));

    (v0, v1)
}

// ======================================================
// LANE KEY SCHEDULE
// ======================================================
// Derives per-lane rotation keys from the prepared working streams.

#[inline(always)]
unsafe fn lane_key_schedule(v0: U64x4, v1: U64x4, consts: &SimdConsts) -> (U64x4, U64x4) {
    let rot0 = rotl64_imm::<11>(v1);
    let x0 = xor(v0, rot0);
    let x0_folded = xor(x0, U64x4(_mm256_srli_epi64(x0.0, 32)));

    let rot1 = rotl64_imm::<13>(v0);
    let x1 = xor(v1, rot1);
    let x1_folded = xor(x1, U64x4(_mm256_srli_epi64(x1.0, 32)));

    let k0 = {
        let mul_res = mul_x_or_one(x0_folded, consts);

        or(U64x4(_mm256_srli_epi64(mul_res.0, 58)), consts.one)
    };

    let k1 = {
        let mul_res = mul_x_or_one(x1_folded, consts);

        or(U64x4(_mm256_srli_epi64(mul_res.0, 58)), consts.one)
    };

    (k0, k1)
}

// ======================================================
// LANE MIX ROTATE
// ======================================================
// Applies variable-rotation mixing using the
// rotation keys produced by lane_key_schedule().
//
// The first stream updates v0. The updated v0 is then used to update v1.

#[inline(always)]
unsafe fn lane_mix_rotate(
    v0: U64x4,
    v1: U64x4,
    k0: U64x4,
    k1: U64x4,
    consts: &SimdConsts,
) -> (U64x4, U64x4) {
    let v0 = {
        let rot = rotl64_var_bounded(v1, k0, consts);
        let shr = U64x4(_mm256_srli_epi64(v1.0, 7));
        add(v0, xor(rot, shr))
    };

    let v1 = {
        let rot = rotl64_var_bounded(v0, k1, consts);
        let shr = U64x4(_mm256_srli_epi64(v0.0, 7));
        xor(v1, xor(rot, shr))
    };

    (v0, v1)
}

// ======================================================
// LANE FINALIZE AND PROJECT
// ======================================================
// Finalizes the working streams and converts them into bounded rotation values.

#[inline(always)]
unsafe fn lane_finalize_and_project(v0: U64x4, v1: U64x4, consts: &SimdConsts) -> (U64x4, U64x4) {
    let v0_step1 = add(v0, rotl64_imm::<17>(v1));
    let v1_final = xor(v1, rotl64_imm::<40>(v0_step1));
    let v0_final = xor(v0_step1, U64x4(_mm256_srli_epi64(v0_step1.0, 29)));

    // Projection 0: Derive a rotation value from finalized v0.
    let rot0 = {
        let v = v0_final;
        let lo = _mm256_and_si256(v.0, consts.mask32);
        let hi = _mm256_srli_epi64(v.0, 32);
        let plo = _mm256_mul_epu32(lo, consts.c127);
        let phi = _mm256_mul_epu32(hi, consts.c127);
        let carry = _mm256_srli_epi64(plo, 32);
        let acc = _mm256_add_epi64(phi, carry);
        add(U64x4(_mm256_srli_epi64(acc, 32)), consts.one)
    };

    // Projection 1: Derive a rotation value from finalized v1.
    let rot1 = {
        let v = v1_final;
        let lo = _mm256_and_si256(v.0, consts.mask32);
        let hi = _mm256_srli_epi64(v.0, 32);
        let plo = _mm256_mul_epu32(lo, consts.c113);
        let phi = _mm256_mul_epu32(hi, consts.c113);
        let carry = _mm256_srli_epi64(plo, 32);
        let acc = _mm256_add_epi64(phi, carry);
        add(U64x4(_mm256_srli_epi64(acc, 32)), consts.one)
    };

    (rot0, rot1)
}

// ======================================================
// LANE APPLY ROTATION
// ======================================================
// Applies the final variable 128-bit rotation to the state.

#[inline(always)]
unsafe fn lane_apply_rotation(s_lane: U128x4, rot: U64x4, consts: &SimdConsts) -> U128x4 {
    rotl_u128_var(s_lane, rot, consts)
}

// ======================================================
// LANE PROCESSING
// ======================================================
// Executes one complete lane update.

#[inline(always)]
unsafe fn process_lane(
    s0_lane: &mut U128x4,
    s1_lane: &mut U128x4,
    current_rc: U64x4,
    consts: &SimdConsts,
) {
    // 1. In-place state perturbation (rc / rc_s1 short lifetime)
    {
        let rc = U128x4 {
            hi: rotl64_imm::<17>(current_rc),
            lo: current_rc,
        };
        let rc_s1 = rotl_u128_17(rc);

        *s0_lane = xor_u128(*s0_lane, rc);
        *s1_lane = xor_u128(*s1_lane, rc_s1);
    }

    // 2. State preparation
    let (v0, v1) = lane_prepare(*s0_lane, *s1_lane, consts);

    // 3. Rotation-key generation
    let (k0, k1) = lane_key_schedule(v0, v1, consts);

    // 4. Variable-rotation mixing
    let (v0, v1) = lane_mix_rotate(v0, v1, k0, k1, consts);

    // 5. Finalization and projection
    let (rot0, rot1) = lane_finalize_and_project(v0, v1, consts);

    // 6. State update through 128-bit rotation directly on perturbed state
    *s0_lane = lane_apply_rotation(*s0_lane, rot0, consts);
    *s1_lane = lane_apply_rotation(*s1_lane, rot1, consts);
}

// ======================================================
// STREAM STEP X4
// ======================================================

// LOAD 4 StreamLane -> SoA (s0, s1, step)
#[inline(always)]
unsafe fn load_lanes_x4(lanes: &[StreamLane]) -> (U128x4, U128x4, U64x4) {
    let mut words = [0u64; 16];
    let mut steps = [0u64; 4];

    for (i, l) in lanes[..4].iter().enumerate() {
        words[4 * i] = l.s0 as u64;
        words[4 * i + 1] = (l.s0 >> 64) as u64;
        words[4 * i + 2] = l.s1 as u64;
        words[4 * i + 3] = (l.s1 >> 64) as u64;
        steps[i] = l.step;
    }

    let (s0_lo, s0_hi, s1_lo, s1_hi) = load_soa(&words);
    let step = U64x4(_mm256_loadu_si256(steps.as_ptr() as *const __m256i));

    (
        U128x4 {
            hi: s0_hi,
            lo: s0_lo,
        },
        U128x4 {
            hi: s1_hi,
            lo: s1_lo,
        },
        step,
    )
}

// STORE SoA (s0, s1, step) -> 4 StreamLane
#[inline(always)]
unsafe fn store_lanes_x4(s0: U128x4, s1: U128x4, step: U64x4, lanes: &mut [StreamLane]) {
    let mut words = [0u64; 16];
    let mut steps = [0u64; 4];

    store_soa_ptr(s0.lo, s0.hi, s1.lo, s1.hi, words.as_mut_ptr());
    _mm256_storeu_si256(steps.as_mut_ptr() as *mut __m256i, step.0);

    for (i, l) in lanes[..4].iter_mut().enumerate() {
        l.s0 = (words[4 * i] as u128) | ((words[4 * i + 1] as u128) << 64);
        l.s1 = (words[4 * i + 2] as u128) | ((words[4 * i + 3] as u128) << 64);
        l.step = steps[i];
    }
}

// ONE STEP: IDM + ERD, RC PASSED IN
#[inline(always)]
unsafe fn step_x4_rc(s0: &mut U128x4, s1: &mut U128x4, rc: U64x4, consts: &SimdConsts) {
    let (a, b, c, d) = idm_core(s0.hi, s0.lo, s1.hi, s1.lo, consts);
    *s0 = U128x4 { hi: a, lo: b };
    *s1 = U128x4 { hi: c, lo: d };

    process_lane(s0, s1, rc, consts);
}

// ADVANCES `lanes` BY `steps`, 4 LANES PER GROUP, STATE KEPT IN REGISTERS
// PRECONDITION: CPU supports AVX2 (caller checks at runtime).
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn process_lanes_x4(
    lanes: &mut [StreamLane],
    out: *mut [u8; 32],
    n: usize,
    first: usize,
    steps: usize,
) {
    assert_eq!(
        lanes.len() % 4,
        0,
        "process_lanes_x4: lanes.len() must be a multiple of 4"
    );
    debug_assert!(first + lanes.len() <= n);

    let consts = make_consts();
    let steps_v = splat(steps as u64);

    for (g, group) in lanes.chunks_exact_mut(4).enumerate() {
        let (mut s0, mut s1, step) = load_lanes_x4(group);
        let mut rc = mul_u64x4_const(add(step, splat(2)), &consts.phi_mul);
        let base = first + 4 * g;

        for s in 0..steps {
            step_x4_rc(&mut s0, &mut s1, rc, &consts);
            // 4 consecutive blocks, AoS [s0_lo, s0_hi, s1_lo, s1_hi] (LE)
            let p = out.add(s * n + base) as *mut u64;
            store_soa_ptr(s0.lo, s0.hi, s1.lo, s1.hi, p);
            rc = add(rc, consts.phi);
        }

        store_lanes_x4(s0, s1, add(step, steps_v), group);
    }
}
