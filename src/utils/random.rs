//! Fast 16-bit pseudo random number generator.

// Based on MIT-licensed code (c) 2012 by Olivier Gillet (ol.gillet@gmail.com)

//! ## Where the state lives
//!
//! Upstream keeps it in one process-global `AtomicU32`. Every engine that needs noise draws from
//! it, so two threads rendering at the same time overwrite each other's seed between the store
//! and the draw: an offline export running while an audio callback plays, or two plugin
//! instances in one host, and neither render is reproducible.
//!
//! With the `std` feature (on by default) the state is thread-local instead, so each thread has
//! its own stream and cannot be disturbed by another. The generator is unchanged — same LCG,
//! same constants, same initial state — so a single-threaded caller gets exactly the numbers it
//! got before, in the same order.
//!
//! Thread-local is not on its own enough to make a render a pure function of position: one audio
//! thread renders many voices, and which voice draws when depends on the pool. A caller that
//! needs that guarantee reseeds at a known boundary, which is what Grout's wrapper does before
//! every 24-sample chunk. Thread-local is what makes *that* scheme hold across threads.
//!
//! Without `std` the old global remains, because there is nowhere else to put it. That build
//! keeps upstream's behaviour and upstream's hazard.

#[cfg(feature = "std")]
use core::cell::Cell;
#[cfg(not(feature = "std"))]
use core::sync::atomic::{AtomicU32, Ordering};

/// Upstream's initial state.
pub const DEFAULT_SEED: u32 = 0x21;

#[cfg(feature = "std")]
std::thread_local! {
    /// `const` initialiser and a type that is not `Drop`: no lazy-init branch and no destructor
    /// registration, so the first touch from a fresh audio thread allocates nothing and takes
    /// no lock. That matters — this is read on the audio thread.
    static RNG_STATE: Cell<u32> = const { Cell::new(DEFAULT_SEED) };
}

#[cfg(not(feature = "std"))]
static RNG_STATE: AtomicU32 = AtomicU32::new(DEFAULT_SEED);

#[cfg(not(feature = "std"))]
#[inline]
fn state() -> u32 {
    RNG_STATE.load(Ordering::Relaxed)
}

#[inline]
pub fn seed(seed: u32) {
    #[cfg(feature = "std")]
    RNG_STATE.with(|s| s.set(seed));
    #[cfg(not(feature = "std"))]
    RNG_STATE.store(seed, Ordering::Relaxed);
}

#[inline]
pub fn get_word() -> u32 {
    #[cfg(feature = "std")]
    {
        RNG_STATE.with(|s| {
            let next = s.get().wrapping_mul(1664525).wrapping_add(1013904223);
            s.set(next);
            next
        })
    }
    #[cfg(not(feature = "std"))]
    {
        RNG_STATE.store(
            RNG_STATE
                .load(Ordering::Relaxed)
                .wrapping_mul(1664525)
                .wrapping_add(1013904223),
            Ordering::Relaxed,
        );
        state()
    }
}

#[inline]
pub fn get_sample() -> i16 {
    (get_word() >> 16) as i16
}

#[inline]
pub fn get_float() -> f32 {
    get_word() as f32 / 4294967296.0
}

/// True when the state is per thread. Lets a caller assert the guarantee it is relying on
/// rather than assume the feature survived someone's `default-features = false`.
pub const fn is_thread_local() -> bool {
    cfg!(feature = "std")
}
