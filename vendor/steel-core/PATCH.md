# steel-core 0.8.3, patched

The crates.io release of `steel-core` 0.8.3 (MIT OR Apache-2.0,
<https://github.com/mattwparas/steel>), without its tests and benches, with
these changes, all for Block Schemer's console, whose input port waits for
lines entered while a program runs:

- `SteelVal::new_dyn_reader_port` in `src/rvals.rs`, beside
  `new_dyn_writer_port`: an input port reading from any `Read`. Upstream
  has no public way to make a `DynReader` port outside its `dylibs` FFI.
- `Peekable::peek_char` in `src/values/port.rs` reads a byte at a time
  until it holds a whole character. It read four bytes ahead, which on an
  interactive reader waits for input beyond the character peeked.
- Reading a line, or the rest of a port, starts with the bytes a peek took
  out of the reader, which were lost; a peeked newline ends the line
  without reading on.
- `#![allow(warnings)]` in `src/lib.rs`, since a path dependency's warnings
  are not capped as a registry one's are.

The commit after the one adding the untouched copy shows them all. Once
upstream has them, delete this directory and the `[patch.crates-io]` entry
in the workspace's `Cargo.toml`.
