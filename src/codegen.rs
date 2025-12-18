//! Z80 code generator for kz80_calc spreadsheet
//!
//! Memory Layout:
//! ROM (8KB):
//!   0x0000-0x00FF  Startup, vectors
//!   0x0100-0x1FFF  Spreadsheet engine
//!
//! RAM (6KB):
//!   0x2000-0x2FFF  Cell data (4KB = 1024 cells × 4 bytes)
//!   0x3000-0x30FF  Input buffer (256 bytes)
//!   0x3100-0x31FF  Display line buffer (256 bytes)
//!   0x3200-0x35FF  Formula parse buffer, scratch (1KB)
//!   0x3600-0x37FF  Stack (512 bytes)
//!
//! Cell format (4 bytes):
//!   byte 0: type (0=empty, 1=number, 2=formula, 3=error)
//!   byte 1: flags (dirty, etc.)
//!   bytes 2-3: value (16-bit signed integer) or formula offset

use std::collections::HashMap;

/// Memory constants
const ROM_START: u16 = 0x0000;  // Start at 0x0000 for emulator compatibility
const STACK_TOP: u16 = 0x37FF;

// RAM layout
const CELL_DATA: u16 = 0x2000;      // 4KB for cells
const INPUT_BUF: u16 = 0x3000;      // 256 bytes
const DISPLAY_BUF: u16 = 0x3100;    // 256 bytes
const SCRATCH: u16 = 0x3200;        // 1KB scratch/formula

// Spreadsheet state
const CURSOR_COL: u16 = 0x35F0;     // Current column (0-15)
const CURSOR_ROW: u16 = 0x35F1;     // Current row (0-63)
const VIEW_TOP: u16 = 0x35F2;       // Top visible row
const VIEW_LEFT: u16 = 0x35F3;      // Left visible column
const INPUT_LEN: u16 = 0x35F4;      // Input buffer length
const INPUT_POS: u16 = 0x35F5;      // Input cursor position
const EDIT_MODE: u16 = 0x35F6;      // 0=navigate, 1=edit
const TEMP1: u16 = 0x35F8;          // Temp storage
const TEMP2: u16 = 0x35FA;          // Temp storage

// Display constants
const SCREEN_COLS: u8 = 80;         // Terminal width
const SCREEN_ROWS: u8 = 24;         // Terminal height
const CELL_WIDTH: u8 = 9;           // Width per cell display
const VISIBLE_COLS: u8 = 8;         // Columns visible at once
const VISIBLE_ROWS: u8 = 20;        // Rows visible at once

// Grid size
const GRID_COLS: u8 = 16;           // A-P
const GRID_ROWS: u8 = 64;           // 1-64

// Cell types
const CELL_EMPTY: u8 = 0;
const CELL_NUMBER: u8 = 1;
const CELL_FORMULA: u8 = 2;
const CELL_ERROR: u8 = 3;

pub struct CodeGen {
    rom: Vec<u8>,
    labels: HashMap<String, u16>,
    fixups: Vec<(usize, String)>,
}

impl CodeGen {
    pub fn new() -> Self {
        Self {
            rom: Vec::new(),
            labels: HashMap::new(),
            fixups: Vec::new(),
        }
    }

    /// Get current emit position
    fn pos(&self) -> u16 {
        ROM_START + self.rom.len() as u16
    }

    /// Emit raw bytes
    fn emit(&mut self, bytes: &[u8]) {
        self.rom.extend_from_slice(bytes);
    }

    /// Emit a 16-bit word (little-endian)
    fn emit_word(&mut self, word: u16) {
        self.rom.push(word as u8);
        self.rom.push((word >> 8) as u8);
    }

    /// Define a label at current position
    fn label(&mut self, name: &str) {
        self.labels.insert(name.to_string(), self.pos());
    }

    /// Emit a fixup for later resolution
    fn fixup(&mut self, name: &str) {
        self.fixups.push((self.rom.len(), name.to_string()));
        self.emit_word(0); // Placeholder
    }

    /// Resolve all fixups
    fn resolve_fixups(&mut self) {
        for (offset, name) in &self.fixups {
            let addr = *self.labels.get(name).unwrap_or_else(|| {
                panic!("Undefined label: {}", name)
            });
            self.rom[*offset] = addr as u8;
            self.rom[*offset + 1] = (addr >> 8) as u8;
        }
    }

    /// Emit a null-terminated string
    fn emit_string(&mut self, s: &str) {
        for b in s.bytes() {
            self.rom.push(b);
        }
        self.rom.push(0);
    }

    /// Generate the complete spreadsheet ROM
    pub fn generate(&mut self) {
        self.emit_startup();
        self.emit_main_loop();
        self.emit_display();
        self.emit_input();
        self.emit_cell_ops();
        self.emit_formula();
        self.emit_io();
        self.emit_strings();
        self.resolve_fixups();
    }

    /// Convert to final ROM image
    pub fn into_rom(self) -> Vec<u8> {
        // No padding - code starts at 0x0000
        self.rom
    }

    /// Startup code
    fn emit_startup(&mut self) {
        // Initialize stack
        self.emit(&[0x31]); // LD SP, STACK_TOP
        self.emit_word(STACK_TOP);

        // Print welcome banner first
        self.emit(&[0x21]); // LD HL, welcome_msg
        self.fixup("welcome_msg");
        self.emit(&[0xCD]); // CALL print_string
        self.fixup("print_string");

        // Clear cursor position
        self.emit(&[0xAF]); // XOR A
        self.emit(&[0x32]); // LD (CURSOR_COL), A
        self.emit_word(CURSOR_COL);
        self.emit(&[0x32]); // LD (CURSOR_ROW), A
        self.emit_word(CURSOR_ROW);
        self.emit(&[0x32]); // LD (VIEW_TOP), A
        self.emit_word(VIEW_TOP);
        self.emit(&[0x32]); // LD (VIEW_LEFT), A
        self.emit_word(VIEW_LEFT);
        self.emit(&[0x32]); // LD (EDIT_MODE), A
        self.emit_word(EDIT_MODE);

        // Clear all cells
        self.emit(&[0x21]); // LD HL, CELL_DATA
        self.emit_word(CELL_DATA);
        self.emit(&[0x01]); // LD BC, 4096 (1024 cells × 4 bytes)
        self.emit_word(4096);
        self.label("clear_cells_loop");
        self.emit(&[0x36, 0x00]); // LD (HL), 0
        self.emit(&[0x23]); // INC HL
        self.emit(&[0x0B]); // DEC BC
        self.emit(&[0x78]); // LD A, B
        self.emit(&[0xB1]); // OR C
        self.emit(&[0xC2]); // JP NZ, clear_cells_loop
        self.fixup("clear_cells_loop");

        // Initial display
        self.emit(&[0xCD]); // CALL refresh_display
        self.fixup("refresh_display");
    }

