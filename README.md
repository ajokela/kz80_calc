# kz80_calc

A VisiCalc-style spreadsheet for the Z80 processor, targeting the RetroShield platform.

## Features

- 16×64 cell grid (columns A-P, rows 1-64)
- 16-bit integer values
- Formula support: `=A1+B2`, `=C3*5`, `=SUM(A1:A10)`
- Arrow key navigation
- Automatic recalculation
- Fits in 8KB ROM / 6KB RAM

## Building

```bash
cargo build --release
./target/release/kz80_calc -o spreadsheet.bin
```

## Usage

Run with the RetroShield emulator:

```bash
retroshield spreadsheet.bin
```

### Keys

- Arrow keys: Navigate cells
- Enter: Edit cell
- Escape: Cancel edit
- Letters/numbers: Enter values or formulas
- `=`: Start formula entry

### Formulas

```
=A1+B2      Addition
=A1-B2      Subtraction
=A1*B2      Multiplication
=A1/B2      Division
=SUM(A1:A5) Sum of range
```

## Memory Layout

```
ROM (8KB):
  0x0000-0x00FF  Startup, vectors
  0x0100-0x1FFF  Spreadsheet engine

RAM (6KB):
  0x2000-0x2FFF  Cell data (4KB = 1024 cells)
  0x3000-0x35FF  Formula buffer, parse state
  0x3600-0x37FF  Display buffer, stack
```

## Inspiration

Inspired by VisiCalc (1979), the original "killer app" that launched
the personal computer revolution.

## License

BSD 3-Clause License
