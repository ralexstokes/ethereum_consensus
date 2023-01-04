#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
extern crate alloc;
#[cfg(not(feature = "std"))]
extern crate core;

pub mod altair;
pub mod bellatrix;
pub mod builder;
pub(crate) mod bytes;
#[cfg(feature = "std")]
pub mod clock;
pub mod configs;
pub mod crypto;
pub mod domains;
#[cfg(feature = "std")]
pub mod networking;
pub mod phase0;
pub mod prelude;
pub mod primitives;
#[cfg(feature = "serde")]
pub mod serde;
pub mod signing;
pub mod ssz;
pub mod state_transition;