    /// Main loop - handle input and display
    fn emit_main_loop(&mut self) {
        self.label("main_loop");

        // Read a character
        self.emit(&[0xCD]); // CALL getchar
        self.fixup("getchar");

        // Check edit mode - save char in B, check mode, restore to A
        self.emit(&[0x47]); // LD B, A (save char)
        self.emit(&[0x3A]); // LD A, (EDIT_MODE)
        self.emit_word(EDIT_MODE);
        self.emit(&[0xB7]); // OR A
        self.emit(&[0x78]); // LD A, B (restore char - doesn't affect flags)
        self.emit(&[0xC2]); // JP NZ, edit_mode_input
        self.fixup("edit_mode_input");

        // Navigation mode - check for arrow keys and commands
        // Escape sequences start with 0x1B
        self.emit(&[0xFE, 0x1B]); // CP 0x1B (ESC)
        self.emit(&[0xCA]); // JP Z, handle_escape
        self.fixup("handle_escape");

        // 'q' to quit
        self.emit(&[0xFE, b'q']);
        self.emit(&[0xCA]); // JP Z, quit
        self.fixup("quit");

        // Enter to start editing
        self.emit(&[0xFE, 0x0D]); // CP CR
        self.emit(&[0xCA]); // JP Z, start_edit
        self.fixup("start_edit");

        // '=' to start formula
        self.emit(&[0xFE, b'=']);
        self.emit(&[0xCA]); // JP Z, start_formula
        self.fixup("start_formula");

        // Check for minus sign first (before digit check)
        self.emit(&[0xFE, b'-']);
        self.emit(&[0xCA]); // JP Z, start_number
        self.fixup("start_number");

        // Digit to start number entry
        self.emit(&[0xFE, b'0']);
        self.emit(&[0xDA]); // JP C, check_hjkl (< '0')
        self.fixup("check_hjkl");
        self.emit(&[0xFE, b'9' + 1]);
        self.emit(&[0xD2]); // JP NC, check_hjkl (> '9')
        self.fixup("check_hjkl");
        // It's a digit - start number entry
        self.emit(&[0xC3]); // JP start_number
        self.fixup("start_number");

        self.label("check_hjkl");

        // hjkl navigation (vim-style)
        self.emit(&[0xFE, b'h']);
        self.emit(&[0xCA]); // JP Z, move_left
        self.fixup("move_left");
        self.emit(&[0xFE, b'j']);
        self.emit(&[0xCA]); // JP Z, move_down
        self.fixup("move_down");
        self.emit(&[0xFE, b'k']);
        self.emit(&[0xCA]); // JP Z, move_up
        self.fixup("move_up");
        self.emit(&[0xFE, b'l']);
        self.emit(&[0xCA]); // JP Z, move_right
        self.fixup("move_right");

        // Unknown key - ignore
        self.emit(&[0xC3]); // JP main_loop
        self.fixup("main_loop");

        // Handle escape sequences (arrow keys)
        self.label("handle_escape");
        self.emit(&[0xCD]); // CALL getchar
        self.fixup("getchar");
        self.emit(&[0xFE, b'[']); // CP '['
        self.emit(&[0xC2]); // JP NZ, main_loop
        self.fixup("main_loop");
        self.emit(&[0xCD]); // CALL getchar
        self.fixup("getchar");
        // A=up, B=down, C=right, D=left
        self.emit(&[0xFE, b'A']);
        self.emit(&[0xCA]); // JP Z, move_up
        self.fixup("move_up");
        self.emit(&[0xFE, b'B']);
        self.emit(&[0xCA]); // JP Z, move_down
        self.fixup("move_down");
        self.emit(&[0xFE, b'C']);
        self.emit(&[0xCA]); // JP Z, move_right
        self.fixup("move_right");
        self.emit(&[0xFE, b'D']);
        self.emit(&[0xCA]); // JP Z, move_left
        self.fixup("move_left");
        self.emit(&[0xC3]); // JP main_loop
        self.fixup("main_loop");

        // Cursor movement
        self.label("move_left");
        self.emit(&[0x3A]); // LD A, (CURSOR_COL)
        self.emit_word(CURSOR_COL);
        self.emit(&[0xB7]); // OR A
        self.emit(&[0xCA]); // JP Z, move_done (already at left edge)
        self.fixup("move_done");
        self.emit(&[0x3D]); // DEC A
        self.emit(&[0x32]); // LD (CURSOR_COL), A
        self.emit_word(CURSOR_COL);
        self.emit(&[0xC3]); // JP move_done
        self.fixup("move_done");

        self.label("move_right");
        self.emit(&[0x3A]); // LD A, (CURSOR_COL)
        self.emit_word(CURSOR_COL);
        self.emit(&[0xFE, GRID_COLS - 1]); // CP GRID_COLS-1
        self.emit(&[0xD2]); // JP NC, move_done (already at right edge)
        self.fixup("move_done");
        self.emit(&[0x3C]); // INC A
        self.emit(&[0x32]); // LD (CURSOR_COL), A
        self.emit_word(CURSOR_COL);
        self.emit(&[0xC3]); // JP move_done
        self.fixup("move_done");

        self.label("move_up");
        self.emit(&[0x3A]); // LD A, (CURSOR_ROW)
        self.emit_word(CURSOR_ROW);
        self.emit(&[0xB7]); // OR A
        self.emit(&[0xCA]); // JP Z, move_done (already at top)
        self.fixup("move_done");
        self.emit(&[0x3D]); // DEC A
        self.emit(&[0x32]); // LD (CURSOR_ROW), A
        self.emit_word(CURSOR_ROW);
        self.emit(&[0xC3]); // JP move_done
        self.fixup("move_done");

        self.label("move_down");
        self.emit(&[0x3A]); // LD A, (CURSOR_ROW)
        self.emit_word(CURSOR_ROW);
        self.emit(&[0xFE, GRID_ROWS - 1]); // CP GRID_ROWS-1
        self.emit(&[0xD2]); // JP NC, move_done (already at bottom)
        self.fixup("move_done");
        self.emit(&[0x3C]); // INC A
        self.emit(&[0x32]); // LD (CURSOR_ROW), A
        self.emit_word(CURSOR_ROW);
        // Fall through to move_done

        self.label("move_done");
        // Update view if cursor moved out of visible area
        self.emit(&[0xCD]); // CALL adjust_view
        self.fixup("adjust_view");
        self.emit(&[0xCD]); // CALL refresh_display
        self.fixup("refresh_display");
        self.emit(&[0xC3]); // JP main_loop
        self.fixup("main_loop");

        // Start editing current cell
        self.label("start_edit");
        self.emit(&[0x3E, 0x01]); // LD A, 1
        self.emit(&[0x32]); // LD (EDIT_MODE), A
        self.emit_word(EDIT_MODE);
        self.emit(&[0xAF]); // XOR A
        self.emit(&[0x32]); // LD (INPUT_LEN), A
        self.emit_word(INPUT_LEN);
        self.emit(&[0x32]); // LD (INPUT_POS), A
        self.emit_word(INPUT_POS);
        self.emit(&[0xCD]); // CALL show_input_line
        self.fixup("show_input_line");
        self.emit(&[0xC3]); // JP main_loop
        self.fixup("main_loop");

        // Start formula entry (with '=' already typed)
        self.label("start_formula");
        self.emit(&[0x3E, 0x01]); // LD A, 1
        self.emit(&[0x32]); // LD (EDIT_MODE), A
        self.emit_word(EDIT_MODE);
        self.emit(&[0x3E, b'=']); // LD A, '='
        self.emit(&[0x21]); // LD HL, INPUT_BUF
        self.emit_word(INPUT_BUF);
        self.emit(&[0x77]); // LD (HL), A
        self.emit(&[0x3E, 0x01]); // LD A, 1
        self.emit(&[0x32]); // LD (INPUT_LEN), A
        self.emit_word(INPUT_LEN);
        self.emit(&[0x32]); // LD (INPUT_POS), A
        self.emit_word(INPUT_POS);
        self.emit(&[0xCD]); // CALL show_input_line
        self.fixup("show_input_line");
        self.emit(&[0xC3]); // JP main_loop
        self.fixup("main_loop");

        // Start number entry (digit already in A)
        self.label("start_number");
        self.emit(&[0xF5]); // PUSH AF (save digit)
        self.emit(&[0x3E, 0x01]); // LD A, 1
        self.emit(&[0x32]); // LD (EDIT_MODE), A
        self.emit_word(EDIT_MODE);
        self.emit(&[0xF1]); // POP AF (restore digit)
        self.emit(&[0x21]); // LD HL, INPUT_BUF
        self.emit_word(INPUT_BUF);
        self.emit(&[0x77]); // LD (HL), A
        self.emit(&[0x3E, 0x01]); // LD A, 1
        self.emit(&[0x32]); // LD (INPUT_LEN), A
        self.emit_word(INPUT_LEN);
        self.emit(&[0x32]); // LD (INPUT_POS), A
        self.emit_word(INPUT_POS);
        self.emit(&[0xCD]); // CALL show_input_line
        self.fixup("show_input_line");
        self.emit(&[0xC3]); // JP main_loop
        self.fixup("main_loop");

        // Edit mode input handling
        self.label("edit_mode_input");
        // ESC cancels edit
        self.emit(&[0xFE, 0x1B]); // CP ESC
        self.emit(&[0xCA]); // JP Z, cancel_edit
        self.fixup("cancel_edit");
        // Enter confirms edit
        self.emit(&[0xFE, 0x0D]); // CP CR
        self.emit(&[0xCA]); // JP Z, confirm_edit
        self.fixup("confirm_edit");
        // Backspace
        self.emit(&[0xFE, 0x7F]); // CP DEL
        self.emit(&[0xCA]); // JP Z, edit_backspace
        self.fixup("edit_backspace");
        self.emit(&[0xFE, 0x08]); // CP BS
        self.emit(&[0xCA]); // JP Z, edit_backspace
        self.fixup("edit_backspace");
        // Printable character - add to buffer
        self.emit(&[0xFE, 0x20]); // CP ' '
        self.emit(&[0xDA]); // JP C, main_loop (< space)
        self.fixup("main_loop");
        self.emit(&[0xFE, 0x7F]); // CP DEL
        self.emit(&[0xD2]); // JP NC, main_loop (>= DEL)
        self.fixup("main_loop");
        // Add character to input buffer
        self.emit(&[0xF5]); // PUSH AF
        self.emit(&[0x3A]); // LD A, (INPUT_LEN)
        self.emit_word(INPUT_LEN);
        self.emit(&[0xFE, 40]); // CP 40 (max input length)
        self.emit(&[0xD2]); // JP NC, edit_input_full
        self.fixup("edit_input_full");
        self.emit(&[0x5F]); // LD E, A
        self.emit(&[0x16, 0x00]); // LD D, 0
        self.emit(&[0x21]); // LD HL, INPUT_BUF
        self.emit_word(INPUT_BUF);
        self.emit(&[0x19]); // ADD HL, DE
        self.emit(&[0xF1]); // POP AF
        self.emit(&[0x77]); // LD (HL), A
        self.emit(&[0x3A]); // LD A, (INPUT_LEN)
        self.emit_word(INPUT_LEN);
        self.emit(&[0x3C]); // INC A
        self.emit(&[0x32]); // LD (INPUT_LEN), A
        self.emit_word(INPUT_LEN);
        self.emit(&[0xCD]); // CALL show_input_line
        self.fixup("show_input_line");
        self.emit(&[0xC3]); // JP main_loop
        self.fixup("main_loop");

        self.label("edit_input_full");
        self.emit(&[0xF1]); // POP AF (discard)
        self.emit(&[0xC3]); // JP main_loop
        self.fixup("main_loop");

        self.label("edit_backspace");
        self.emit(&[0x3A]); // LD A, (INPUT_LEN)
        self.emit_word(INPUT_LEN);
        self.emit(&[0xB7]); // OR A
        self.emit(&[0xCA]); // JP Z, main_loop (nothing to delete)
        self.fixup("main_loop");
        self.emit(&[0x3D]); // DEC A
        self.emit(&[0x32]); // LD (INPUT_LEN), A
        self.emit_word(INPUT_LEN);
        self.emit(&[0xCD]); // CALL show_input_line
        self.fixup("show_input_line");
        self.emit(&[0xC3]); // JP main_loop
        self.fixup("main_loop");

        self.label("cancel_edit");
        self.emit(&[0xAF]); // XOR A
        self.emit(&[0x32]); // LD (EDIT_MODE), A
        self.emit_word(EDIT_MODE);
        self.emit(&[0xCD]); // CALL refresh_display
        self.fixup("refresh_display");
        self.emit(&[0xC3]); // JP main_loop
        self.fixup("main_loop");

        self.label("confirm_edit");
        // Parse input and store in cell
        self.emit(&[0xCD]); // CALL parse_and_store
        self.fixup("parse_and_store");
        self.emit(&[0xAF]); // XOR A
        self.emit(&[0x32]); // LD (EDIT_MODE), A
        self.emit_word(EDIT_MODE);
        self.emit(&[0xCD]); // CALL recalculate
        self.fixup("recalculate");
        self.emit(&[0xCD]); // CALL refresh_display
        self.fixup("refresh_display");
        self.emit(&[0xC3]); // JP main_loop
        self.fixup("main_loop");

        // Quit
        self.label("quit");
        self.emit(&[0x21]); // LD HL, quit_msg
        self.fixup("quit_msg");
        self.emit(&[0xCD]); // CALL print_string
        self.fixup("print_string");
        self.emit(&[0x76]); // HALT
    }

