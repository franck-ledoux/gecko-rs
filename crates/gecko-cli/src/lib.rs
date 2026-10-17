//! # gecko, on this machine
//!
//! The two pieces the binary is made of, as a library so that both the
//! binary and the tests link **the same code**. A binary's modules can only
//! be reached from a test by including the file again, which compiles a
//! second copy: the test then passes against a copy, and coverage of the
//! real one reads as nothing at all. One target, one copy, one answer.

pub mod files;
pub mod serve;
