//! kz80_calc - VisiCalc-style spreadsheet for Z80
//!
//! Generates Z80 machine code for a minimal spreadsheet application
//! targeting the RetroShield platform (8KB ROM, 6KB RAM).
//!
//! Built on the retroshield-z80 framework.

pub mod codegen;

pub use codegen::SpreadsheetCodeGen;