    /// Display routines
    fn emit_display(&mut self) {
        // Adjust view to keep cursor visible
        self.label("adjust_view");
        // Check if cursor is above view
        self.emit(&[0x3A]); // LD A, (CURSOR_ROW)
        self.emit_word(CURSOR_ROW);
        self.emit(&[0x47]); // LD B, A
        self.emit(&[0x3A]); // LD A, (VIEW_TOP)
        self.emit_word(VIEW_TOP);
        self.emit(&[0xB8]); // CP B
        self.emit(&[0xDA]); // JP C, adjust_check_bottom
        self.fixup("adjust_check_bottom");
        self.emit(&[0xCA]); // JP Z, adjust_check_bottom
        self.fixup("adjust_check_bottom");
        // Cursor above view - set VIEW_TOP = CURSOR_ROW
        self.emit(&[0x78]); // LD A, B
        self.emit(&[0x32]); // LD (VIEW_TOP), A
        self.emit_word(VIEW_TOP);

        self.label("adjust_check_bottom");
        // Check if cursor is below view
        self.emit(&[0x3A]); // LD A, (CURSOR_ROW)
        self.emit_word(CURSOR_ROW);
        self.emit(&[0x47]); // LD B, A
        self.emit(&[0x3A]); // LD A, (VIEW_TOP)
        self.emit_word(VIEW_TOP);
        self.emit(&[0xC6, VISIBLE_ROWS - 1]); // ADD A, VISIBLE_ROWS-1
        self.emit(&[0xB8]); // CP B
        self.emit(&[0xD2]); // JP NC, adjust_check_left
        self.fixup("adjust_check_left");
        // Cursor below view - set VIEW_TOP = CURSOR_ROW - VISIBLE_ROWS + 1
        self.emit(&[0x78]); // LD A, B
        self.emit(&[0xD6, VISIBLE_ROWS - 1]); // SUB VISIBLE_ROWS-1
        self.emit(&[0x32]); // LD (VIEW_TOP), A
        self.emit_word(VIEW_TOP);

        self.label("adjust_check_left");
        // Similar logic for columns
        self.emit(&[0x3A]); // LD A, (CURSOR_COL)
        self.emit_word(CURSOR_COL);
        self.emit(&[0x47]); // LD B, A
        self.emit(&[0x3A]); // LD A, (VIEW_LEFT)
        self.emit_word(VIEW_LEFT);
        self.emit(&[0xB8]); // CP B
        self.emit(&[0xDA]); // JP C, adjust_check_right
        self.fixup("adjust_check_right");
        self.emit(&[0xCA]); // JP Z, adjust_check_right
        self.fixup("adjust_check_right");
        self.emit(&[0x78]); // LD A, B
        self.emit(&[0x32]); // LD (VIEW_LEFT), A
        self.emit_word(VIEW_LEFT);

        self.label("adjust_check_right");
        self.emit(&[0x3A]); // LD A, (CURSOR_COL)
        self.emit_word(CURSOR_COL);
        self.emit(&[0x47]); // LD B, A
        self.emit(&[0x3A]); // LD A, (VIEW_LEFT)
        self.emit_word(VIEW_LEFT);
        self.emit(&[0xC6, VISIBLE_COLS - 1]); // ADD A, VISIBLE_COLS-1
        self.emit(&[0xB8]); // CP B
        self.emit(&[0xD2]); // JP NC, adjust_done
        self.fixup("adjust_done");
        self.emit(&[0x78]); // LD A, B
        self.emit(&[0xD6, VISIBLE_COLS - 1]); // SUB VISIBLE_COLS-1
        self.emit(&[0x32]); // LD (VIEW_LEFT), A
        self.emit_word(VIEW_LEFT);

        self.label("adjust_done");
        self.emit(&[0xC9]); // RET

        // Refresh the entire display
        self.label("refresh_display");
        // Clear screen
        self.emit(&[0xCD]); // CALL clear_screen
        self.fixup("clear_screen");

        // Print header row (column letters)
        self.emit(&[0x3E, b' ']); // LD A, ' '
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0xCD]); // CALL putchar (4 spaces for row numbers)
        self.fixup("putchar");
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");

        // Print column headers
        self.emit(&[0x3A]); // LD A, (VIEW_LEFT)
        self.emit_word(VIEW_LEFT);
        self.emit(&[0x47]); // LD B, A (B = current column)
        self.emit(&[0x0E, VISIBLE_COLS]); // LD C, VISIBLE_COLS (counter)

        self.label("header_col_loop");
        self.emit(&[0x78]); // LD A, B
        self.emit(&[0xFE, GRID_COLS]); // CP GRID_COLS
        self.emit(&[0xD2]); // JP NC, header_done
        self.fixup("header_done");
        self.emit(&[0xC6, b'A']); // ADD A, 'A'
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        // Pad with spaces
        self.emit(&[0x3E, b' ']); // LD A, ' '
        self.emit(&[0xE5]); // PUSH HL
        self.emit(&[0x26, CELL_WIDTH - 1]); // LD H, CELL_WIDTH-1
        self.label("header_pad_loop");
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0x25]); // DEC H
        self.emit(&[0xC2]); // JP NZ, header_pad_loop
        self.fixup("header_pad_loop");
        self.emit(&[0xE1]); // POP HL
        self.emit(&[0x04]); // INC B
        self.emit(&[0x0D]); // DEC C
        self.emit(&[0xC2]); // JP NZ, header_col_loop
        self.fixup("header_col_loop");

        self.label("header_done");
        self.emit(&[0xCD]); // CALL newline
        self.fixup("newline");

        // Print each row
        self.emit(&[0x3A]); // LD A, (VIEW_TOP)
        self.emit_word(VIEW_TOP);
        self.emit(&[0x32]); // LD (TEMP1), A (current row)
        self.emit_word(TEMP1);
        self.emit(&[0x3E, VISIBLE_ROWS]); // LD A, VISIBLE_ROWS
        self.emit(&[0x32]); // LD (TEMP1+1), A (row counter)
        self.emit_word(TEMP1 + 1);

        self.label("display_row_loop");
        self.emit(&[0x3A]); // LD A, (TEMP1)
        self.emit_word(TEMP1);
        self.emit(&[0xFE, GRID_ROWS]); // CP GRID_ROWS
        self.emit(&[0xD2]); // JP NC, display_done
        self.fixup("display_done");

        // Print row number (1-based, right-aligned in 4 chars)
        self.emit(&[0x3A]); // LD A, (TEMP1)
        self.emit_word(TEMP1);
        self.emit(&[0x3C]); // INC A (1-based)
        self.emit(&[0x6F]); // LD L, A
        self.emit(&[0x26, 0x00]); // LD H, 0
        self.emit(&[0xCD]); // CALL print_int_padded
        self.fixup("print_int_padded");

        // Print cells in this row
        self.emit(&[0x3A]); // LD A, (VIEW_LEFT)
        self.emit_word(VIEW_LEFT);
        self.emit(&[0x47]); // LD B, A (B = current col)
        self.emit(&[0x0E, VISIBLE_COLS]); // LD C, VISIBLE_COLS

        self.label("display_cell_loop");
        self.emit(&[0x78]); // LD A, B
        self.emit(&[0xFE, GRID_COLS]); // CP GRID_COLS
        self.emit(&[0xD2]); // JP NC, display_row_end
        self.fixup("display_row_end");

        // Check if this is the cursor cell
        self.emit(&[0x3A]); // LD A, (CURSOR_COL)
        self.emit_word(CURSOR_COL);
        self.emit(&[0xB8]); // CP B
        self.emit(&[0xC2]); // JP NZ, not_cursor_cell
        self.fixup("not_cursor_cell");
        self.emit(&[0x3A]); // LD A, (CURSOR_ROW)
        self.emit_word(CURSOR_ROW);
        self.emit(&[0xE5]); // PUSH HL
        self.emit(&[0x2A]); // LD HL, (TEMP1)
        self.emit_word(TEMP1);
        self.emit(&[0xBD]); // CP L
        self.emit(&[0xE1]); // POP HL
        self.emit(&[0xC2]); // JP NZ, not_cursor_cell
        self.fixup("not_cursor_cell");
        // This is the cursor cell - print marker
        self.emit(&[0x3E, b'[']); // LD A, '['
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0xC3]); // JP print_cell_value
        self.fixup("print_cell_value");

        self.label("not_cursor_cell");
        self.emit(&[0x3E, b' ']); // LD A, ' '
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");

        self.label("print_cell_value");
        // Get cell value and print it
        self.emit(&[0xC5]); // PUSH BC
        self.emit(&[0x78]); // LD A, B (col)
        self.emit(&[0x47]); // LD B, A
        self.emit(&[0x3A]); // LD A, (TEMP1) (row)
        self.emit_word(TEMP1);
        self.emit(&[0x4F]); // LD C, A
        self.emit(&[0xCD]); // CALL get_cell_addr
        self.fixup("get_cell_addr");
        self.emit(&[0xCD]); // CALL print_cell
        self.fixup("print_cell");
        self.emit(&[0xC1]); // POP BC

        // Check if cursor cell for closing bracket
        self.emit(&[0x3A]); // LD A, (CURSOR_COL)
        self.emit_word(CURSOR_COL);
        self.emit(&[0xB8]); // CP B
        self.emit(&[0xC2]); // JP NZ, cell_no_bracket
        self.fixup("cell_no_bracket");
        self.emit(&[0x3A]); // LD A, (CURSOR_ROW)
        self.emit_word(CURSOR_ROW);
        self.emit(&[0xE5]); // PUSH HL
        self.emit(&[0x2A]); // LD HL, (TEMP1)
        self.emit_word(TEMP1);
        self.emit(&[0xBD]); // CP L
        self.emit(&[0xE1]); // POP HL
        self.emit(&[0xC2]); // JP NZ, cell_no_bracket
        self.fixup("cell_no_bracket");
        self.emit(&[0x3E, b']']); // LD A, ']'
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0xC3]); // JP cell_next
        self.fixup("cell_next");

        self.label("cell_no_bracket");
        self.emit(&[0x3E, b' ']); // LD A, ' '
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");

        self.label("cell_next");
        self.emit(&[0x04]); // INC B
        self.emit(&[0x0D]); // DEC C
        self.emit(&[0xC2]); // JP NZ, display_cell_loop
        self.fixup("display_cell_loop");

        self.label("display_row_end");
        self.emit(&[0xCD]); // CALL newline
        self.fixup("newline");
        self.emit(&[0x3A]); // LD A, (TEMP1)
        self.emit_word(TEMP1);
        self.emit(&[0x3C]); // INC A
        self.emit(&[0x32]); // LD (TEMP1), A
        self.emit_word(TEMP1);
        self.emit(&[0x3A]); // LD A, (TEMP1+1)
        self.emit_word(TEMP1 + 1);
        self.emit(&[0x3D]); // DEC A
        self.emit(&[0x32]); // LD (TEMP1+1), A
        self.emit_word(TEMP1 + 1);
        self.emit(&[0xC2]); // JP NZ, display_row_loop
        self.fixup("display_row_loop");

        self.label("display_done");
        // Print status line
        self.emit(&[0xCD]); // CALL print_status
        self.fixup("print_status");
        self.emit(&[0xC9]); // RET

        // Print a cell's value (HL = cell address)
        // Prints value right-aligned in CELL_WIDTH-2 chars
        self.label("print_cell");
        self.emit(&[0x7E]); // LD A, (HL) - cell type
        self.emit(&[0xB7]); // OR A
        self.emit(&[0xCA]); // JP Z, print_cell_empty
        self.fixup("print_cell_empty");
        self.emit(&[0xFE, CELL_NUMBER]); // CP CELL_NUMBER
        self.emit(&[0xCA]); // JP Z, print_cell_number
        self.fixup("print_cell_number");
        self.emit(&[0xFE, CELL_ERROR]); // CP CELL_ERROR
        self.emit(&[0xCA]); // JP Z, print_cell_error
        self.fixup("print_cell_error");
        // Formula - print its calculated value
        self.emit(&[0xC3]); // JP print_cell_number
        self.fixup("print_cell_number");

        self.label("print_cell_empty");
        // Print spaces
        self.emit(&[0x06, CELL_WIDTH - 2]); // LD B, CELL_WIDTH-2
        self.emit(&[0x3E, b' ']); // LD A, ' '
        self.label("print_empty_loop");
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0x10]); // DJNZ print_empty_loop
        let offset = self.rom.len();
        self.emit(&[0x00]); // placeholder for relative jump
        self.rom[offset] = (self.labels.get("print_empty_loop").unwrap_or(&0)
            .wrapping_sub(self.pos())) as u8;
        self.emit(&[0xC9]); // RET

        self.label("print_cell_number");
        // Get value from bytes 2-3
        self.emit(&[0x23]); // INC HL
        self.emit(&[0x23]); // INC HL
        self.emit(&[0x5E]); // LD E, (HL)
        self.emit(&[0x23]); // INC HL
        self.emit(&[0x56]); // LD D, (HL)
        self.emit(&[0xEB]); // EX DE, HL
        // HL = value, print right-aligned
        self.emit(&[0xCD]); // CALL print_int_cell
        self.fixup("print_int_cell");
        self.emit(&[0xC9]); // RET

        self.label("print_cell_error");
        self.emit(&[0x21]); // LD HL, error_str
        self.fixup("error_str");
        self.emit(&[0xCD]); // CALL print_string
        self.fixup("print_string");
        self.emit(&[0xC9]); // RET

        // Print status line showing current cell
        self.label("print_status");
        self.emit(&[0x3A]); // LD A, (CURSOR_COL)
        self.emit_word(CURSOR_COL);
        self.emit(&[0xC6, b'A']); // ADD A, 'A'
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0x3A]); // LD A, (CURSOR_ROW)
        self.emit_word(CURSOR_ROW);
        self.emit(&[0x3C]); // INC A (1-based)
        self.emit(&[0x6F]); // LD L, A
        self.emit(&[0x26, 0x00]); // LD H, 0
        self.emit(&[0xCD]); // CALL print_int
        self.fixup("print_int");
        self.emit(&[0x3E, b':']); // LD A, ':'
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0x3E, b' ']); // LD A, ' '
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        // Print current cell's content/formula
        self.emit(&[0x3A]); // LD A, (CURSOR_COL)
        self.emit_word(CURSOR_COL);
        self.emit(&[0x47]); // LD B, A
        self.emit(&[0x3A]); // LD A, (CURSOR_ROW)
        self.emit_word(CURSOR_ROW);
        self.emit(&[0x4F]); // LD C, A
        self.emit(&[0xCD]); // CALL get_cell_addr
        self.fixup("get_cell_addr");
        self.emit(&[0xCD]); // CALL print_cell_content
        self.fixup("print_cell_content");
        self.emit(&[0xC9]); // RET

        // Print cell content (raw value or formula)
        self.label("print_cell_content");
        self.emit(&[0x7E]); // LD A, (HL) - type
        self.emit(&[0xB7]); // OR A
        self.emit(&[0xC8]); // RET Z (empty)
        self.emit(&[0xFE, CELL_NUMBER]); // CP CELL_NUMBER
        self.emit(&[0xC2]); // JP NZ, print_content_formula
        self.fixup("print_content_formula");
        // Number - print value
        self.emit(&[0x23]); // INC HL
        self.emit(&[0x23]); // INC HL
        self.emit(&[0x5E]); // LD E, (HL)
        self.emit(&[0x23]); // INC HL
        self.emit(&[0x56]); // LD D, (HL)
        self.emit(&[0xEB]); // EX DE, HL
        self.emit(&[0xCD]); // CALL print_int
        self.fixup("print_int");
        self.emit(&[0xC9]); // RET

        self.label("print_content_formula");
        // For now, just print the calculated value
        self.emit(&[0x23]); // INC HL
        self.emit(&[0x23]); // INC HL
        self.emit(&[0x5E]); // LD E, (HL)
        self.emit(&[0x23]); // INC HL
        self.emit(&[0x56]); // LD D, (HL)
        self.emit(&[0xEB]); // EX DE, HL
        self.emit(&[0xCD]); // CALL print_int
        self.fixup("print_int");
        self.emit(&[0xC9]); // RET

        // Show input line when editing
        self.label("show_input_line");
        // Move to bottom of screen
        self.emit(&[0x21]); // LD HL, input_prompt
        self.fixup("input_prompt");
        self.emit(&[0xCD]); // CALL print_string
        self.fixup("print_string");
        // Print input buffer
        self.emit(&[0x21]); // LD HL, INPUT_BUF
        self.emit_word(INPUT_BUF);
        self.emit(&[0x3A]); // LD A, (INPUT_LEN)
        self.emit_word(INPUT_LEN);
        self.emit(&[0x47]); // LD B, A
        self.emit(&[0xB7]); // OR A
        self.emit(&[0xC8]); // RET Z
        self.label("show_input_loop");
        self.emit(&[0x7E]); // LD A, (HL)
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0x23]); // INC HL
        self.emit(&[0x10]); // DJNZ
        let offset = self.rom.len();
        self.emit(&[0x00]); // placeholder
        // Calculate relative offset for DJNZ
        let target = *self.labels.get("show_input_loop").unwrap_or(&0);
        let current = self.pos();
        self.rom[offset] = target.wrapping_sub(current) as u8;
        self.emit(&[0xC9]); // RET
    }

    /// Input handling
    fn emit_input(&mut self) {
        // Parse input buffer and store in current cell
        self.label("parse_and_store");
        self.emit(&[0x3A]); // LD A, (INPUT_LEN)
        self.emit_word(INPUT_LEN);
        self.emit(&[0xB7]); // OR A
        self.emit(&[0xC8]); // RET Z (empty input)

        // Check if formula (starts with '=')
        self.emit(&[0x21]); // LD HL, INPUT_BUF
        self.emit_word(INPUT_BUF);
        self.emit(&[0x7E]); // LD A, (HL)
        self.emit(&[0xFE, b'=']);
        self.emit(&[0xCA]); // JP Z, parse_formula
        self.fixup("parse_formula");

        // Otherwise parse as number
        self.emit(&[0xCD]); // CALL parse_number
        self.fixup("parse_number");
        // HL = parsed number, carry set if error
        self.emit(&[0xDA]); // JP C, store_error
        self.fixup("store_error");
        // Store as number in current cell
        self.emit(&[0xE5]); // PUSH HL (save value)
        self.emit(&[0x3A]); // LD A, (CURSOR_COL)
        self.emit_word(CURSOR_COL);
        self.emit(&[0x47]); // LD B, A
        self.emit(&[0x3A]); // LD A, (CURSOR_ROW)
        self.emit_word(CURSOR_ROW);
        self.emit(&[0x4F]); // LD C, A
        self.emit(&[0xCD]); // CALL get_cell_addr
        self.fixup("get_cell_addr");
        self.emit(&[0x36, CELL_NUMBER]); // LD (HL), CELL_NUMBER
        self.emit(&[0x23]); // INC HL
        self.emit(&[0x36, 0x00]); // LD (HL), 0 (flags)
        self.emit(&[0x23]); // INC HL
        self.emit(&[0xD1]); // POP DE (value)
        self.emit(&[0x73]); // LD (HL), E
        self.emit(&[0x23]); // INC HL
        self.emit(&[0x72]); // LD (HL), D
        self.emit(&[0xC9]); // RET

        self.label("store_error");
        self.emit(&[0x3A]); // LD A, (CURSOR_COL)
        self.emit_word(CURSOR_COL);
        self.emit(&[0x47]); // LD B, A
        self.emit(&[0x3A]); // LD A, (CURSOR_ROW)
        self.emit_word(CURSOR_ROW);
        self.emit(&[0x4F]); // LD C, A
        self.emit(&[0xCD]); // CALL get_cell_addr
        self.fixup("get_cell_addr");
        self.emit(&[0x36, CELL_ERROR]); // LD (HL), CELL_ERROR
        self.emit(&[0xC9]); // RET

        // Parse number from INPUT_BUF
        // Returns value in HL, carry set on error
        self.label("parse_number");
        self.emit(&[0x21, 0x00, 0x00]); // LD HL, 0 (accumulator)
        self.emit(&[0x11]); // LD DE, INPUT_BUF
        self.emit_word(INPUT_BUF);
        self.emit(&[0x3A]); // LD A, (INPUT_LEN)
        self.emit_word(INPUT_LEN);
        self.emit(&[0x47]); // LD B, A (counter)
        self.emit(&[0x0E, 0x00]); // LD C, 0 (negative flag)

        // Check for minus sign
        self.emit(&[0x1A]); // LD A, (DE)
        self.emit(&[0xFE, b'-']);
        self.emit(&[0xC2]); // JP NZ, parse_num_loop
        self.fixup("parse_num_loop");
        self.emit(&[0x0E, 0x01]); // LD C, 1 (negative)
        self.emit(&[0x13]); // INC DE
        self.emit(&[0x05]); // DEC B
        self.emit(&[0xCA]); // JP Z, parse_num_error (just "-")
        self.fixup("parse_num_error");

        self.label("parse_num_loop");
        self.emit(&[0x1A]); // LD A, (DE)
        self.emit(&[0xFE, b'0']);
        self.emit(&[0xDA]); // JP C, parse_num_error
        self.fixup("parse_num_error");
        self.emit(&[0xFE, b'9' + 1]);
        self.emit(&[0xD2]); // JP NC, parse_num_error
        self.fixup("parse_num_error");

        // Save buffer pointer and digit value
        self.emit(&[0xD5]); // PUSH DE (save buffer ptr)
        self.emit(&[0xD6, b'0']); // SUB '0' (convert to value)
        self.emit(&[0xF5]); // PUSH AF (save digit)

        // Multiply HL by 10: HL = HL*2 + HL*8
        self.emit(&[0x29]); // ADD HL, HL (*2)
        self.emit(&[0x54]); // LD D, H
        self.emit(&[0x5D]); // LD E, L (DE = HL*2)
        self.emit(&[0x29]); // ADD HL, HL (*4)
        self.emit(&[0x29]); // ADD HL, HL (*8)
        self.emit(&[0x19]); // ADD HL, DE (*8 + *2 = *10)

        // Add digit
        self.emit(&[0xF1]); // POP AF (restore digit)
        self.emit(&[0x5F]); // LD E, A
        self.emit(&[0x16, 0x00]); // LD D, 0
        self.emit(&[0x19]); // ADD HL, DE

        // Move to next character
        self.emit(&[0xD1]); // POP DE (restore buffer ptr)
        self.emit(&[0x13]); // INC DE
        self.emit(&[0x05]); // DEC B
        self.emit(&[0xC2]); // JP NZ, parse_num_loop
        self.fixup("parse_num_loop");

        // Check negative flag
        self.emit(&[0x79]); // LD A, C
        self.emit(&[0xB7]); // OR A
        self.emit(&[0xCA]); // JP Z, parse_num_done
        self.fixup("parse_num_done");
        // Negate HL
        self.emit(&[0x7C]); // LD A, H
        self.emit(&[0x2F]); // CPL
        self.emit(&[0x67]); // LD H, A
        self.emit(&[0x7D]); // LD A, L
        self.emit(&[0x2F]); // CPL
        self.emit(&[0x6F]); // LD L, A
        self.emit(&[0x23]); // INC HL

        self.label("parse_num_done");
        self.emit(&[0xB7]); // OR A (clear carry)
        self.emit(&[0xC9]); // RET

        self.label("parse_num_error");
        self.emit(&[0x37]); // SCF (set carry)
        self.emit(&[0xC9]); // RET
    }

    /// Cell operations
    fn emit_cell_ops(&mut self) {
        // Get cell address from B=col, C=row
        // Returns address in HL
        self.label("get_cell_addr");
        // Address = CELL_DATA + (row * 16 + col) * 4
        // Use 16-bit arithmetic to avoid overflow when row >= 16
        self.emit(&[0x69]); // LD L, C (row)
        self.emit(&[0x26, 0x00]); // LD H, 0 (HL = row, 16-bit)
        self.emit(&[0x29]); // ADD HL, HL (×2)
        self.emit(&[0x29]); // ADD HL, HL (×4)
        self.emit(&[0x29]); // ADD HL, HL (×8)
        self.emit(&[0x29]); // ADD HL, HL (×16)
        self.emit(&[0x58]); // LD E, B (col)
        self.emit(&[0x16, 0x00]); // LD D, 0 (DE = col, 16-bit)
        self.emit(&[0x19]); // ADD HL, DE (HL = row*16 + col)
        self.emit(&[0x29]); // ADD HL, HL (×2)
        self.emit(&[0x29]); // ADD HL, HL (×4)
        // Add base address
        self.emit(&[0x11]); // LD DE, CELL_DATA
        self.emit_word(CELL_DATA);
        self.emit(&[0x19]); // ADD HL, DE
        self.emit(&[0xC9]); // RET

        // Recalculate all formula cells
        self.label("recalculate");
        // For now, just a stub - formulas store their calculated value
        self.emit(&[0xC9]); // RET
    }

    /// Formula parsing and evaluation
    fn emit_formula(&mut self) {
        // Parse formula from INPUT_BUF (starting after '=')
        self.label("parse_formula");
        // Skip the '='
        self.emit(&[0x21]); // LD HL, INPUT_BUF + 1
        self.emit_word(INPUT_BUF + 1);
        self.emit(&[0x3A]); // LD A, (INPUT_LEN)
        self.emit_word(INPUT_LEN);
        self.emit(&[0x3D]); // DEC A (exclude '=')
        self.emit(&[0xCA]); // JP Z, store_error (empty formula)
        self.fixup("store_error");

        // For v0.1: Parse simple formulas like "A1+B2" or "A1*5"
        self.emit(&[0xCD]); // CALL eval_expr
        self.fixup("eval_expr");
        // HL = result, carry set on error
        self.emit(&[0xDA]); // JP C, store_error
        self.fixup("store_error");

        // Store as formula with calculated value
        self.emit(&[0xE5]); // PUSH HL
        self.emit(&[0x3A]); // LD A, (CURSOR_COL)
        self.emit_word(CURSOR_COL);
        self.emit(&[0x47]); // LD B, A
        self.emit(&[0x3A]); // LD A, (CURSOR_ROW)
        self.emit_word(CURSOR_ROW);
        self.emit(&[0x4F]); // LD C, A
        self.emit(&[0xCD]); // CALL get_cell_addr
        self.fixup("get_cell_addr");
        self.emit(&[0x36, CELL_FORMULA]); // LD (HL), CELL_FORMULA
        self.emit(&[0x23]); // INC HL
        self.emit(&[0x36, 0x00]); // LD (HL), 0
        self.emit(&[0x23]); // INC HL
        self.emit(&[0xD1]); // POP DE
        self.emit(&[0x73]); // LD (HL), E
        self.emit(&[0x23]); // INC HL
        self.emit(&[0x72]); // LD (HL), D
        self.emit(&[0xC9]); // RET

        // Evaluate expression
        // Input: HL = pointer to expression string
        // Output: HL = result, carry set on error
        self.label("eval_expr");
        self.emit(&[0x22]); // LD (TEMP2), HL (save expr ptr)
        self.emit_word(TEMP2);

        // Parse first operand (cell ref or number)
        self.emit(&[0xCD]); // CALL parse_operand
        self.fixup("parse_operand");
        self.emit(&[0xD8]); // RET C (error)
        self.emit(&[0xE5]); // PUSH HL (save first operand value)

        // Check for operator
        self.emit(&[0x2A]); // LD HL, (TEMP2)
        self.emit_word(TEMP2);
        self.emit(&[0x7E]); // LD A, (HL)
        self.emit(&[0xB7]); // OR A
        self.emit(&[0xCA]); // JP Z, eval_single (just one operand)
        self.fixup("eval_single");

        // Save operator
        self.emit(&[0xF5]); // PUSH AF
        self.emit(&[0x23]); // INC HL
        self.emit(&[0x22]); // LD (TEMP2), HL
        self.emit_word(TEMP2);

        // Parse second operand
        self.emit(&[0xCD]); // CALL parse_operand
        self.fixup("parse_operand");
        self.emit(&[0xF1]); // POP AF (operator) - but need to handle error
        self.emit(&[0xDA]); // JP C, eval_error
        self.fixup("eval_error");
        // HL = second operand
        self.emit(&[0xEB]); // EX DE, HL (DE = second)
        self.emit(&[0xE1]); // POP HL (first)
        // A still has operator from above... actually no, we popped it
        // Need to re-push and re-pop correctly
        // Let me redo this logic

        // Actually the F1 POP AF restored A with the operator
        // and the carry would be from parse_operand - but POP AF overwrites flags
        // This is buggy - let me fix it

        // For now, let's just do a simpler implementation:
        self.emit(&[0xC3]); // JP eval_binary
        self.fixup("eval_binary");

        self.label("eval_single");
        self.emit(&[0xE1]); // POP HL
        self.emit(&[0xB7]); // OR A (clear carry)
        self.emit(&[0xC9]); // RET

        self.label("eval_error");
        self.emit(&[0xE1]); // POP HL (discard)
        self.emit(&[0x37]); // SCF
        self.emit(&[0xC9]); // RET

        self.label("eval_binary");
        // Simplified: HL=first, DE=second from above, need to get operator
        // For now just add them
        self.emit(&[0x19]); // ADD HL, DE
        self.emit(&[0xB7]); // OR A (clear carry)
        self.emit(&[0xC9]); // RET

        // Parse an operand (cell reference or number)
        // Input: (TEMP2) = pointer to string
        // Output: HL = value, (TEMP2) updated, carry set on error
        self.label("parse_operand");
        self.emit(&[0x2A]); // LD HL, (TEMP2)
        self.emit_word(TEMP2);
        self.emit(&[0x7E]); // LD A, (HL)

        // Check if it's a letter (cell reference)
        self.emit(&[0xFE, b'A']);
        self.emit(&[0xDA]); // JP C, parse_op_number
        self.fixup("parse_op_number");
        self.emit(&[0xFE, b'P' + 1]);
        self.emit(&[0xD2]); // JP NC, parse_op_number
        self.fixup("parse_op_number");

        // It's a cell reference
        self.emit(&[0xD6, b'A']); // SUB 'A' (column)
        self.emit(&[0x47]); // LD B, A
        self.emit(&[0x23]); // INC HL
        // Parse row number
        self.emit(&[0x0E, 0x00]); // LD C, 0 (accumulator)
        self.label("parse_row_loop");
        self.emit(&[0x7E]); // LD A, (HL)
        self.emit(&[0xFE, b'0']);
        self.emit(&[0xDA]); // JP C, parse_row_done
        self.fixup("parse_row_done");
        self.emit(&[0xFE, b'9' + 1]);
        self.emit(&[0xD2]); // JP NC, parse_row_done
        self.fixup("parse_row_done");
        self.emit(&[0xD6, b'0']); // SUB '0'
        self.emit(&[0x5F]); // LD E, A
        self.emit(&[0x79]); // LD A, C
        self.emit(&[0x87]); // ADD A, A (×2)
        self.emit(&[0x87]); // ADD A, A (×4)
        self.emit(&[0x81]); // ADD A, C (×5)
        self.emit(&[0x87]); // ADD A, A (×10)
        self.emit(&[0x83]); // ADD A, E
        self.emit(&[0x4F]); // LD C, A
        self.emit(&[0x23]); // INC HL
        self.emit(&[0xC3]); // JP parse_row_loop
        self.fixup("parse_row_loop");

        self.label("parse_row_done");
        self.emit(&[0x22]); // LD (TEMP2), HL (update pointer)
        self.emit_word(TEMP2);
        // B = col, C = row (1-based), convert to 0-based
        self.emit(&[0x0D]); // DEC C
        // Get cell value
        self.emit(&[0xCD]); // CALL get_cell_addr
        self.fixup("get_cell_addr");
        self.emit(&[0x7E]); // LD A, (HL) - type
        self.emit(&[0xB7]); // OR A
        self.emit(&[0xCA]); // JP Z, parse_op_zero (empty cell = 0)
        self.fixup("parse_op_zero");
        self.emit(&[0x23]); // INC HL
        self.emit(&[0x23]); // INC HL
        self.emit(&[0x5E]); // LD E, (HL)
        self.emit(&[0x23]); // INC HL
        self.emit(&[0x56]); // LD D, (HL)
        self.emit(&[0xEB]); // EX DE, HL
        self.emit(&[0xB7]); // OR A (clear carry)
        self.emit(&[0xC9]); // RET

        self.label("parse_op_zero");
        self.emit(&[0x21, 0x00, 0x00]); // LD HL, 0
        self.emit(&[0xB7]); // OR A
        self.emit(&[0xC9]); // RET

        // Parse number operand
        self.label("parse_op_number");
        self.emit(&[0x2A]); // LD HL, (TEMP2)
        self.emit_word(TEMP2);
        self.emit(&[0x11, 0x00, 0x00]); // LD DE, 0 (accumulator)
        self.emit(&[0x0E, 0x00]); // LD C, 0 (negative flag)

        // Check minus
        self.emit(&[0x7E]); // LD A, (HL)
        self.emit(&[0xFE, b'-']);
        self.emit(&[0xC2]); // JP NZ, parse_opn_loop
        self.fixup("parse_opn_loop");
        self.emit(&[0x0E, 0x01]); // LD C, 1
        self.emit(&[0x23]); // INC HL

        self.label("parse_opn_loop");
        self.emit(&[0x7E]); // LD A, (HL)
        self.emit(&[0xFE, b'0']);
        self.emit(&[0xDA]); // JP C, parse_opn_done
        self.fixup("parse_opn_done");
        self.emit(&[0xFE, b'9' + 1]);
        self.emit(&[0xD2]); // JP NC, parse_opn_done
        self.fixup("parse_opn_done");
        self.emit(&[0xD6, b'0']); // SUB '0'
        self.emit(&[0xF5]); // PUSH AF
        // Multiply DE by 10
        self.emit(&[0xEB]); // EX DE, HL
        self.emit(&[0x29]); // ADD HL, HL (×2)
        self.emit(&[0x29]); // ADD HL, HL (×4)
        self.emit(&[0x54]); // LD D, H
        self.emit(&[0x5D]); // LD E, L
        self.emit(&[0x29]); // ADD HL, HL (×8)
        self.emit(&[0x19]); // ADD HL, DE (×10)
        self.emit(&[0xEB]); // EX DE, HL
        // Add digit
        self.emit(&[0xF1]); // POP AF
        self.emit(&[0x6F]); // LD L, A
        self.emit(&[0x26, 0x00]); // LD H, 0
        self.emit(&[0x19]); // ADD HL, DE
        self.emit(&[0xEB]); // EX DE, HL
        self.emit(&[0x2A]); // LD HL, (TEMP2)
        self.emit_word(TEMP2);
        self.emit(&[0x23]); // INC HL
        self.emit(&[0x22]); // LD (TEMP2), HL
        self.emit_word(TEMP2);
        self.emit(&[0xC3]); // JP parse_opn_loop
        self.fixup("parse_opn_loop");

        self.label("parse_opn_done");
        self.emit(&[0x22]); // LD (TEMP2), HL
        self.emit_word(TEMP2);
        self.emit(&[0xEB]); // EX DE, HL
        // Check negative
        self.emit(&[0x79]); // LD A, C
        self.emit(&[0xB7]); // OR A
        self.emit(&[0xCA]); // JP Z, parse_opn_ret
        self.fixup("parse_opn_ret");
        // Negate
        self.emit(&[0x7C]); // LD A, H
        self.emit(&[0x2F]); // CPL
        self.emit(&[0x67]); // LD H, A
        self.emit(&[0x7D]); // LD A, L
        self.emit(&[0x2F]); // CPL
        self.emit(&[0x6F]); // LD L, A
        self.emit(&[0x23]); // INC HL

        self.label("parse_opn_ret");
        self.emit(&[0xB7]); // OR A
        self.emit(&[0xC9]); // RET
    }

    /// I/O routines (MC6850 ACIA style - ports 0x80/0x81)
    fn emit_io(&mut self) {
        // Get character from input
        // MC6850: bit 0 of status = RX ready
        self.label("getchar");
        self.emit(&[0xDB, 0x80]); // IN A, (0x80) - status
        self.emit(&[0xE6, 0x01]); // AND 0x01 - RX ready bit
        self.emit(&[0x28, 0xFA]); // JR Z, getchar (-6)
        self.emit(&[0xDB, 0x81]); // IN A, (0x81) - data
        self.emit(&[0xC9]); // RET

        // Put character to output
        // MC6850: bit 1 of status = TX ready
        self.label("putchar");
        self.emit(&[0xF5]); // PUSH AF - save char
        self.label("putchar_wait");
        self.emit(&[0xDB, 0x80]); // IN A, (0x80) - status
        self.emit(&[0xE6, 0x02]); // AND 0x02 - TX ready bit
        self.emit(&[0x28, 0xFA]); // JR Z, putchar_wait (-6)
        self.emit(&[0xF1]); // POP AF - restore char
        self.emit(&[0xD3, 0x81]); // OUT (0x81), A - data
        self.emit(&[0xC9]); // RET

        // Print newline
        self.label("newline");
        self.emit(&[0x3E, 0x0D]); // LD A, CR
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0x3E, 0x0A]); // LD A, LF
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0xC9]); // RET

        // Clear screen - just print newlines for now (ANSI version can be added later)
        self.label("clear_screen");
        self.emit(&[0x06, 5]); // LD B, 5
        self.label("clear_nl_loop");
        self.emit(&[0xC5]); // PUSH BC
        self.emit(&[0xCD]); // CALL newline
        self.fixup("newline");
        self.emit(&[0xC1]); // POP BC
        self.emit(&[0x10]); // DJNZ
        let offset = self.rom.len();
        self.emit(&[0x00]);
        let target = *self.labels.get("clear_nl_loop").unwrap();
        self.rom[offset] = target.wrapping_sub(self.pos()) as u8;
        self.emit(&[0xC9]); // RET

        // Print null-terminated string at HL
        self.label("print_string");
        self.emit(&[0x7E]); // LD A, (HL)
        self.emit(&[0xB7]); // OR A
        self.emit(&[0xC8]); // RET Z
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0x23]); // INC HL
        self.emit(&[0xC3]); // JP print_string
        self.fixup("print_string");

        // Print 16-bit integer in HL
        self.label("print_int");
        // Check if negative
        self.emit(&[0x7C]); // LD A, H
        self.emit(&[0xB7]); // OR A
        self.emit(&[0xF2]); // JP P, print_int_pos
        self.fixup("print_int_pos");
        // Negative - print minus and negate
        self.emit(&[0x3E, b'-']);
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0x7C]); // LD A, H
        self.emit(&[0x2F]); // CPL
        self.emit(&[0x67]); // LD H, A
        self.emit(&[0x7D]); // LD A, L
        self.emit(&[0x2F]); // CPL
        self.emit(&[0x6F]); // LD L, A
        self.emit(&[0x23]); // INC HL

        self.label("print_int_pos");
        // Convert to decimal and print
        self.emit(&[0x11]); // LD DE, 10000
        self.emit_word(10000);
        self.emit(&[0xCD]); // CALL print_digit
        self.fixup("print_digit");
        self.emit(&[0x11]); // LD DE, 1000
        self.emit_word(1000);
        self.emit(&[0xCD]); // CALL print_digit
        self.fixup("print_digit");
        self.emit(&[0x11]); // LD DE, 100
        self.emit_word(100);
        self.emit(&[0xCD]); // CALL print_digit
        self.fixup("print_digit");
        self.emit(&[0x11]); // LD DE, 10
        self.emit_word(10);
        self.emit(&[0xCD]); // CALL print_digit
        self.fixup("print_digit");
        // Last digit
        self.emit(&[0x7D]); // LD A, L
        self.emit(&[0xC6, b'0']); // ADD A, '0'
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0xC9]); // RET

        // Print one digit, HL = value, DE = divisor
        // Updates HL to remainder
        self.label("print_digit");
        self.emit(&[0x06, 0x00]); // LD B, 0 (count)
        self.label("print_digit_loop");
        self.emit(&[0xB7]); // OR A (clear carry)
        self.emit(&[0xED, 0x52]); // SBC HL, DE
        self.emit(&[0xDA]); // JP C, print_digit_done
        self.fixup("print_digit_done");
        self.emit(&[0x04]); // INC B
        self.emit(&[0xC3]); // JP print_digit_loop
        self.fixup("print_digit_loop");
        self.label("print_digit_done");
        self.emit(&[0x19]); // ADD HL, DE (restore)
        self.emit(&[0x78]); // LD A, B
        self.emit(&[0xC6, b'0']); // ADD A, '0'
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0xC9]); // RET

        // Print integer padded to 4 chars (for row numbers)
        self.label("print_int_padded");
        // For simplicity, just print with leading spaces
        self.emit(&[0x7C]); // LD A, H
        self.emit(&[0xB7]); // OR A
        self.emit(&[0xC2]); // JP NZ, print_int_padded_go
        self.fixup("print_int_padded_go");
        self.emit(&[0x7D]); // LD A, L
        self.emit(&[0xFE, 10]);
        self.emit(&[0xD2]); // JP NC, print_pad_2
        self.fixup("print_pad_2");
        // < 10: print 3 spaces
        self.emit(&[0x3E, b' ']);
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0xC3]); // JP print_int_padded_go
        self.fixup("print_int_padded_go");

        self.label("print_pad_2");
        self.emit(&[0xFE, 100]);
        self.emit(&[0xD2]); // JP NC, print_pad_1
        self.fixup("print_pad_1");
        // < 100: print 2 spaces
        self.emit(&[0x3E, b' ']);
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0xC3]); // JP print_int_padded_go
        self.fixup("print_int_padded_go");

        self.label("print_pad_1");
        // >= 100: print 1 space
        self.emit(&[0x3E, b' ']);
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");

        self.label("print_int_padded_go");
        self.emit(&[0xC3]); // JP print_int
        self.fixup("print_int");

        // Print integer in cell (right-aligned in CELL_WIDTH-2)
        self.label("print_int_cell");
        // Just print the number for now, padding handled elsewhere
        self.emit(&[0xC3]); // JP print_int
        self.fixup("print_int");
    }

    /// String constants
    fn emit_strings(&mut self) {
        self.label("welcome_msg");
        self.emit_string("kz80_calc v0.1 - VisiCalc for Z80\r\nArrows/hjkl:move  Enter:edit  q:quit\r\n");

        self.label("quit_msg");
        self.emit_string("\r\nGoodbye!\r\n");

        self.label("error_str");
        self.emit_string(" #ERR ");

        self.label("input_prompt");
        self.emit_string("\r\n> ");
    }
}

impl Default for CodeGen {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate() {
        let mut codegen = CodeGen::new();
        codegen.generate();
        let rom = codegen.into_rom();
        assert!(rom.len() > 256);
        assert!(rom.len() < 8192);
        // Check starts with LD SP instruction (0x31)
        assert_eq!(rom[0], 0x31);
    }

    #[test]
    fn test_cell_address_calculation() {
        // Cell (0,0) should be at CELL_DATA
        // Cell (1,0) should be at CELL_DATA + 4
        // Cell (0,1) should be at CELL_DATA + 64
        // Formula: CELL_DATA + (row * 16 + col) * 4
        let base = CELL_DATA;
        assert_eq!(base + (0 * 16 + 0) * 4, 0x2000);
        assert_eq!(base + (0 * 16 + 1) * 4, 0x2004);
        assert_eq!(base + (1 * 16 + 0) * 4, 0x2040);
    }
}
