// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

// Used for rustc_diagnostic_item.
#![feature(rustc_attrs)]

pub mod arbitrary;
#[cfg(feature = "concrete_playback")]
mod concrete_playback;
pub mod futures;
pub mod slice;
pub mod vec;

pub use arbitrary::Arbitrary;
#[cfg(feature = "concrete_playback")]
pub use concrete_playback::concrete_playback_run;
pub use futures::block_on;

/// Creates an assumption that will be valid after this statement run. Note that the assumption
/// will only be applied for paths that follow the assumption. If the assumption doesn't hold, the
/// program will exit successfully.
///
/// # Example:
///
/// The code snippet below should never panic.
///
/// ```rust
/// let i : i32 = kani::any();
/// kani::assume(i > 10);
/// if i < 0 {
///   panic!("This will never panic");
/// }
/// ```
///
/// The following code may panic though:
///
/// ```rust
/// let i : i32 = kani::any();
/// assert!(i < 0, "This may panic and verification should fail.");
/// kani::assume(i > 10);
/// ```
#[inline(never)]
#[rustc_diagnostic_item = "KaniAssume"]
pub fn assume(_cond: bool) {
    if cfg!(feature = "concrete_playback") {
        assert!(_cond, "kani::assume should always hold");
    }
}

/// Creates an assertion of the specified condition and message.
///
/// # Example:
///
/// ```rust
/// let x: bool = kani::any();
/// let y = !x;
/// kani::assert(x || y, "ORing a boolean variable with its negation must be true")
/// ```
#[inline(never)]
#[rustc_diagnostic_item = "KaniAssert"]
pub const fn assert(_cond: bool, _msg: &'static str) {
    if cfg!(feature = "concrete_playback") {
        assert!(_cond, "{}", _msg);
    }
}

/// This creates an symbolic *valid* value of type `T`. You can assign the return value of this
/// function to a variable that you want to make symbolic.
///
/// # Example:
///
/// In the snippet below, we are verifying the behavior of the function `fn_under_verification`
/// under all possible `NonZeroU8` input values, i.e., all possible `u8` values except zero.
///
/// ```rust
/// let inputA = kani::any::<std::num::NonZeroU8>();
/// fn_under_verification(inputA);
/// ```
///
/// Note: This is a safe construct and can only be used with types that implement the `Arbitrary`
/// trait. The Arbitrary trait is used to build a symbolic value that represents all possible
/// valid values for type `T`.
#[inline(always)]
pub fn any<T: Arbitrary>() -> T {
    T::any()
}

/// This function creates a symbolic value of type `T`. This may result in an invalid value.
///
/// # Safety
///
/// This function is unsafe and it may represent invalid `T` values which can lead to many
/// undesirable undefined behaviors. Because of that, this function can only be used
/// internally when we can guarantee that the type T has no restriction regarding its bit level
/// representation.
///
/// This function is also used to find concrete values in the CBMC output trace
/// and return those concrete values in concrete playback mode.
///
/// Note that SIZE_T must be equal the size of type T in bytes.
#[inline(never)]
pub(crate) unsafe fn any_raw_internal<T, const SIZE_T: usize>() -> T {
    #[cfg(feature = "concrete_playback")]
    return concrete_playback::any_raw_internal::<T, SIZE_T>();

    #[cfg(not(feature = "concrete_playback"))]
    #[allow(unreachable_code)]
    any_raw_inner::<T>()
}

/// This low-level function returns nondet bytes of size T.
#[rustc_diagnostic_item = "KaniAnyRaw"]
#[inline(never)]
#[allow(dead_code)]
fn any_raw_inner<T>() -> T {
    unimplemented!("Kani any_raw_inner");
}

/// Function used in tests for cases where the condition is not always true.
#[inline(never)]
#[rustc_diagnostic_item = "KaniExpectFail"]
pub fn expect_fail(_cond: bool, _message: &'static str) {
    if cfg!(feature = "concrete_playback") {
        assert!(!_cond, "kani::expect_fail does not hold: {_message}");
    }
}

/// Function used to generate panic with a static message as this is the only one currently
/// supported by Kani display.
///
/// During verification this will get replaced by `assert(false)`. For concrete executions, we just
/// invoke the regular `std::panic!()` function. This function is used by our standard library
/// overrides, but not the other way around.
#[inline(never)]
#[rustc_diagnostic_item = "KaniPanic"]
#[doc(hidden)]
pub const fn panic(message: &'static str) -> ! {
    panic!("{}", message)
}

/// Kani proc macros must be in a separate crate
pub use kani_macros::*;
