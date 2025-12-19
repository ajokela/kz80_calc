//! kz80_calc - VisiCalc-style spreadsheet for Z80

use std::env;
use std::fs::File;
use std::io::Write;
use std::process;

use kz80_calc::SpreadsheetCodeGen;

fn print_help() {
    eprintln!("kz80_calc - VisiCalc-style spreadsheet for Z80");
    eprintln!();
    eprintln!("Usage: kz80_calc [options]");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  -o <file>     Output binary file (default: calc.bin)");
    eprintln!("  -h, --help    Show this help");
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  kz80_calc                    Generate calc.bin");
    eprintln!("  kz80_calc -o spreadsheet.bin Generate spreadsheet.bin");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let mut output_file = "calc.bin".to_string();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                print_help();
                process::exit(0);
            }
            "-o" => {
                if i + 1 >= args.len() {
                    eprintln!("Error: -o requires an argument");
                    process::exit(1);
                }
                output_file = args[i + 1].clone();
                i += 2;
            }
            arg => {
                eprintln!("Unknown option: {}", arg);
                print_help();
                process::exit(1);
            }
        }
    }

    // Generate the spreadsheet ROM
    let mut codegen = SpreadsheetCodeGen::new();
    codegen.generate();
    let rom = codegen.into_rom();

    // Write output file
    let mut file = File::create(&output_file).expect("Failed to create output file");
    file.write_all(&rom).expect("Failed to write output file");

    eprintln!("Generated spreadsheet binary: {}", output_file);
    eprintln!("  {} bytes", rom.len());
}
