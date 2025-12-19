//! Z80 code generator for kz80_calc spreadsheet
//!
//! Built on the retroshield-z80 framework.
//!
//! Memory Layout:
//! ROM (8KB):
//!   0x0000-0x00FF  Startup, vectors
//!   0x0100-0x1FFF  Spreadsheet engine
//!
//! RAM (8KB):
//!   0x2000-0x37FF  Cell data (6KB = 1024 cells x 6 bytes)
//!   0x3800-0x38FF  Input buffer (256 bytes)
//!   0x3900-0x39FF  Display line buffer (256 bytes)
//!   0x3A00-0x3DFF  Formula parse buffer, scratch (1KB)
//!   0x3E00-0x3FFF  Stack (512 bytes)
//!
//! Cell format (6 bytes) - 8-digit packed BCD:
//!   byte 0: type (0=empty, 1=number, 2=formula, 3=error, 4=repeat, 5=label)
//!   byte 1: sign (0x00=positive, 0x80=negative)
//!   bytes 2-5: 8-digit packed BCD (big-endian: d7d6 d5d4 d3d2 d1d0)

use std::ops::{Deref, DerefMut};
use retroshield_z80_workbench::CodeGen;

/// Memory constants
const STACK_TOP: u16 = 0x3FFF;

// RAM layout
const CELL_DATA: u16 = 0x2000;      // 6KB for cells (1024 x 6 bytes)
const INPUT_BUF: u16 = 0x3800;      // 256 bytes
const SCRATCH: u16 = 0x3A00;        // 1KB scratch/formula

// Cell size for BCD
const CELL_SIZE: u8 = 6;            // 6 bytes per cell

// Spreadsheet state (in scratch area, above formula storage)
const CURSOR_COL: u16 = 0x3DF0;     // Current column (0-15)
const CURSOR_ROW: u16 = 0x3DF1;     // Current row (0-63)
const VIEW_TOP: u16 = 0x3DF2;       // Top visible row
const VIEW_LEFT: u16 = 0x3DF3;      // Left visible column
const INPUT_LEN: u16 = 0x3DF4;      // Input buffer length
const INPUT_POS: u16 = 0x3DF5;      // Input cursor position
const EDIT_MODE: u16 = 0x3DF6;      // 0=navigate, 1=edit
const TEMP1: u16 = 0x3DF8;          // Temp storage
const TEMP2: u16 = 0x3DFA;          // Temp storage
const FORMULA_PTR: u16 = 0x3DFC;    // Next free position in formula storage
const COL_WIDTH_VAR: u16 = 0x3DFE;  // Column width (default 9)
const RANGE_ROW2: u16 = 0x3DE0;     // Range function end row
const RANGE_COL2: u16 = 0x3DDA;     // Range function end column
const RANGE_CUR_COL: u16 = 0x3DDB;  // Current column in range iteration
const SIGN_ACCUM: u16 = 0x3DDC;     // Sign of formula accumulator (0x00=pos, 0x80=neg)
const SIGN_OP: u16 = 0x3DDD;        // Sign of current operand
const FUNC_TYPE: u16 = 0x3DE1;      // Function type: 0=SUM, 1=AVG, 2=MIN, 3=MAX, 4=COUNT
const FUNC_COUNT: u16 = 0x3DE2;     // Cell count for AVG
const FUNC_MINMAX: u16 = 0x3DE4;    // Min/max accumulator (16-bit)
const FUNC_SIGN: u16 = 0x3DE6;      // Sign of function accumulator (0x00=pos, 0x80=neg)
const FUNC_SIGN2: u16 = 0x3DE7;     // Sign of current cell value in function

// BCD working storage (in scratch area, before state variables)
const BCD_TEMP1: u16 = 0x3DC0;      // 4-byte BCD temp
const BCD_TEMP2: u16 = 0x3DC4;      // 4-byte BCD temp
const BCD_ACCUM: u16 = 0x3DC8;      // 8-byte BCD accumulator for mul (ends at 0x3DCF)
const ATOB_FLAGS: u16 = 0x3DD0;     // 2 bytes: [0]=decimal seen flag, [1]=frac digit count
const FUNC_BCD: u16 = 0x3DD2;       // 4-byte BCD for function SUM/MIN/MAX accumulator
const FUNC_BCD2: u16 = 0x3DD6;      // 4-byte BCD temp for cell value in functions

// Display constants
const CELL_WIDTH: u8 = 9;           // Width per cell display
const VISIBLE_COLS: u8 = 8;         // Columns visible at once
const VISIBLE_ROWS: u8 = 10;        // Rows visible at once

// VT220 screen layout (1-based row numbers)
const TITLE_ROW: u8 = 1;            // Title line
const HELP_ROW: u8 = 2;             // Help/instructions
const HEADER_ROW: u8 = 4;           // Column headers (A B C D...)
const DATA_ROW: u8 = 5;             // First data row
const STATUS_ROW: u8 = 15;          // Status line (after 10 data rows)
const INPUT_ROW: u8 = 16;           // Input prompt row

// Grid size
const GRID_COLS: u8 = 16;           // A-P
const GRID_ROWS: u8 = 64;           // 1-64

// Cell types
const CELL_NUMBER: u8 = 1;
const CELL_FORMULA: u8 = 2;
const CELL_ERROR: u8 = 3;
const CELL_REPEAT: u8 = 4;
const CELL_LABEL: u8 = 5;

/// Spreadsheet code generator - wraps the framework's CodeGen
/// and adds spreadsheet-specific methods
pub struct SpreadsheetCodeGen {
    inner: CodeGen,
}

impl Default for SpreadsheetCodeGen {
    fn default() -> Self {
        Self::new()
    }
}

impl Deref for SpreadsheetCodeGen {
    type Target = CodeGen;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for SpreadsheetCodeGen {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl SpreadsheetCodeGen {
    /// Create a new spreadsheet code generator
    pub fn new() -> Self {
        Self {
            inner: CodeGen::new(),
        }
    }

    /// Generate the complete spreadsheet ROM
    pub fn generate(&mut self) {
        self.emit_spreadsheet_startup();
        self.emit_main_loop();
        self.emit_display();
        self.emit_input();
        self.emit_cell_ops();
        self.emit_bcd_ops();
        self.emit_formula();
        self.emit_io();
        self.emit_strings();
        self.resolve_fixups();
    }

    /// Convert to final ROM bytes
    pub fn into_rom(self) -> Vec<u8> {
        self.inner.rom().to_vec()
    }

    /// Startup code (renamed to avoid conflict with framework method)
    fn emit_spreadsheet_startup(&mut self) {
        // Initialize stack
        self.ld_sp(STACK_TOP);

        // Print welcome banner first
        self.ld_hl_label("welcome_msg");
        self.call("print_string");

        // Clear cursor position
        self.xor_a();
        self.ld_addr_a(CURSOR_COL);
        self.ld_addr_a(CURSOR_ROW);
        self.ld_addr_a(VIEW_TOP);
        self.ld_addr_a(VIEW_LEFT);
        self.ld_addr_a(EDIT_MODE);

        // Initialize column width
        self.ld_a(CELL_WIDTH);
        self.ld_addr_a(COL_WIDTH_VAR);

        // Initialize formula storage pointer
        self.ld_hl(SCRATCH);
        self.ld_addr_hl(FORMULA_PTR);

        // Clear all cells
        self.ld_hl(CELL_DATA);
        self.ld_bc(6144); // 1024 cells × 6 bytes
        self.label("clear_cells_loop");
        self.emit(&[0x36, 0x00]); // LD (HL), 0
        self.inc_hl();
        self.dec_bc();
        self.ld_a_b();
        self.emit(&[0xB1]); // OR C
        self.jp_nz("clear_cells_loop");

        // Initial display
        self.call("refresh_display");
    }

    /// Main loop - handle input and display
    fn emit_main_loop(&mut self) {
        self.label("main_loop");

        // Read a character
        self.call("getchar");

        // Check edit mode - save char in B, check mode, restore to A
        self.ld_b_a();
        self.ld_a_addr(EDIT_MODE);
        self.or_a_a();
        self.ld_a_b();
        self.jp_nz("edit_mode_input");

        // Navigation mode - check for arrow keys and commands
        // Escape sequences start with 0x1B
        self.cp(0x1B);
        self.jp_z("handle_escape");

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

        // '/' to enter command mode
        self.emit(&[0xFE, b'/']);
        self.emit(&[0xCA]); // JP Z, command_mode
        self.fixup("command_mode");

        // '!' to force recalculation
        self.emit(&[0xFE, b'!']);
        self.emit(&[0xCA]); // JP Z, do_recalc
        self.fixup("do_recalc");

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
        self.or_a_a();
        self.emit(&[0xCA]); // JP Z, move_done (already at left edge)
        self.fixup("move_done");
        self.dec_a();
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
        self.inc_a();
        self.emit(&[0x32]); // LD (CURSOR_COL), A
        self.emit_word(CURSOR_COL);
        self.emit(&[0xC3]); // JP move_done
        self.fixup("move_done");

        self.label("move_up");
        self.emit(&[0x3A]); // LD A, (CURSOR_ROW)
        self.emit_word(CURSOR_ROW);
        self.or_a_a();
        self.emit(&[0xCA]); // JP Z, move_done (already at top)
        self.fixup("move_done");
        self.dec_a();
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
        self.inc_a();
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
        // Load current cell content into INPUT_BUF
        self.emit(&[0xCD]); // CALL load_cell_to_input
        self.fixup("load_cell_to_input");
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
        self.ld_hl_ind_a();
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
        self.push_af(); //save digit)
        self.emit(&[0x3E, 0x01]); // LD A, 1
        self.emit(&[0x32]); // LD (EDIT_MODE), A
        self.emit_word(EDIT_MODE);
        self.pop_af(); //restore digit)
        self.emit(&[0x21]); // LD HL, INPUT_BUF
        self.emit_word(INPUT_BUF);
        self.ld_hl_ind_a();
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
        self.push_af();
        self.emit(&[0x3A]); // LD A, (INPUT_LEN)
        self.emit_word(INPUT_LEN);
        self.emit(&[0xFE, 40]); // CP 40 (max input length)
        self.emit(&[0xD2]); // JP NC, edit_input_full
        self.fixup("edit_input_full");
        self.ld_e_a();
        self.emit(&[0x16, 0x00]); // LD D, 0
        self.emit(&[0x21]); // LD HL, INPUT_BUF
        self.emit_word(INPUT_BUF);
        self.add_hl_de();
        self.pop_af();
        self.ld_hl_ind_a();
        self.emit(&[0x3A]); // LD A, (INPUT_LEN)
        self.emit_word(INPUT_LEN);
        self.inc_a();
        self.emit(&[0x32]); // LD (INPUT_LEN), A
        self.emit_word(INPUT_LEN);
        self.emit(&[0xCD]); // CALL show_input_line
        self.fixup("show_input_line");
        self.emit(&[0xC3]); // JP main_loop
        self.fixup("main_loop");

        self.label("edit_input_full");
        self.pop_af(); //discard)
        self.emit(&[0xC3]); // JP main_loop
        self.fixup("main_loop");

        self.label("edit_backspace");
        self.emit(&[0x3A]); // LD A, (INPUT_LEN)
        self.emit_word(INPUT_LEN);
        self.or_a_a();
        self.emit(&[0xCA]); // JP Z, main_loop (nothing to delete)
        self.fixup("main_loop");
        self.dec_a();
        self.emit(&[0x32]); // LD (INPUT_LEN), A
        self.emit_word(INPUT_LEN);
        self.emit(&[0xCD]); // CALL show_input_line
        self.fixup("show_input_line");
        self.emit(&[0xC3]); // JP main_loop
        self.fixup("main_loop");

        self.label("cancel_edit");
        self.xor_a();
        self.emit(&[0x32]); // LD (EDIT_MODE), A
        self.emit_word(EDIT_MODE);
        self.emit(&[0xCD]); // CALL refresh_display
        self.fixup("refresh_display");
        self.emit(&[0xC3]); // JP main_loop
        self.fixup("main_loop");

        self.label("confirm_edit");
        // Null-terminate input buffer
        self.emit(&[0x3A]); // LD A, (INPUT_LEN)
        self.emit_word(INPUT_LEN);
        self.ld_e_a();
        self.emit(&[0x16, 0x00]); // LD D, 0
        self.emit(&[0x21]); // LD HL, INPUT_BUF
        self.emit_word(INPUT_BUF);
        self.add_hl_de();
        self.emit(&[0x36, 0x00]); // LD (HL), 0
        // Parse input and store in cell
        self.emit(&[0xCD]); // CALL parse_and_store
        self.fixup("parse_and_store");
        self.xor_a();
        self.emit(&[0x32]); // LD (EDIT_MODE), A
        self.emit_word(EDIT_MODE);
        self.emit(&[0xCD]); // CALL recalculate
        self.fixup("recalculate");
        self.emit(&[0xCD]); // CALL refresh_display
        self.fixup("refresh_display");
        self.emit(&[0xC3]); // JP main_loop
        self.fixup("main_loop");

        // Command mode - show help and wait for command key
        self.label("command_mode");
        // Show command help on input line
        self.emit(&[0x06, INPUT_ROW]); // LD B, INPUT_ROW
        self.emit(&[0x0E, 1]); // LD C, 1
        self.emit(&[0xCD]); // CALL cursor_pos
        self.fixup("cursor_pos");
        self.emit(&[0xCD]); // CALL clear_to_eol
        self.fixup("clear_to_eol");
        self.emit(&[0x21]); // LD HL, cmd_help_str
        self.fixup("cmd_help_str");
        self.emit(&[0xCD]); // CALL print_string
        self.fixup("print_string");
        // Wait for command key
        self.emit(&[0xCD]); // CALL getchar
        self.fixup("getchar");
        // Check for G/g (goto)
        self.emit(&[0xFE, b'G']);
        self.emit(&[0xCA]); // JP Z, cmd_goto
        self.fixup("cmd_goto");
        self.emit(&[0xFE, b'g']);
        self.emit(&[0xCA]); // JP Z, cmd_goto
        self.fixup("cmd_goto");
        // Check for C/c (clear)
        self.emit(&[0xFE, b'C']);
        self.emit(&[0xCA]); // JP Z, cmd_clear
        self.fixup("cmd_clear");
        self.emit(&[0xFE, b'c']);
        self.emit(&[0xCA]); // JP Z, cmd_clear
        self.fixup("cmd_clear");
        // Check for Q/q (quit)
        self.emit(&[0xFE, b'Q']);
        self.emit(&[0xCA]); // JP Z, quit
        self.fixup("quit");
        self.emit(&[0xFE, b'q']);
        self.emit(&[0xCA]); // JP Z, quit
        self.fixup("quit");
        // Check for - (repeat character)
        self.emit(&[0xFE, b'-']);
        self.emit(&[0xCA]); // JP Z, cmd_repeat
        self.fixup("cmd_repeat");
        // Check for R/r (replicate/copy)
        self.emit(&[0xFE, b'R']);
        self.emit(&[0xCA]); // JP Z, cmd_replicate
        self.fixup("cmd_replicate");
        self.emit(&[0xFE, b'r']);
        self.emit(&[0xCA]); // JP Z, cmd_replicate
        self.fixup("cmd_replicate");
        // Check for W/w (width)
        self.emit(&[0xFE, b'W']);
        self.emit(&[0xCA]); // JP Z, cmd_width
        self.fixup("cmd_width");
        self.emit(&[0xFE, b'w']);
        self.emit(&[0xCA]); // JP Z, cmd_width
        self.fixup("cmd_width");
        // Unknown command - refresh and return
        self.emit(&[0xCD]); // CALL refresh_display
        self.fixup("refresh_display");
        self.emit(&[0xC3]); // JP main_loop
        self.fixup("main_loop");

        // /G - Goto cell
        self.label("cmd_goto");
        // Show goto prompt
        self.emit(&[0x06, INPUT_ROW]); // LD B, INPUT_ROW
        self.emit(&[0x0E, 1]); // LD C, 1
        self.emit(&[0xCD]); // CALL cursor_pos
        self.fixup("cursor_pos");
        self.emit(&[0xCD]); // CALL clear_to_eol
        self.fixup("clear_to_eol");
        self.emit(&[0x21]); // LD HL, goto_prompt
        self.fixup("goto_prompt");
        self.emit(&[0xCD]); // CALL print_string
        self.fixup("print_string");
        self.emit(&[0xCD]); // CALL cursor_show
        self.fixup("cursor_show");
        // Get column letter
        self.emit(&[0xCD]); // CALL getchar
        self.fixup("getchar");
        self.emit(&[0xCD]); // CALL putchar (echo)
        self.fixup("putchar");
        // Convert to uppercase if needed
        self.emit(&[0xFE, b'a']);
        self.emit(&[0xDA]); // JP C, goto_check_col
        self.fixup("goto_check_col");
        self.emit(&[0xFE, b'z' + 1]);
        self.emit(&[0xD2]); // JP NC, goto_check_col
        self.fixup("goto_check_col");
        self.emit(&[0xD6, 0x20]); // SUB 0x20 (to uppercase)
        self.label("goto_check_col");
        // Check if valid column (A-P)
        self.emit(&[0xFE, b'A']);
        self.emit(&[0xDA]); // JP C, goto_cancel (< 'A')
        self.fixup("goto_cancel");
        self.emit(&[0xFE, b'P' + 1]);
        self.emit(&[0xD2]); // JP NC, goto_cancel (> 'P')
        self.fixup("goto_cancel");
        // Save column
        self.emit(&[0xD6, b'A']); // SUB 'A'
        self.emit(&[0x32]); // LD (TEMP1), A
        self.emit_word(TEMP1);
        // Get row number (1 or 2 digits)
        self.emit(&[0xCD]); // CALL getchar
        self.fixup("getchar");
        self.emit(&[0xCD]); // CALL putchar (echo)
        self.fixup("putchar");
        // Check if digit
        self.emit(&[0xFE, b'0']);
        self.emit(&[0xDA]); // JP C, goto_cancel
        self.fixup("goto_cancel");
        self.emit(&[0xFE, b'9' + 1]);
        self.emit(&[0xD2]); // JP NC, goto_cancel
        self.fixup("goto_cancel");
        // First digit
        self.emit(&[0xD6, b'0']); // SUB '0'
        self.emit(&[0x32]); // LD (TEMP1+1), A
        self.emit_word(TEMP1 + 1);
        // Try to get second digit (or Enter)
        self.emit(&[0xCD]); // CALL getchar
        self.fixup("getchar");
        self.emit(&[0xFE, 0x0D]); // CP CR
        self.emit(&[0xCA]); // JP Z, goto_execute
        self.fixup("goto_execute");
        self.emit(&[0xCD]); // CALL putchar (echo)
        self.fixup("putchar");
        // Check if digit
        self.emit(&[0xFE, b'0']);
        self.emit(&[0xDA]); // JP C, goto_cancel
        self.fixup("goto_cancel");
        self.emit(&[0xFE, b'9' + 1]);
        self.emit(&[0xD2]); // JP NC, goto_cancel
        self.fixup("goto_cancel");
        // Second digit: row = first_digit * 10 + second_digit
        self.emit(&[0xD6, b'0']); // SUB '0'
        self.ld_b_a(); //second digit)
        self.emit(&[0x3A]); // LD A, (TEMP1+1) (first digit)
        self.emit_word(TEMP1 + 1);
        // Multiply by 10: A*10 = A*8 + A*2
        self.ld_c_a();
        self.emit(&[0x87]); // ADD A, A (*2)
        self.emit(&[0x87]); // ADD A, A (*4)
        self.emit(&[0x87]); // ADD A, A (*8)
        self.emit(&[0x81]); // ADD A, C (*9 -- wait, should be +C*2)
        // Actually: A*10 = A*2 + A*8
        // Let me redo: save A, A*2, then A*8, add them
        // Simpler: C=A, A=A*2, A=A*2 (now A=4*orig), A=A+C (5*orig), A=A*2 (10*orig)
        // Hmm this is getting complicated. Let me just do it differently.
        self.emit(&[0x81]); // ADD A, C (A = 9*C, close enough... actually wrong)
        // Let me recalculate: after A*8, add C twice: A*8 + C + C = A*8 + 2*C = 10*C
        // But I already did ADD A,C once. Do it again:
        self.emit(&[0x81]); // ADD A, C (now A = 10*C)
        self.emit(&[0x80]); // ADD A, B (add second digit)
        self.emit(&[0x32]); // LD (TEMP1+1), A
        self.emit_word(TEMP1 + 1);
        // Wait for Enter
        self.emit(&[0xCD]); // CALL getchar
        self.fixup("getchar");
        self.label("goto_execute");
        // Set cursor to new position
        self.emit(&[0x3A]); // LD A, (TEMP1)
        self.emit_word(TEMP1);
        self.emit(&[0x32]); // LD (CURSOR_COL), A
        self.emit_word(CURSOR_COL);
        self.emit(&[0x3A]); // LD A, (TEMP1+1)
        self.emit_word(TEMP1 + 1);
        self.dec_a(); //convert 1-based to 0-based)
        // Clamp to valid range (0-63)
        self.emit(&[0xFE, GRID_ROWS]); // CP GRID_ROWS
        self.emit(&[0xDA]); // JP C, goto_row_ok
        self.fixup("goto_row_ok");
        self.emit(&[0x3E, GRID_ROWS - 1]); // LD A, GRID_ROWS-1
        self.label("goto_row_ok");
        self.emit(&[0x32]); // LD (CURSOR_ROW), A
        self.emit_word(CURSOR_ROW);
        self.emit(&[0xCD]); // CALL adjust_view
        self.fixup("adjust_view");
        self.emit(&[0xCD]); // CALL refresh_display
        self.fixup("refresh_display");
        self.emit(&[0xC3]); // JP main_loop
        self.fixup("main_loop");

        self.label("goto_cancel");
        // Invalid input - just refresh and return
        self.emit(&[0xCD]); // CALL refresh_display
        self.fixup("refresh_display");
        self.emit(&[0xC3]); // JP main_loop
        self.fixup("main_loop");

        // /C - Clear current cell
        self.label("cmd_clear");
        // Get cell address and set type to empty (0)
        self.emit(&[0x3A]); // LD A, (CURSOR_COL)
        self.emit_word(CURSOR_COL);
        self.ld_b_a();
        self.emit(&[0x3A]); // LD A, (CURSOR_ROW)
        self.emit_word(CURSOR_ROW);
        self.ld_c_a();
        self.emit(&[0xCD]); // CALL get_cell_addr
        self.fixup("get_cell_addr");
        self.emit(&[0x36, 0x00]); // LD (HL), 0 (CELL_EMPTY)
        self.emit(&[0xCD]); // CALL refresh_display
        self.fixup("refresh_display");
        self.emit(&[0xC3]); // JP main_loop
        self.fixup("main_loop");

        // /- - Repeating character fill
        self.label("cmd_repeat");
        // Show prompt for character
        self.emit(&[0x06, INPUT_ROW]); // LD B, INPUT_ROW
        self.emit(&[0x0E, 1]); // LD C, 1
        self.emit(&[0xCD]); // CALL cursor_pos
        self.fixup("cursor_pos");
        self.emit(&[0xCD]); // CALL clear_to_eol
        self.fixup("clear_to_eol");
        self.emit(&[0x21]); // LD HL, repeat_prompt
        self.fixup("repeat_prompt");
        self.emit(&[0xCD]); // CALL print_string
        self.fixup("print_string");
        self.emit(&[0xCD]); // CALL cursor_show
        self.fixup("cursor_show");
        // Get character to repeat
        self.emit(&[0xCD]); // CALL getchar
        self.fixup("getchar");
        // Store character in TEMP2
        self.emit(&[0x32]); // LD (TEMP2), A
        self.emit_word(TEMP2);
        // Get cell address
        self.emit(&[0x3A]); // LD A, (CURSOR_COL)
        self.emit_word(CURSOR_COL);
        self.ld_b_a();
        self.emit(&[0x3A]); // LD A, (CURSOR_ROW)
        self.emit_word(CURSOR_ROW);
        self.ld_c_a();
        self.emit(&[0xCD]); // CALL get_cell_addr
        self.fixup("get_cell_addr");
        // HL = cell address
        // Set type to CELL_REPEAT
        self.emit(&[0x36, CELL_REPEAT]); // LD (HL), CELL_REPEAT
        self.inc_hl(); //skip flags)
        self.inc_hl(); //point to byte 2)
        // Get char back from TEMP2
        self.emit(&[0x3A]); // LD A, (TEMP2)
        self.emit_word(TEMP2);
        self.ld_hl_ind_a(); //store repeat char)
        self.emit(&[0xCD]); // CALL refresh_display
        self.fixup("refresh_display");
        self.emit(&[0xC3]); // JP main_loop
        self.fixup("main_loop");

        // /R - Replicate/copy current cell to destination
        self.label("cmd_replicate");
        // Show "To cell: " prompt
        self.emit(&[0x06, INPUT_ROW]); // LD B, INPUT_ROW
        self.emit(&[0x0E, 1]); // LD C, 1
        self.emit(&[0xCD]); // CALL cursor_pos
        self.fixup("cursor_pos");
        self.emit(&[0xCD]); // CALL clear_to_eol
        self.fixup("clear_to_eol");
        self.emit(&[0x21]); // LD HL, copy_to_prompt
        self.fixup("copy_to_prompt");
        self.emit(&[0xCD]); // CALL print_string
        self.fixup("print_string");
        self.emit(&[0xCD]); // CALL cursor_show
        self.fixup("cursor_show");

        // Get destination column (A-P)
        self.emit(&[0xCD]); // CALL getchar
        self.fixup("getchar");
        self.emit(&[0xCD]); // CALL putchar (echo)
        self.fixup("putchar");
        // Convert to uppercase if needed
        self.emit(&[0xFE, b'a']);
        self.emit(&[0xDA]); // JP C, repl_col_check (< 'a')
        self.fixup("repl_col_check");
        self.emit(&[0xFE, b'z' + 1]);
        self.emit(&[0xD2]); // JP NC, repl_col_check (> 'z')
        self.fixup("repl_col_check");
        self.emit(&[0xD6, 0x20]); // SUB 0x20 (to uppercase)
        self.label("repl_col_check");
        // Check range A-P
        self.emit(&[0xFE, b'A']);
        self.emit(&[0xDA]); // JP C, repl_cancel (< 'A')
        self.fixup("repl_cancel");
        self.emit(&[0xFE, b'Q']);
        self.emit(&[0xD2]); // JP NC, repl_cancel (> 'P')
        self.fixup("repl_cancel");
        // Convert to column number (0-15)
        self.emit(&[0xD6, b'A']); // SUB 'A'
        self.emit(&[0x32]); // LD (TEMP1), A (dest col)
        self.emit_word(TEMP1);

        // Get destination row (1-64)
        self.emit(&[0x0E, 0x00]); // LD C, 0 (row accumulator)
        self.label("repl_row_loop");
        self.emit(&[0xCD]); // CALL getchar
        self.fixup("getchar");
        self.emit(&[0xFE, 0x0D]); // CP CR
        self.emit(&[0xCA]); // JP Z, repl_row_done
        self.fixup("repl_row_done");
        self.emit(&[0xCD]); // CALL putchar (echo)
        self.fixup("putchar");
        // Check if digit 0-9
        self.emit(&[0xFE, b'0']);
        self.emit(&[0xDA]); // JP C, repl_cancel
        self.fixup("repl_cancel");
        self.emit(&[0xFE, b'9' + 1]);
        self.emit(&[0xD2]); // JP NC, repl_cancel
        self.fixup("repl_cancel");
        // Add to accumulator: C = C * 10 + (A - '0')
        self.emit(&[0xD6, b'0']); // SUB '0'
        self.ld_b_a(); //save digit)
        self.ld_a_c();
        // Multiply by 10: A*10 = A*2 + A*8
        self.emit(&[0x87]); // ADD A, A (x2)
        self.ld_c_a(); //save x2)
        self.emit(&[0x87]); // ADD A, A (x4)
        self.emit(&[0x87]); // ADD A, A (x8)
        self.emit(&[0x81]); // ADD A, C (x10)
        self.emit(&[0x80]); // ADD A, B (add digit)
        self.ld_c_a();
        self.emit(&[0xC3]); // JP repl_row_loop
        self.fixup("repl_row_loop");

        self.label("repl_row_done");
        // C = row (1-based), convert to 0-based
        self.ld_a_c();
        self.or_a_a();
        self.emit(&[0xCA]); // JP Z, repl_cancel (row = 0 invalid)
        self.fixup("repl_cancel");
        self.dec_a();
        self.emit(&[0xFE, GRID_ROWS]); // CP GRID_ROWS
        self.emit(&[0xD2]); // JP NC, repl_cancel (>= 64)
        self.fixup("repl_cancel");
        self.emit(&[0x32]); // LD (TEMP1+1), A (dest row)
        self.emit_word(TEMP1 + 1);

        // Now copy: source = current cell, dest = TEMP1 (col, row)
        // Get source cell address
        self.emit(&[0x3A]); // LD A, (CURSOR_COL)
        self.emit_word(CURSOR_COL);
        self.ld_b_a();
        self.emit(&[0x3A]); // LD A, (CURSOR_ROW)
        self.emit_word(CURSOR_ROW);
        self.ld_c_a();
        self.emit(&[0xCD]); // CALL get_cell_addr
        self.fixup("get_cell_addr");
        self.push_hl(); //source addr)

        // Get dest cell address
        self.emit(&[0x3A]); // LD A, (TEMP1)
        self.emit_word(TEMP1);
        self.ld_b_a();
        self.emit(&[0x3A]); // LD A, (TEMP1+1)
        self.emit_word(TEMP1 + 1);
        self.ld_c_a();
        self.emit(&[0xCD]); // CALL get_cell_addr
        self.fixup("get_cell_addr");
        // HL = dest addr
        self.ex_de_hl(); //DE = dest)
        self.pop_hl(); //HL = source)

        // Copy 4 bytes from HL to DE
        self.emit(&[0x06, 0x04]); // LD B, 4
        self.label("repl_copy_loop");
        self.ld_a_hl_ind();
        self.emit(&[0x12]); // LD (DE), A
        self.inc_hl();
        self.inc_de();
        self.emit(&[0x10]); // DJNZ repl_copy_loop
        let repl_copy_offset = self.rom().len();
        self.emit(&[0x00]); // placeholder
        self.rom_mut()[repl_copy_offset] = (self.get_label("repl_copy_loop").unwrap_or(0)
            .wrapping_sub(self.pos())) as u8;

        // Move cursor to destination cell
        self.emit(&[0x3A]); // LD A, (TEMP1)
        self.emit_word(TEMP1);
        self.emit(&[0x32]); // LD (CURSOR_COL), A
        self.emit_word(CURSOR_COL);
        self.emit(&[0x3A]); // LD A, (TEMP1+1)
        self.emit_word(TEMP1 + 1);
        self.emit(&[0x32]); // LD (CURSOR_ROW), A
        self.emit_word(CURSOR_ROW);

        // Adjust view and refresh
        self.emit(&[0xCD]); // CALL adjust_view
        self.fixup("adjust_view");
        self.emit(&[0xCD]); // CALL refresh_display
        self.fixup("refresh_display");
        self.emit(&[0xC3]); // JP main_loop
        self.fixup("main_loop");

        self.label("repl_cancel");
        self.emit(&[0xCD]); // CALL refresh_display
        self.fixup("refresh_display");
        self.emit(&[0xC3]); // JP main_loop
        self.fixup("main_loop");

        // /W - Set column width
        self.label("cmd_width");
        // Show width prompt
        self.emit(&[0x06, INPUT_ROW]); // LD B, INPUT_ROW
        self.emit(&[0x0E, 1]); // LD C, 1
        self.emit(&[0xCD]); // CALL cursor_pos
        self.fixup("cursor_pos");
        self.emit(&[0xCD]); // CALL clear_to_eol
        self.fixup("clear_to_eol");
        self.emit(&[0x21]); // LD HL, width_prompt
        self.fixup("width_prompt");
        self.emit(&[0xCD]); // CALL print_string
        self.fixup("print_string");
        self.emit(&[0xCD]); // CALL cursor_show
        self.fixup("cursor_show");

        // Get width (1-2 digits)
        self.emit(&[0x0E, 0x00]); // LD C, 0 (accumulator)
        self.label("width_digit_loop");
        self.emit(&[0xCD]); // CALL getchar
        self.fixup("getchar");
        self.emit(&[0xFE, 0x0D]); // CP CR
        self.emit(&[0xCA]); // JP Z, width_done
        self.fixup("width_done");
        self.emit(&[0xCD]); // CALL putchar (echo)
        self.fixup("putchar");
        // Check if digit
        self.emit(&[0xFE, b'0']);
        self.emit(&[0xDA]); // JP C, width_cancel
        self.fixup("width_cancel");
        self.emit(&[0xFE, b'9' + 1]);
        self.emit(&[0xD2]); // JP NC, width_cancel
        self.fixup("width_cancel");
        // C = C * 10 + digit
        self.emit(&[0xD6, b'0']); // SUB '0'
        self.ld_b_a();
        self.ld_a_c();
        self.emit(&[0x87]); // ADD A, A (x2)
        self.ld_c_a();
        self.emit(&[0x87]); // ADD A, A (x4)
        self.emit(&[0x87]); // ADD A, A (x8)
        self.emit(&[0x81]); // ADD A, C (x10)
        self.emit(&[0x80]); // ADD A, B
        self.ld_c_a();
        self.emit(&[0xC3]); // JP width_digit_loop
        self.fixup("width_digit_loop");

        self.label("width_done");
        // Validate width: 5-15
        self.ld_a_c();
        self.emit(&[0xFE, 5]); // CP 5
        self.emit(&[0xDA]); // JP C, width_cancel (< 5)
        self.fixup("width_cancel");
        self.emit(&[0xFE, 16]); // CP 16
        self.emit(&[0xD2]); // JP NC, width_cancel (>= 16)
        self.fixup("width_cancel");
        // Store new width
        self.emit(&[0x32]); // LD (COL_WIDTH_VAR), A
        self.emit_word(COL_WIDTH_VAR);
        self.emit(&[0xCD]); // CALL refresh_display
        self.fixup("refresh_display");
        self.emit(&[0xC3]); // JP main_loop
        self.fixup("main_loop");

        self.label("width_cancel");
        self.emit(&[0xCD]); // CALL refresh_display
        self.fixup("refresh_display");
        self.emit(&[0xC3]); // JP main_loop
        self.fixup("main_loop");

        // Recalculate all formulas
        self.label("do_recalc");
        // Loop through all 1024 cells (16 cols x 64 rows)
        self.emit(&[0x21]); // LD HL, CELL_DATA
        self.emit_word(CELL_DATA);
        self.emit(&[0x11, 0x00, 0x04]); // LD DE, 1024 (cell count)

        self.label("recalc_loop");
        self.push_hl(); //save cell pointer)
        self.push_de(); //save counter)

        // Check if this cell is a formula (type = 2)
        self.ld_a_hl_ind();
        self.emit(&[0xFE, 0x02]); // CP 2 (CELL_FORMULA)
        self.emit(&[0xC2]); // JP NZ, recalc_next
        self.fixup("recalc_next");

        // It's a formula - get pointer from bytes 2-3
        self.inc_hl();
        self.inc_hl();
        self.emit(&[0x5E]); // LD E, (HL)
        self.inc_hl();
        self.emit(&[0x56]); // LD D, (HL)
        // DE = formula pointer, save HL (points to high byte of pointer)
        self.push_hl();

        // Copy formula pointer to TEMP2 for later
        self.ex_de_hl(); //HL = formula string)
        self.push_hl(); //save formula pointer)

        // Skip the '=' and evaluate the expression
        self.inc_hl(); //skip '=')
        self.emit(&[0xCD]); // CALL eval_expr
        self.fixup("eval_expr");
        // HL = result

        // Get formula pointer back
        self.pop_de(); //DE = formula string pointer)
        // Find end of string (null terminator)
        self.label("recalc_find_end");
        self.emit(&[0x1A]); // LD A, (DE)
        self.inc_de();
        self.or_a_a();
        self.emit(&[0xC2]); // JP NZ, recalc_find_end
        self.fixup("recalc_find_end");
        // DE now points to value storage location
        // Store new BCD value (4 bytes from BCD_TEMP1) at (DE)
        self.ex_de_hl(); //HL = storage ptr)
        self.emit(&[0x11]); // LD DE, BCD_TEMP1
        self.emit_word(BCD_TEMP1);
        self.emit(&[0x06, 4]); // LD B, 4
        self.label("recalc_store_loop");
        self.emit(&[0x1A]); // LD A, (DE)
        self.emit(&[0x77]); // LD (HL), A
        self.inc_hl();
        self.inc_de();
        self.emit(&[0x10]); // DJNZ recalc_store_loop
        self.emit_relative("recalc_store_loop");

        // Restore cell pointer high byte position
        self.pop_hl();

        self.label("recalc_next");
        self.pop_de(); //restore counter)
        self.pop_hl(); //restore cell pointer)
        // Move to next cell (6 bytes)
        self.inc_hl();
        self.inc_hl();
        self.inc_hl();
        self.inc_hl();
        self.inc_hl();
        self.inc_hl();
        // Decrement counter
        self.emit(&[0x1B]); // DEC DE
        self.ld_a_d();
        self.emit(&[0xB3]); // OR E
        self.emit(&[0xC2]); // JP NZ, recalc_loop
        self.fixup("recalc_loop");

        // Refresh display and return to main loop
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
        self.halt();
    }

    /// Display routines
    fn emit_display(&mut self) {
        // Adjust view to keep cursor visible
        self.label("adjust_view");
        // Check if cursor is above view
        self.emit(&[0x3A]); // LD A, (CURSOR_ROW)
        self.emit_word(CURSOR_ROW);
        self.ld_b_a();
        self.emit(&[0x3A]); // LD A, (VIEW_TOP)
        self.emit_word(VIEW_TOP);
        self.emit(&[0xB8]); // CP B
        self.emit(&[0xDA]); // JP C, adjust_check_bottom
        self.fixup("adjust_check_bottom");
        self.emit(&[0xCA]); // JP Z, adjust_check_bottom
        self.fixup("adjust_check_bottom");
        // Cursor above view - set VIEW_TOP = CURSOR_ROW
        self.ld_a_b();
        self.emit(&[0x32]); // LD (VIEW_TOP), A
        self.emit_word(VIEW_TOP);

        self.label("adjust_check_bottom");
        // Check if cursor is below view
        self.emit(&[0x3A]); // LD A, (CURSOR_ROW)
        self.emit_word(CURSOR_ROW);
        self.ld_b_a();
        self.emit(&[0x3A]); // LD A, (VIEW_TOP)
        self.emit_word(VIEW_TOP);
        self.emit(&[0xC6, VISIBLE_ROWS - 1]); // ADD A, VISIBLE_ROWS-1
        self.emit(&[0xB8]); // CP B
        self.emit(&[0xD2]); // JP NC, adjust_check_left
        self.fixup("adjust_check_left");
        // Cursor below view - set VIEW_TOP = CURSOR_ROW - VISIBLE_ROWS + 1
        self.ld_a_b();
        self.emit(&[0xD6, VISIBLE_ROWS - 1]); // SUB VISIBLE_ROWS-1
        self.emit(&[0x32]); // LD (VIEW_TOP), A
        self.emit_word(VIEW_TOP);

        self.label("adjust_check_left");
        // Similar logic for columns
        self.emit(&[0x3A]); // LD A, (CURSOR_COL)
        self.emit_word(CURSOR_COL);
        self.ld_b_a();
        self.emit(&[0x3A]); // LD A, (VIEW_LEFT)
        self.emit_word(VIEW_LEFT);
        self.emit(&[0xB8]); // CP B
        self.emit(&[0xDA]); // JP C, adjust_check_right
        self.fixup("adjust_check_right");
        self.emit(&[0xCA]); // JP Z, adjust_check_right
        self.fixup("adjust_check_right");
        self.ld_a_b();
        self.emit(&[0x32]); // LD (VIEW_LEFT), A
        self.emit_word(VIEW_LEFT);

        self.label("adjust_check_right");
        self.emit(&[0x3A]); // LD A, (CURSOR_COL)
        self.emit_word(CURSOR_COL);
        self.ld_b_a();
        self.emit(&[0x3A]); // LD A, (VIEW_LEFT)
        self.emit_word(VIEW_LEFT);
        self.emit(&[0xC6, VISIBLE_COLS - 1]); // ADD A, VISIBLE_COLS-1
        self.emit(&[0xB8]); // CP B
        self.emit(&[0xD2]); // JP NC, adjust_done
        self.fixup("adjust_done");
        self.ld_a_b();
        self.emit(&[0xD6, VISIBLE_COLS - 1]); // SUB VISIBLE_COLS-1
        self.emit(&[0x32]); // LD (VIEW_LEFT), A
        self.emit_word(VIEW_LEFT);

        self.label("adjust_done");
        self.ret();

        // Refresh the entire display
        self.label("refresh_display");
        // Clear screen (also homes cursor)
        self.emit(&[0xCD]); // CALL clear_screen
        self.fixup("clear_screen");
        // Hide cursor during refresh
        self.emit(&[0xCD]); // CALL cursor_hide
        self.fixup("cursor_hide");

        // Print title line at row 1
        self.emit(&[0x06, TITLE_ROW]); // LD B, TITLE_ROW
        self.emit(&[0x0E, 1]); // LD C, 1
        self.emit(&[0xCD]); // CALL cursor_pos
        self.fixup("cursor_pos");
        self.emit(&[0x21]); // LD HL, title_str
        self.fixup("title_str");
        self.emit(&[0xCD]); // CALL print_string
        self.fixup("print_string");

        // Print help line at row 2
        self.emit(&[0x06, HELP_ROW]); // LD B, HELP_ROW
        self.emit(&[0x0E, 1]); // LD C, 1
        self.emit(&[0xCD]); // CALL cursor_pos
        self.fixup("cursor_pos");
        self.emit(&[0x21]); // LD HL, help_str
        self.fixup("help_str");
        self.emit(&[0xCD]); // CALL print_string
        self.fixup("print_string");

        // Position at header row and print column headers
        self.emit(&[0x06, HEADER_ROW]); // LD B, HEADER_ROW
        self.emit(&[0x0E, 1]); // LD C, 1
        self.emit(&[0xCD]); // CALL cursor_pos
        self.fixup("cursor_pos");

        // Print header row (column letters)
        // 5 spaces: 4 for row number area + 1 for cell marker
        self.emit(&[0x3E, b' ']); // LD A, ' '
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");

        // Print column headers
        self.emit(&[0x3A]); // LD A, (VIEW_LEFT)
        self.emit_word(VIEW_LEFT);
        self.ld_b_a(); //B = current column)
        self.emit(&[0x0E, VISIBLE_COLS]); // LD C, VISIBLE_COLS (counter)

        self.label("header_col_loop");
        self.ld_a_b();
        self.emit(&[0xFE, GRID_COLS]); // CP GRID_COLS
        self.emit(&[0xD2]); // JP NC, header_done
        self.fixup("header_done");
        self.emit(&[0xC6, b'A']); // ADD A, 'A'
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        // Pad with spaces
        self.emit(&[0x3E, b' ']); // LD A, ' '
        self.push_hl();
        self.emit(&[0x26, CELL_WIDTH - 1]); // LD H, CELL_WIDTH-1
        self.label("header_pad_loop");
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0x25]); // DEC H
        self.emit(&[0xC2]); // JP NZ, header_pad_loop
        self.fixup("header_pad_loop");
        self.pop_hl();
        self.inc_b();
        self.dec_c();
        self.emit(&[0xC2]); // JP NZ, header_col_loop
        self.fixup("header_col_loop");

        self.label("header_done");
        // No newline needed - we'll position cursor for each row

        // Print each row
        self.emit(&[0x3A]); // LD A, (VIEW_TOP)
        self.emit_word(VIEW_TOP);
        self.emit(&[0x32]); // LD (TEMP1), A (current row in grid)
        self.emit_word(TEMP1);
        self.emit(&[0x3E, 0]); // LD A, 0
        self.emit(&[0x32]); // LD (TEMP1+1), A (screen row offset, 0-9)
        self.emit_word(TEMP1 + 1);

        self.label("display_row_loop");
        self.emit(&[0x3A]); // LD A, (TEMP1)
        self.emit_word(TEMP1);
        self.emit(&[0xFE, GRID_ROWS]); // CP GRID_ROWS
        self.emit(&[0xD2]); // JP NC, display_done
        self.fixup("display_done");
        // Check if we've done all visible rows
        self.emit(&[0x3A]); // LD A, (TEMP1+1)
        self.emit_word(TEMP1 + 1);
        self.emit(&[0xFE, VISIBLE_ROWS]); // CP VISIBLE_ROWS
        self.emit(&[0xD2]); // JP NC, display_done
        self.fixup("display_done");

        // Position cursor at start of this row: DATA_ROW + screen_row_offset
        self.emit(&[0x3A]); // LD A, (TEMP1+1)
        self.emit_word(TEMP1 + 1);
        self.emit(&[0xC6, DATA_ROW]); // ADD A, DATA_ROW
        self.ld_b_a(); //row)
        self.emit(&[0x0E, 1]); // LD C, 1 (col)
        self.emit(&[0xCD]); // CALL cursor_pos
        self.fixup("cursor_pos");

        // Print row number (1-based, right-aligned in 4 chars)
        self.emit(&[0x3A]); // LD A, (TEMP1)
        self.emit_word(TEMP1);
        self.inc_a(); //1-based)
        self.emit(&[0x6F]); // LD L, A
        self.emit(&[0x26, 0x00]); // LD H, 0
        self.emit(&[0xCD]); // CALL print_int_padded
        self.fixup("print_int_padded");

        // Print cells in this row
        self.emit(&[0x3A]); // LD A, (VIEW_LEFT)
        self.emit_word(VIEW_LEFT);
        self.ld_b_a(); //B = current col)
        self.emit(&[0x0E, VISIBLE_COLS]); // LD C, VISIBLE_COLS

        self.label("display_cell_loop");
        self.ld_a_b();
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
        self.push_hl();
        self.emit(&[0x2A]); // LD HL, (TEMP1)
        self.emit_word(TEMP1);
        self.emit(&[0xBD]); // CP L
        self.pop_hl();
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
        self.push_bc();
        self.ld_a_b(); //col)
        self.ld_b_a();
        self.emit(&[0x3A]); // LD A, (TEMP1) (row)
        self.emit_word(TEMP1);
        self.ld_c_a();
        self.emit(&[0xCD]); // CALL get_cell_addr
        self.fixup("get_cell_addr");
        self.emit(&[0xCD]); // CALL print_cell
        self.fixup("print_cell");
        self.pop_bc();

        // Check if cursor cell for closing bracket
        self.emit(&[0x3A]); // LD A, (CURSOR_COL)
        self.emit_word(CURSOR_COL);
        self.emit(&[0xB8]); // CP B
        self.emit(&[0xC2]); // JP NZ, cell_no_bracket
        self.fixup("cell_no_bracket");
        self.emit(&[0x3A]); // LD A, (CURSOR_ROW)
        self.emit_word(CURSOR_ROW);
        self.push_hl();
        self.emit(&[0x2A]); // LD HL, (TEMP1)
        self.emit_word(TEMP1);
        self.emit(&[0xBD]); // CP L
        self.pop_hl();
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
        self.inc_b();
        self.dec_c();
        self.emit(&[0xC2]); // JP NZ, display_cell_loop
        self.fixup("display_cell_loop");

        self.label("display_row_end");
        // Increment grid row (TEMP1)
        self.emit(&[0x3A]); // LD A, (TEMP1)
        self.emit_word(TEMP1);
        self.inc_a();
        self.emit(&[0x32]); // LD (TEMP1), A
        self.emit_word(TEMP1);
        // Increment screen row offset (TEMP1+1)
        self.emit(&[0x3A]); // LD A, (TEMP1+1)
        self.emit_word(TEMP1 + 1);
        self.inc_a();
        self.emit(&[0x32]); // LD (TEMP1+1), A
        self.emit_word(TEMP1 + 1);
        self.emit(&[0xC3]); // JP display_row_loop (always loop, check at top)
        self.fixup("display_row_loop");

        self.label("display_done");
        // Position cursor at status row
        self.emit(&[0x06, STATUS_ROW]); // LD B, STATUS_ROW
        self.emit(&[0x0E, 1]); // LD C, 1
        self.emit(&[0xCD]); // CALL cursor_pos
        self.fixup("cursor_pos");
        // Print status line
        self.emit(&[0xCD]); // CALL print_status
        self.fixup("print_status");
        // Show cursor again
        self.emit(&[0xCD]); // CALL cursor_show
        self.fixup("cursor_show");
        self.ret();

        // Print a cell's value (HL = cell address)
        // Prints value right-aligned in CELL_WIDTH-2 chars
        self.label("print_cell");
        self.ld_a_hl_ind(); // cell type
        self.or_a_a();
        self.emit(&[0xCA]); // JP Z, print_cell_empty
        self.fixup("print_cell_empty");
        self.emit(&[0xFE, CELL_NUMBER]); // CP CELL_NUMBER
        self.emit(&[0xCA]); // JP Z, print_cell_number
        self.fixup("print_cell_number");
        self.emit(&[0xFE, CELL_ERROR]); // CP CELL_ERROR
        self.emit(&[0xCA]); // JP Z, print_cell_error
        self.fixup("print_cell_error");
        self.emit(&[0xFE, CELL_REPEAT]); // CP CELL_REPEAT
        self.emit(&[0xCA]); // JP Z, print_cell_repeat
        self.fixup("print_cell_repeat");
        self.emit(&[0xFE, CELL_LABEL]); // CP CELL_LABEL
        self.emit(&[0xCA]); // JP Z, print_cell_label
        self.fixup("print_cell_label");
        // Formula - get value from formula storage
        self.emit(&[0xC3]); // JP print_cell_formula
        self.fixup("print_cell_formula");

        self.label("print_cell_empty");
        // Print spaces
        self.emit(&[0x06, CELL_WIDTH - 2]); // LD B, CELL_WIDTH-2
        self.emit(&[0x3E, b' ']); // LD A, ' '
        self.label("print_empty_loop");
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0x10]); // DJNZ print_empty_loop
        let offset = self.rom().len();
        self.emit(&[0x00]); // placeholder for relative jump
        self.rom_mut()[offset] = (self.get_label("print_empty_loop").unwrap_or(0)
            .wrapping_sub(self.pos())) as u8;
        self.ret();

        self.label("print_cell_number");
        // Cell format: byte 0 = type, byte 1 = sign, bytes 2-5 = BCD
        self.inc_hl();
        self.emit(&[0x4E]); // LD C, (HL) (save sign)
        self.inc_hl();
        // Copy 4 BCD bytes to BCD_TEMP1
        self.push_bc(); // save sign
        self.emit(&[0x11]); // LD DE, BCD_TEMP1
        self.emit_word(BCD_TEMP1);
        self.emit(&[0x06, 4]); // LD B, 4
        self.label("load_bcd_loop");
        self.ld_a_hl_ind();
        self.emit(&[0x12]); // LD (DE), A
        self.inc_hl();
        self.inc_de();
        self.emit(&[0x10]); // DJNZ load_bcd_loop
        self.emit_relative("load_bcd_loop");
        // Convert BCD to ASCII
        self.emit(&[0xCD]); // CALL bcd_to_ascii
        self.fixup("bcd_to_ascii");
        // Print with sign and padding
        self.pop_bc(); // restore sign in C
        self.emit(&[0xCD]); // CALL print_bcd_cell_signed
        self.fixup("print_bcd_cell_signed");
        self.ret();

        self.label("print_cell_error");
        self.emit(&[0x21]); // LD HL, error_str
        self.fixup("error_str");
        self.emit(&[0xCD]); // CALL print_string
        self.fixup("print_string");
        self.ret();

        // Formula cell - get pointer and read sign + BCD value
        self.label("print_cell_formula");
        // HL points to cell, bytes 2-3 have formula pointer
        self.inc_hl();
        self.inc_hl();
        self.emit(&[0x5E]); // LD E, (HL)
        self.inc_hl();
        self.emit(&[0x56]); // LD D, (HL)
        // DE = formula pointer, scan to end of string to find value
        self.ex_de_hl(); //HL = formula pointer)
        self.label("find_formula_value");
        self.ld_a_hl_ind();
        self.inc_hl();
        self.or_a_a();
        self.emit(&[0xC2]); // JP NZ, find_formula_value
        self.fixup("find_formula_value");
        // HL now points to sign byte, then 4 BCD bytes
        self.ld_a_hl_ind(); // load sign
        self.ld_c_a(); // save sign in C
        self.inc_hl(); // point to BCD
        // Copy BCD to BCD_TEMP1
        self.push_bc(); // save sign
        self.emit(&[0x11]); // LD DE, BCD_TEMP1
        self.emit_word(BCD_TEMP1);
        self.emit(&[0x06, 4]); // LD B, 4
        self.label("load_formula_bcd");
        self.ld_a_hl_ind();
        self.emit(&[0x12]); // LD (DE), A
        self.inc_hl();
        self.inc_de();
        self.emit(&[0x10]); // DJNZ
        self.emit_relative("load_formula_bcd");
        // Convert to ASCII and print with sign
        self.emit(&[0xCD]); // CALL bcd_to_ascii
        self.fixup("bcd_to_ascii");
        self.pop_bc(); // restore sign in C
        self.emit(&[0xCD]); // CALL print_bcd_cell_signed
        self.fixup("print_bcd_cell_signed");
        self.ret();

        // Print repeating character cell
        self.label("print_cell_repeat");
        // HL points to cell, byte 2 has repeat character
        self.inc_hl(); //skip type)
        self.inc_hl(); //point to char)
        self.emit(&[0x4E]); // LD C, (HL) - get repeat char into C
        self.emit(&[0x06, CELL_WIDTH - 2]); // LD B, CELL_WIDTH-2
        self.label("print_repeat_loop");
        self.ld_a_c(); //restore char from C)
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0x10]); // DJNZ print_repeat_loop
        let repeat_offset = self.rom().len();
        self.emit(&[0x00]); // placeholder
        self.rom_mut()[repeat_offset] = (self.get_label("print_repeat_loop").unwrap_or(0)
            .wrapping_sub(self.pos())) as u8;
        self.ret();

        // Print label cell (left-aligned string)
        self.label("print_cell_label");
        // HL points to cell, bytes 2-3 have string pointer
        self.inc_hl();
        self.inc_hl();
        self.emit(&[0x5E]); // LD E, (HL)
        self.inc_hl();
        self.emit(&[0x56]); // LD D, (HL)
        self.ex_de_hl(); //HL = string pointer)
        // Skip the leading " character
        self.inc_hl();
        // Print up to CELL_WIDTH-2 characters, then pad with spaces
        self.emit(&[0x06, CELL_WIDTH - 2]); // LD B, CELL_WIDTH-2 (max chars)
        self.label("print_label_loop");
        self.ld_a_hl_ind();
        self.or_a_a(); //check for null)
        self.emit(&[0xCA]); // JP Z, print_label_pad
        self.fixup("print_label_pad");
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.inc_hl();
        self.emit(&[0x10]); // DJNZ print_label_loop
        let label_offset = self.rom().len();
        self.emit(&[0x00]); // placeholder
        self.rom_mut()[label_offset] = (self.get_label("print_label_loop").unwrap_or(0)
            .wrapping_sub(self.pos())) as u8;
        self.ret(); //printed all CELL_WIDTH-2 chars)
        // Pad remaining with spaces
        self.label("print_label_pad");
        self.ld_a_b(); //remaining count)
        self.or_a_a();
        self.ret_z(); //no padding needed)
        self.emit(&[0x3E, b' ']); // LD A, ' '
        self.label("print_label_pad_loop");
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0x10]); // DJNZ print_label_pad_loop
        let pad_offset = self.rom().len();
        self.emit(&[0x00]); // placeholder
        self.rom_mut()[pad_offset] = (self.get_label("print_label_pad_loop").unwrap_or(0)
            .wrapping_sub(self.pos())) as u8;
        self.ret();

        // Print status line showing current cell
        self.label("print_status");
        self.emit(&[0x3A]); // LD A, (CURSOR_COL)
        self.emit_word(CURSOR_COL);
        self.emit(&[0xC6, b'A']); // ADD A, 'A'
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0x3A]); // LD A, (CURSOR_ROW)
        self.emit_word(CURSOR_ROW);
        self.inc_a(); //1-based)
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
        self.ld_b_a();
        self.emit(&[0x3A]); // LD A, (CURSOR_ROW)
        self.emit_word(CURSOR_ROW);
        self.ld_c_a();
        self.emit(&[0xCD]); // CALL get_cell_addr
        self.fixup("get_cell_addr");
        self.emit(&[0xCD]); // CALL print_cell_content
        self.fixup("print_cell_content");
        self.ret();

        // Print cell content (raw value or formula)
        self.label("print_cell_content");
        self.ld_a_hl_ind(); // type
        self.or_a_a();
        self.ret_z(); //empty)
        self.emit(&[0xFE, CELL_NUMBER]); // CP CELL_NUMBER
        self.emit(&[0xC2]); // JP NZ, print_content_formula
        self.fixup("print_content_formula");
        // Number - print BCD value with sign
        self.inc_hl(); // skip type
        self.emit(&[0x4E]); // LD C, (HL) (save sign)
        self.inc_hl();
        // Copy 4 BCD bytes to BCD_TEMP1
        self.push_bc(); // save sign
        self.emit(&[0x11]); // LD DE, BCD_TEMP1
        self.emit_word(BCD_TEMP1);
        self.emit(&[0x06, 4]); // LD B, 4
        self.label("load_status_bcd");
        self.ld_a_hl_ind();
        self.emit(&[0x12]); // LD (DE), A
        self.inc_hl();
        self.inc_de();
        self.emit(&[0x10]); // DJNZ
        self.emit_relative("load_status_bcd");
        // Convert to ASCII
        self.emit(&[0xCD]); // CALL bcd_to_ascii
        self.fixup("bcd_to_ascii");
        // Check sign and print minus if negative
        self.pop_bc(); // restore sign in C
        self.ld_a_c();
        self.or_a_a();
        self.emit(&[0xCA]); // JP Z, status_skip_zeros (positive)
        self.fixup("status_skip_zeros");
        // Negative - print minus sign first
        self.emit(&[0x3E, b'-']); // LD A, '-'
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        // Print INPUT_BUF, skipping leading zeros
        self.label("status_skip_zeros");
        self.emit(&[0x21]); // LD HL, INPUT_BUF
        self.emit_word(INPUT_BUF);
        self.emit(&[0x06, 7]); // LD B, 7 (skip up to 7 leading zeros)
        self.label("status_skip_zeros_loop");
        self.ld_a_hl_ind();
        self.emit(&[0xFE, b'0']); // CP '0'
        self.emit(&[0xC2]); // JP NZ, status_print_num
        self.fixup("status_print_num");
        self.inc_hl();
        self.emit(&[0x10]); // DJNZ status_skip_zeros_loop
        self.emit_relative("status_skip_zeros_loop");
        self.label("status_print_num");
        self.emit(&[0xCD]); // CALL print_string
        self.fixup("print_string");
        self.ret();

        self.label("print_content_formula");
        // Print the formula text (stored at formula pointer)
        self.inc_hl();
        self.inc_hl();
        self.emit(&[0x5E]); // LD E, (HL)
        self.inc_hl();
        self.emit(&[0x56]); // LD D, (HL)
        self.ex_de_hl(); //HL = formula pointer)
        self.emit(&[0xCD]); // CALL print_string
        self.fixup("print_string");
        self.ret();

        // Show input line when editing
        self.label("show_input_line");
        // Position cursor at input row
        self.emit(&[0x06, INPUT_ROW]); // LD B, INPUT_ROW
        self.emit(&[0x0E, 1]); // LD C, 1
        self.emit(&[0xCD]); // CALL cursor_pos
        self.fixup("cursor_pos");
        // Print prompt
        self.emit(&[0x3E, b'>']); // LD A, '>'
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0x3E, b' ']); // LD A, ' '
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        // Print input buffer
        self.emit(&[0x21]); // LD HL, INPUT_BUF
        self.emit_word(INPUT_BUF);
        self.emit(&[0x3A]); // LD A, (INPUT_LEN)
        self.emit_word(INPUT_LEN);
        self.ld_b_a();
        self.or_a_a();
        self.emit(&[0xCA]); // JP Z, show_input_done
        self.fixup("show_input_done");
        self.label("show_input_loop");
        self.ld_a_hl_ind();
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.inc_hl();
        self.emit(&[0x10]); // DJNZ
        let offset = self.rom().len();
        self.emit(&[0x00]); // placeholder
        // Calculate relative offset for DJNZ
        let target = self.get_label("show_input_loop").unwrap_or(0);
        let current = self.pos();
        self.rom_mut()[offset] = target.wrapping_sub(current) as u8;
        self.label("show_input_done");
        // Clear to end of line (removes old chars when backspacing)
        self.emit(&[0xCD]); // CALL clear_to_eol
        self.fixup("clear_to_eol");
        self.ret();
    }

    /// Input handling
    fn emit_input(&mut self) {
        // Parse input buffer and store in current cell
        self.label("parse_and_store");
        self.emit(&[0x3A]); // LD A, (INPUT_LEN)
        self.emit_word(INPUT_LEN);
        self.or_a_a();
        self.ret_z(); //empty input)

        // Check if formula (starts with '=')
        self.emit(&[0x21]); // LD HL, INPUT_BUF
        self.emit_word(INPUT_BUF);
        self.ld_a_hl_ind();
        self.emit(&[0xFE, b'=']);
        self.emit(&[0xCA]); // JP Z, parse_formula
        self.fixup("parse_formula");

        // Check if label (starts with '"')
        self.emit(&[0xFE, b'"']);
        self.emit(&[0xCA]); // JP Z, parse_label
        self.fixup("parse_label");

        // Otherwise parse as number
        self.emit(&[0xCD]); // CALL parse_number
        self.fixup("parse_number");
        // C = sign, BCD value in BCD_TEMP1, carry set if error
        self.emit(&[0xDA]); // JP C, store_error
        self.fixup("store_error");
        // Store as number in current cell (6 bytes: type, sign, 4 BCD bytes)
        self.push_bc(); // save sign in C
        self.emit(&[0x3A]); // LD A, (CURSOR_COL)
        self.emit_word(CURSOR_COL);
        self.ld_b_a();
        self.emit(&[0x3A]); // LD A, (CURSOR_ROW)
        self.emit_word(CURSOR_ROW);
        self.ld_c_a();
        self.emit(&[0xCD]); // CALL get_cell_addr
        self.fixup("get_cell_addr");
        self.emit(&[0x36, CELL_NUMBER]); // LD (HL), CELL_NUMBER (byte 0: type)
        self.inc_hl();
        self.pop_bc(); // restore sign
        self.emit(&[0x71]); // LD (HL), C (byte 1: sign)
        self.inc_hl();
        // Copy 4 BCD bytes from BCD_TEMP1 to cell
        self.emit(&[0x11]); // LD DE, BCD_TEMP1
        self.emit_word(BCD_TEMP1);
        self.emit(&[0x06, 4]); // LD B, 4
        self.label("store_num_loop");
        self.emit(&[0x1A]); // LD A, (DE)
        self.emit(&[0x77]); // LD (HL), A
        self.inc_hl();
        self.inc_de();
        self.emit(&[0x10]); // DJNZ store_num_loop
        self.emit_relative("store_num_loop");
        self.ret();

        self.label("store_error");
        self.emit(&[0x3A]); // LD A, (CURSOR_COL)
        self.emit_word(CURSOR_COL);
        self.ld_b_a();
        self.emit(&[0x3A]); // LD A, (CURSOR_ROW)
        self.emit_word(CURSOR_ROW);
        self.ld_c_a();
        self.emit(&[0xCD]); // CALL get_cell_addr
        self.fixup("get_cell_addr");
        self.emit(&[0x36, CELL_ERROR]); // LD (HL), CELL_ERROR
        self.ret();

        // Parse and store label (starts with ")
        self.label("parse_label");
        // Copy label text to SCRATCH storage area (reuse formula storage)
        // Get storage pointer
        self.emit(&[0x2A]); // LD HL, (FORMULA_PTR)
        self.emit_word(FORMULA_PTR);
        self.push_hl(); //save label pointer for cell)
        // Copy input buffer to storage
        self.emit(&[0x11]); // LD DE, INPUT_BUF
        self.emit_word(INPUT_BUF);
        self.emit(&[0x3A]); // LD A, (INPUT_LEN)
        self.emit_word(INPUT_LEN);
        self.ld_b_a(); //loop count)
        self.label("copy_label_loop");
        self.emit(&[0x1A]); // LD A, (DE)
        self.ld_hl_ind_a();
        self.inc_de();
        self.inc_hl();
        self.emit(&[0x10]); // DJNZ copy_label_loop
        let copy_label_offset = self.rom().len();
        self.emit(&[0x00]); // placeholder
        self.rom_mut()[copy_label_offset] = (self.get_label("copy_label_loop").unwrap_or(0)
            .wrapping_sub(self.pos())) as u8;
        // Add null terminator
        self.emit(&[0x36, 0x00]); // LD (HL), 0
        self.inc_hl();
        // Update FORMULA_PTR
        self.emit(&[0x22]); // LD (FORMULA_PTR), HL
        self.emit_word(FORMULA_PTR);
        // Get cell address
        self.emit(&[0x3A]); // LD A, (CURSOR_COL)
        self.emit_word(CURSOR_COL);
        self.ld_b_a();
        self.emit(&[0x3A]); // LD A, (CURSOR_ROW)
        self.emit_word(CURSOR_ROW);
        self.ld_c_a();
        self.emit(&[0xCD]); // CALL get_cell_addr
        self.fixup("get_cell_addr");
        // Store CELL_LABEL type and pointer
        self.emit(&[0x36, CELL_LABEL]); // LD (HL), CELL_LABEL
        self.inc_hl();
        self.emit(&[0x36, 0x00]); // LD (HL), 0 (flags)
        self.inc_hl();
        // Store label pointer from stack
        self.pop_de(); //label pointer)
        self.emit(&[0x73]); // LD (HL), E
        self.inc_hl();
        self.emit(&[0x72]); // LD (HL), D
        self.ret();

        // Load current cell content into INPUT_BUF
        // Sets INPUT_LEN and INPUT_POS appropriately
        self.label("load_cell_to_input");
        // Get current cell
        self.emit(&[0x3A]); // LD A, (CURSOR_COL)
        self.emit_word(CURSOR_COL);
        self.ld_b_a();
        self.emit(&[0x3A]); // LD A, (CURSOR_ROW)
        self.emit_word(CURSOR_ROW);
        self.ld_c_a();
        self.emit(&[0xCD]); // CALL get_cell_addr
        self.fixup("get_cell_addr");
        // HL = cell address
        self.ld_a_hl_ind(); // type
        self.or_a_a();
        self.emit(&[0xCA]); // JP Z, load_cell_empty
        self.fixup("load_cell_empty");
        self.emit(&[0xFE, CELL_NUMBER]); // CP CELL_NUMBER
        self.emit(&[0xCA]); // JP Z, load_cell_number
        self.fixup("load_cell_number");
        self.emit(&[0xFE, CELL_FORMULA]); // CP CELL_FORMULA
        self.emit(&[0xCA]); // JP Z, load_cell_formula
        self.fixup("load_cell_formula");
        // Error or unknown - treat as empty
        self.label("load_cell_empty");
        self.xor_a();
        self.emit(&[0x32]); // LD (INPUT_LEN), A
        self.emit_word(INPUT_LEN);
        self.emit(&[0x32]); // LD (INPUT_POS), A
        self.emit_word(INPUT_POS);
        self.ret();

        // Load number into INPUT_BUF
        self.label("load_cell_number");
        self.inc_hl();
        self.inc_hl();
        self.emit(&[0x5E]); // LD E, (HL)
        self.inc_hl();
        self.emit(&[0x56]); // LD D, (HL)
        self.ex_de_hl(); //HL = value)
        // Convert HL to decimal string in INPUT_BUF
        self.emit(&[0xCD]); // CALL int_to_str
        self.fixup("int_to_str");
        self.ret();

        // Load formula into INPUT_BUF
        self.label("load_cell_formula");
        self.inc_hl();
        self.inc_hl();
        self.emit(&[0x5E]); // LD E, (HL)
        self.inc_hl();
        self.emit(&[0x56]); // LD D, (HL)
        // DE = formula pointer, copy to INPUT_BUF
        self.emit(&[0x21]); // LD HL, INPUT_BUF
        self.emit_word(INPUT_BUF);
        self.emit(&[0x06, 0x00]); // LD B, 0 (length counter)
        self.label("load_formula_loop");
        self.emit(&[0x1A]); // LD A, (DE)
        self.or_a_a();
        self.emit(&[0xCA]); // JP Z, load_formula_done
        self.fixup("load_formula_done");
        self.ld_hl_ind_a();
        self.inc_de();
        self.inc_hl();
        self.inc_b();
        self.emit(&[0xC3]); // JP load_formula_loop
        self.fixup("load_formula_loop");
        self.label("load_formula_done");
        self.ld_a_b();
        self.emit(&[0x32]); // LD (INPUT_LEN), A
        self.emit_word(INPUT_LEN);
        self.emit(&[0x32]); // LD (INPUT_POS), A
        self.emit_word(INPUT_POS);
        self.ret();

        // Parse number from INPUT_BUF to BCD
        // Returns: C = sign (0x00 = positive, 0x80 = negative)
        // BCD value is stored in BCD_TEMP1, carry set on error
        self.label("parse_number");
        self.emit(&[0x0E, 0x00]); // LD C, 0 (positive)
        self.emit(&[0x21]); // LD HL, INPUT_BUF
        self.emit_word(INPUT_BUF);

        // Check for minus sign
        self.ld_a_hl_ind();
        self.emit(&[0xFE, b'-']);
        self.emit(&[0x20, 0x03]); // JR NZ, +3 (skip sign handling: 2 bytes + 1 byte)
        self.emit(&[0x0E, 0x80]); // LD C, 0x80 (negative) - 2 bytes
        self.inc_hl(); // skip minus sign - 1 byte

        // Validate at least one digit exists
        self.ld_a_hl_ind();
        self.emit(&[0xFE, b'0']);
        self.emit(&[0xDA]); // JP C, parse_num_error
        self.fixup("parse_num_error");
        self.emit(&[0xFE, b'9' + 1]);
        self.emit(&[0xD2]); // JP NC, parse_num_error
        self.fixup("parse_num_error");

        // Call ascii_to_bcd (HL points to digit string)
        self.emit(&[0xCD]); // CALL ascii_to_bcd
        self.fixup("ascii_to_bcd");
        // BCD value now in BCD_TEMP1
        self.or_a_a(); // clear carry
        self.ret();

        self.label("parse_num_error");
        self.emit(&[0x37]); // SCF (set carry)
        self.ret();
    }

    /// Cell operations
    fn emit_cell_ops(&mut self) {
        // Get cell address from B=col, C=row
        // Returns address in HL
        self.label("get_cell_addr");
        // Address = CELL_DATA + (row * 16 + col) * 6
        // Use 16-bit arithmetic to avoid overflow when row >= 16
        self.emit(&[0x69]); // LD L, C (row)
        self.emit(&[0x26, 0x00]); // LD H, 0 (HL = row, 16-bit)
        self.add_hl_hl(); // x2
        self.add_hl_hl(); // x4
        self.add_hl_hl(); // x8
        self.add_hl_hl(); // x16
        self.emit(&[0x58]); // LD E, B (col)
        self.emit(&[0x16, 0x00]); // LD D, 0 (DE = col, 16-bit)
        self.add_hl_de(); // HL = row*16 + col
        // Multiply by 6: HL * 6 = HL * 4 + HL * 2
        self.add_hl_hl(); // x2
        self.push_hl(); // save x2
        self.add_hl_hl(); // x4
        self.pop_de(); // DE = x2
        self.add_hl_de(); // HL = x4 + x2 = x6
        // Add base address
        self.emit(&[0x11]); // LD DE, CELL_DATA
        self.emit_word(CELL_DATA);
        self.add_hl_de();
        self.ret();

        // Recalculate all formula cells
        self.label("recalculate");
        // For now, just a stub - formulas store their calculated value
        self.ret();
    }

    /// BCD arithmetic operations (8-digit packed BCD)
    fn emit_bcd_ops(&mut self) {
        // BCD values are stored big-endian: d7d6 d5d4 d3d2 d1d0
        // Sign is separate (byte 1 of cell: 0x00=positive, 0x80=negative)

        // bcd_add: Add BCD at (DE) to BCD at (HL), result at (HL)
        // Both point to 4-byte BCD data, carry returned if overflow
        self.label("bcd_add");
        // Work from LSB (byte 3) to MSB (byte 0)
        self.emit(&[0x23]); // INC HL (point to byte 1)
        self.emit(&[0x23]); // INC HL (point to byte 2)
        self.emit(&[0x23]); // INC HL (point to byte 3, LSB)
        self.emit(&[0x13]); // INC DE
        self.emit(&[0x13]); // INC DE
        self.emit(&[0x13]); // INC DE (DE points to LSB)
        self.emit(&[0x06, 4]); // LD B, 4 (4 bytes)
        self.or_a_a(); // clear carry
        self.label("bcd_add_loop");
        self.emit(&[0x1A]); // LD A, (DE)
        self.emit(&[0x8E]); // ADC A, (HL)
        self.emit(&[0x27]); // DAA
        self.emit(&[0x77]); // LD (HL), A
        self.emit(&[0x2B]); // DEC HL
        self.emit(&[0x1B]); // DEC DE
        self.emit(&[0x10]); // DJNZ bcd_add_loop
        self.emit_relative("bcd_add_loop");
        self.ret();

        // bcd_sub: Subtract BCD at (DE) from BCD at (HL), result at (HL)
        // Computes: (HL) = (HL) - (DE)
        // Uses Z80 SBC + DAA which works for BCD when N flag is set
        self.label("bcd_sub");
        // Work from LSB to MSB
        self.emit(&[0x23]); // INC HL x3 to point to LSB (byte 3)
        self.emit(&[0x23]);
        self.emit(&[0x23]);
        self.emit(&[0x13]); // INC DE x3
        self.emit(&[0x13]);
        self.emit(&[0x13]);
        self.emit(&[0x06, 4]); // LD B, 4 (4 bytes)
        self.or_a_a(); // clear carry (no initial borrow)
        self.label("bcd_sub_loop");
        // Load subtrahend, save it, load minuend, subtract, adjust
        self.emit(&[0x1A]); // LD A, (DE) = subtrahend
        self.emit(&[0x4F]); // LD C, A = save subtrahend in C
        self.emit(&[0x7E]); // LD A, (HL) = minuend
        self.emit(&[0x99]); // SBC A, C = minuend - subtrahend - borrow
        self.emit(&[0x27]); // DAA (works after SBC since N flag is set)
        self.emit(&[0x77]); // LD (HL), A = store result
        self.emit(&[0x2B]); // DEC HL
        self.emit(&[0x1B]); // DEC DE
        self.emit(&[0x10]); // DJNZ bcd_sub_loop
        self.emit_relative("bcd_sub_loop");
        self.ret();

        // bcd_cmp: Compare BCD at (HL) with BCD at (DE)
        // Returns: Z if equal, C if (HL) < (DE)
        self.label("bcd_cmp");
        self.emit(&[0x06, 4]); // LD B, 4
        self.label("bcd_cmp_loop");
        self.emit(&[0x1A]); // LD A, (DE)
        self.emit(&[0xBE]); // CP (HL)
        self.emit(&[0xC0]); // RET NZ (return with flags set)
        self.emit(&[0x23]); // INC HL
        self.emit(&[0x13]); // INC DE
        self.emit(&[0x10]); // DJNZ
        self.emit_relative("bcd_cmp_loop");
        self.ret(); // Z set if equal

        // bcd_zero: Zero 4-byte BCD at (HL)
        self.label("bcd_zero");
        self.emit(&[0xAF]);
        self.emit(&[0x77]); // LD (HL), A
        self.emit(&[0x23]); // INC HL
        self.emit(&[0x77]);
        self.emit(&[0x23]);
        self.emit(&[0x77]);
        self.emit(&[0x23]);
        self.emit(&[0x77]);
        self.ret();

        // bcd_copy: Copy 4-byte BCD from (DE) to (HL)
        self.label("bcd_copy");
        self.emit(&[0x06, 4]); // LD B, 4
        self.label("bcd_copy_loop");
        self.emit(&[0x1A]); // LD A, (DE)
        self.emit(&[0x77]); // LD (HL), A
        self.emit(&[0x23]); // INC HL
        self.emit(&[0x13]); // INC DE
        self.emit(&[0x10]); // DJNZ
        self.emit_relative("bcd_copy_loop");
        self.ret();

        // signed_add: Signed BCD addition (callable subroutine version)
        // Input: BCD_TEMP2 + BCD_TEMP1, SIGN_ACCUM = sign of TEMP2, SIGN_OP = sign of TEMP1
        // Output: Result in BCD_TEMP1, sign in SIGN_ACCUM
        self.label("signed_add");
        // Check if signs are the same
        self.emit(&[0x3A]); // LD A, (SIGN_ACCUM)
        self.emit_word(SIGN_ACCUM);
        self.ld_b_a();
        self.emit(&[0x3A]); // LD A, (SIGN_OP)
        self.emit_word(SIGN_OP);
        self.emit(&[0xB8]); // CP B
        self.emit(&[0xCA]); // JP Z, signed_add_same
        self.fixup("signed_add_same");

        // Different signs: subtract smaller magnitude from larger
        self.emit(&[0x21]); // LD HL, BCD_TEMP1
        self.emit_word(BCD_TEMP1);
        self.emit(&[0x11]); // LD DE, BCD_TEMP2
        self.emit_word(BCD_TEMP2);
        self.emit(&[0xCD]); // CALL bcd_cmp (C set if TEMP2 < TEMP1)
        self.fixup("bcd_cmp");
        self.emit(&[0xDA]); // JP C, signed_add_op_larger
        self.fixup("signed_add_op_larger");

        // TEMP2 >= TEMP1: result = TEMP2 - TEMP1, sign = SIGN_ACCUM
        self.emit(&[0x21]); // LD HL, BCD_TEMP2
        self.emit_word(BCD_TEMP2);
        self.emit(&[0x11]); // LD DE, BCD_TEMP1
        self.emit_word(BCD_TEMP1);
        self.emit(&[0xCD]); // CALL bcd_sub
        self.fixup("bcd_sub");
        // Copy result from TEMP2 to TEMP1
        self.emit(&[0x21]); // LD HL, BCD_TEMP1
        self.emit_word(BCD_TEMP1);
        self.emit(&[0x11]); // LD DE, BCD_TEMP2
        self.emit_word(BCD_TEMP2);
        self.emit(&[0xCD]); // CALL bcd_copy
        self.fixup("bcd_copy");
        self.ret();

        // TEMP1 > TEMP2: result = TEMP1 - TEMP2, sign = SIGN_OP
        self.label("signed_add_op_larger");
        self.emit(&[0x21]); // LD HL, BCD_TEMP1
        self.emit_word(BCD_TEMP1);
        self.emit(&[0x11]); // LD DE, BCD_TEMP2
        self.emit_word(BCD_TEMP2);
        self.emit(&[0xCD]); // CALL bcd_sub
        self.fixup("bcd_sub");
        // Set sign to SIGN_OP
        self.emit(&[0x3A]); // LD A, (SIGN_OP)
        self.emit_word(SIGN_OP);
        self.emit(&[0x32]); // LD (SIGN_ACCUM), A
        self.emit_word(SIGN_ACCUM);
        self.ret();

        // Same signs: add magnitudes, keep sign
        self.label("signed_add_same");
        self.emit(&[0x21]); // LD HL, BCD_TEMP1
        self.emit_word(BCD_TEMP1);
        self.emit(&[0x11]); // LD DE, BCD_TEMP2
        self.emit_word(BCD_TEMP2);
        self.emit(&[0xCD]); // CALL bcd_add
        self.fixup("bcd_add");
        self.ret();

        // bcd_mul: Multiply BCD at BCD_TEMP1 by BCD at BCD_TEMP2
        // Result in BCD_TEMP1 (only lower 8 digits kept)
        // Algorithm: Process multiplier from MSB to LSB
        //   For each digit: shift accumulator left, then add (multiplicand × digit)
        self.label("bcd_mul");
        // Clear accumulator (8 bytes for intermediate result)
        self.emit(&[0x21]); // LD HL, BCD_ACCUM
        self.emit_word(BCD_ACCUM);
        self.emit(&[0x06, 8]); // LD B, 8
        self.emit(&[0xAF]);
        self.label("bcd_mul_clr");
        self.emit(&[0x77]); // LD (HL), A
        self.emit(&[0x23]); // INC HL
        self.emit(&[0x10]); // DJNZ
        self.emit_relative("bcd_mul_clr");

        // Process multiplier from MSB to LSB (8 digits = 4 bytes)
        self.emit(&[0x0E, 8]); // LD C, 8 (digit counter)
        self.emit(&[0x21]); // LD HL, BCD_TEMP2 (MSB first)
        self.emit_word(BCD_TEMP2);

        self.label("bcd_mul_digit");
        // Get multiplier digit (high nibble first, then low)
        self.emit(&[0x7E]); // LD A, (HL)
        self.emit(&[0x0F]); // RRCA x4 (rotate high nibble to low)
        self.emit(&[0x0F]);
        self.emit(&[0x0F]);
        self.emit(&[0x0F]);
        self.emit(&[0xE6, 0x0F]); // AND 0x0F (high digit)
        self.push_hl();
        self.push_bc();
        self.emit(&[0xCD]); // CALL bcd_mul_by_digit
        self.fixup("bcd_mul_by_digit");
        self.pop_bc();
        self.pop_hl();
        self.dec_c();
        self.emit(&[0xCA]); // JP Z, bcd_mul_done
        self.fixup("bcd_mul_done");

        // Low nibble
        self.emit(&[0x7E]); // LD A, (HL)
        self.emit(&[0xE6, 0x0F]); // AND 0x0F (low digit)
        self.push_hl();
        self.push_bc();
        self.emit(&[0xCD]); // CALL bcd_mul_by_digit
        self.fixup("bcd_mul_by_digit");
        self.pop_bc();
        self.pop_hl();
        self.emit(&[0x23]); // INC HL (next byte of multiplier)
        self.dec_c();
        self.emit(&[0xC2]); // JP NZ, bcd_mul_digit
        self.fixup("bcd_mul_digit");

        self.label("bcd_mul_done");
        // Scale result by ÷100 for fixed-point (2 decimal places)
        // Shift 8-byte accumulator right by 2 BCD digits (1 byte)
        // This is needed because: cents × cents = cents², divide by 100 to get cents
        self.emit(&[0x21]); // LD HL, BCD_ACCUM+7 (destination)
        self.emit_word(BCD_ACCUM + 7);
        self.emit(&[0x11]); // LD DE, BCD_ACCUM+6 (source)
        self.emit_word(BCD_ACCUM + 6);
        self.emit(&[0x06, 7]); // LD B, 7 (copy 7 bytes)
        self.label("bcd_shr_loop");
        self.emit(&[0x1A]); // LD A, (DE)
        self.emit(&[0x77]); // LD (HL), A
        self.emit(&[0x2B]); // DEC HL
        self.emit(&[0x1B]); // DEC DE
        self.emit(&[0x10]); // DJNZ bcd_shr_loop
        self.emit_relative("bcd_shr_loop");
        // Clear byte 0 (MSB)
        self.emit(&[0x21]); // LD HL, BCD_ACCUM
        self.emit_word(BCD_ACCUM);
        self.xor_a();
        self.emit(&[0x77]); // LD (HL), A

        // Copy lower 4 bytes of accumulator to BCD_TEMP1
        self.emit(&[0x11]); // LD DE, BCD_ACCUM+4
        self.emit_word(BCD_ACCUM + 4);
        self.emit(&[0x21]); // LD HL, BCD_TEMP1
        self.emit_word(BCD_TEMP1);
        self.emit(&[0xCD]); // CALL bcd_copy
        self.fixup("bcd_copy");
        self.ret();

        // bcd_mul_by_digit: Shift accumulator left, then add BCD_TEMP1 × digit to accumulator
        // A = single digit (0-9)
        self.label("bcd_mul_by_digit");
        self.push_af();
        // Shift accumulator left by one BCD digit (×10)
        self.emit(&[0x21]); // LD HL, BCD_ACCUM
        self.emit_word(BCD_ACCUM);
        self.emit(&[0xCD]); // CALL bcd_shift_left
        self.fixup("bcd_shift_left");
        self.pop_af();
        // Now add BCD_TEMP1 × digit to accumulator
        self.or_a_a();
        self.ret_z(); // multiplying by 0 adds nothing
        self.emit(&[0x47]); // LD B, A (digit count for repeated addition)
        self.label("bcd_mul_add_loop");
        self.push_bc(); // Save B (digit counter) - bcd_add uses B internally
        // Add BCD_TEMP1 to accumulator at current position
        self.emit(&[0x21]); // LD HL, BCD_ACCUM+4 (lower 4 bytes)
        self.emit_word(BCD_ACCUM + 4);
        self.emit(&[0x11]); // LD DE, BCD_TEMP1
        self.emit_word(BCD_TEMP1);
        self.emit(&[0xCD]); // CALL bcd_add
        self.fixup("bcd_add");
        self.pop_bc(); // Restore digit counter
        self.emit(&[0x10]); // DJNZ bcd_mul_add_loop
        self.emit_relative("bcd_mul_add_loop");
        self.ret();

        // bcd_shift_left: Shift 8-byte BCD at (HL) left by one digit (×10)
        // Start from LSB (byte 7), shift nibbles toward MSB
        self.label("bcd_shift_left");
        self.emit(&[0x11, 7, 0]); // LD DE, 7 (offset to LSB)
        self.add_hl_de(); // HL points to byte 7 (LSB)
        self.emit(&[0x06, 8]); // LD B, 8
        self.emit(&[0xAF]); // carry nibble = 0
        self.label("bcd_shl_loop");
        self.emit(&[0x4F]); // LD C, A (save carry nibble from previous byte)
        self.emit(&[0x7E]); // LD A, (HL)
        self.emit(&[0x57]); // LD D, A (save original)
        // Shift left 4 bits: low nibble becomes high, carry becomes low
        self.emit(&[0x07]); // RLCA x4
        self.emit(&[0x07]);
        self.emit(&[0x07]);
        self.emit(&[0x07]);
        self.emit(&[0xE6, 0xF0]); // AND 0xF0 (shifted low nibble is now high)
        self.emit(&[0xB1]); // OR C (carry from previous becomes low)
        self.emit(&[0x77]); // LD (HL), A
        self.emit(&[0x7A]); // LD A, D (original value)
        self.emit(&[0xE6, 0xF0]); // AND 0xF0 (high nibble of original)
        self.emit(&[0x0F]); // RRCA x4 (move to low position for carry)
        self.emit(&[0x0F]);
        self.emit(&[0x0F]);
        self.emit(&[0x0F]);
        self.emit(&[0x2B]); // DEC HL (move toward MSB)
        self.emit(&[0x10]); // DJNZ
        self.emit_relative("bcd_shl_loop");
        self.ret();

        // bcd_div: Divide BCD at BCD_TEMP1 by BCD at BCD_TEMP2
        // Quotient in BCD_TEMP1, uses repeated subtraction
        self.label("bcd_div");
        // Check for divide by zero
        self.emit(&[0x21]); // LD HL, BCD_TEMP2
        self.emit_word(BCD_TEMP2);
        self.emit(&[0x7E]); // LD A, (HL)
        self.emit(&[0x23]);
        self.emit(&[0xB6]); // OR (HL)
        self.emit(&[0x23]);
        self.emit(&[0xB6]); // OR (HL)
        self.emit(&[0x23]);
        self.emit(&[0xB6]); // OR (HL)
        self.emit(&[0xC2]); // JP NZ, bcd_div_ok
        self.fixup("bcd_div_ok");
        self.emit(&[0x37]); // SCF (divide by zero)
        self.ret();

        self.label("bcd_div_ok");
        // Scale dividend by ×100 for fixed-point (2 decimal places)
        // Shift BCD_TEMP1 left by 2 BCD digits (1 byte)
        // This is needed because: cents / cents = dimensionless, multiply by 100 to get cents
        self.emit(&[0x21]); // LD HL, BCD_TEMP1 (destination)
        self.emit_word(BCD_TEMP1);
        self.emit(&[0x11]); // LD DE, BCD_TEMP1+1 (source)
        self.emit_word(BCD_TEMP1 + 1);
        self.emit(&[0x06, 3]); // LD B, 3 (copy 3 bytes)
        self.label("bcd_div_shl_loop");
        self.emit(&[0x1A]); // LD A, (DE)
        self.emit(&[0x77]); // LD (HL), A
        self.emit(&[0x23]); // INC HL
        self.emit(&[0x13]); // INC DE
        self.emit(&[0x10]); // DJNZ bcd_div_shl_loop
        self.emit_relative("bcd_div_shl_loop");
        // Clear last byte (LSB) with zeros
        self.xor_a();
        self.emit(&[0x77]); // LD (HL), A

        // Entry point for division without ×100 scaling (used by AVG)
        self.label("bcd_div_noscale");
        // Clear quotient accumulator
        self.emit(&[0x21]); // LD HL, BCD_ACCUM
        self.emit_word(BCD_ACCUM);
        self.emit(&[0xCD]); // CALL bcd_zero
        self.fixup("bcd_zero");
        self.emit(&[0x21]); // LD HL, BCD_ACCUM+4
        self.emit_word(BCD_ACCUM + 4);
        self.emit(&[0xCD]); // CALL bcd_zero
        self.fixup("bcd_zero");

        // Repeated subtraction: while BCD_TEMP1 >= BCD_TEMP2, subtract and increment quotient
        self.label("bcd_div_loop");
        // Compare BCD_TEMP1 with BCD_TEMP2
        // bcd_cmp returns C if (DE) < (HL), so swap args to get C when TEMP1 < TEMP2
        self.emit(&[0x21]); // LD HL, BCD_TEMP2
        self.emit_word(BCD_TEMP2);
        self.emit(&[0x11]); // LD DE, BCD_TEMP1
        self.emit_word(BCD_TEMP1);
        self.emit(&[0xCD]); // CALL bcd_cmp
        self.fixup("bcd_cmp");
        self.emit(&[0xDA]); // JP C, bcd_div_done (TEMP1 < TEMP2)
        self.fixup("bcd_div_done2");

        // Subtract: BCD_TEMP1 -= BCD_TEMP2
        self.emit(&[0x21]); // LD HL, BCD_TEMP1
        self.emit_word(BCD_TEMP1);
        self.emit(&[0x11]); // LD DE, BCD_TEMP2
        self.emit_word(BCD_TEMP2);
        self.emit(&[0xCD]); // CALL bcd_sub
        self.fixup("bcd_sub");

        // Increment quotient (BCD_ACCUM, lower 4 bytes)
        self.emit(&[0x21]); // LD HL, BCD_ACCUM+7 (LSB)
        self.emit_word(BCD_ACCUM + 7);
        self.emit(&[0x7E]); // LD A, (HL)
        self.emit(&[0xC6, 0x01]); // ADD A, 1
        self.emit(&[0x27]); // DAA
        self.emit(&[0x77]); // LD (HL), A
        self.emit(&[0x30]); // JR NC, bcd_div_loop (no carry, continue)
        self.emit_relative("bcd_div_loop");
        // Propagate carry through quotient
        self.emit(&[0x06, 3]); // LD B, 3 (3 more bytes)
        self.label("bcd_div_carry");
        self.emit(&[0x2B]); // DEC HL
        self.emit(&[0x7E]); // LD A, (HL)
        self.emit(&[0xCE, 0x00]); // ADC A, 0
        self.emit(&[0x27]); // DAA
        self.emit(&[0x77]); // LD (HL), A
        self.emit(&[0x30]); // JR NC, bcd_div_loop
        self.emit_relative("bcd_div_loop");
        self.emit(&[0x10]); // DJNZ bcd_div_carry
        self.emit_relative("bcd_div_carry");
        self.emit(&[0xC3]); // JP bcd_div_loop
        self.fixup("bcd_div_loop");

        self.label("bcd_div_done2");
        // Copy quotient to BCD_TEMP1
        self.emit(&[0x11]); // LD DE, BCD_ACCUM+4
        self.emit_word(BCD_ACCUM + 4);
        self.emit(&[0x21]); // LD HL, BCD_TEMP1
        self.emit_word(BCD_TEMP1);
        self.emit(&[0xCD]); // CALL bcd_copy
        self.fixup("bcd_copy");
        self.or_a_a(); // clear carry (success)
        self.ret();

        // ascii_to_bcd: Convert ASCII string at (HL) to packed BCD at BCD_TEMP1
        // Input: HL = pointer to null-terminated ASCII digits
        // Handles leading minus sign and decimal point (2 fixed decimal places)
        // Examples: "123.45" -> 12345, "123" -> 12300, "0.5" -> 50
        self.label("ascii_to_bcd");
        // Clear BCD_TEMP1
        self.push_hl();
        self.emit(&[0x21]); // LD HL, BCD_TEMP1
        self.emit_word(BCD_TEMP1);
        self.emit(&[0xCD]); // CALL bcd_zero
        self.fixup("bcd_zero");
        self.pop_hl();

        // Initialize: ATOB_FLAGS[0] = 0xFF (no decimal seen), ATOB_FLAGS[1] = 0 (frac digit count)
        self.emit(&[0x3E, 0xFF]); // LD A, 0xFF
        self.emit(&[0x32]); // LD (ATOB_FLAGS), A (decimal flag: FF=not seen)
        self.emit_word(ATOB_FLAGS);
        self.xor_a();
        self.emit(&[0x32]); // LD (ATOB_FLAGS+1), A (frac digit count = 0)
        self.emit_word(ATOB_FLAGS + 1);

        // Check for minus sign
        self.emit(&[0x7E]); // LD A, (HL)
        self.emit(&[0xFE, 0x2D]); // CP '-'
        self.emit(&[0x20, 0x01]); // JR NZ, +1
        self.emit(&[0x23]); // INC HL (skip minus)

        // Process each character
        self.label("atob_loop");
        self.emit(&[0x7E]); // LD A, (HL)
        self.or_a_a();
        self.emit(&[0xCA]); // JP Z, atob_done (null terminator)
        self.fixup("atob_done");

        // Check for decimal point
        self.emit(&[0xFE, b'.']); // CP '.'
        self.emit(&[0xC2]); // JP NZ, atob_not_decimal
        self.fixup("atob_not_decimal");
        // It's a decimal point - mark it and continue
        self.xor_a();
        self.emit(&[0x32]); // LD (ATOB_FLAGS), A (decimal flag = 0, seen)
        self.emit_word(ATOB_FLAGS);
        self.inc_hl();
        self.emit(&[0xC3]); // JP atob_loop
        self.fixup("atob_loop");

        self.label("atob_not_decimal");
        // Check if digit
        self.emit(&[0xFE, 0x30]); // CP '0'
        self.emit(&[0xDA]); // JP C, atob_done (< '0')
        self.fixup("atob_done");
        self.emit(&[0xFE, 0x3A]); // CP '9'+1
        self.emit(&[0xD2]); // JP NC, atob_done (> '9')
        self.fixup("atob_done");

        // Check if we've already parsed 2 fractional digits
        self.emit(&[0x3A]); // LD A, (ATOB_FLAGS+1)
        self.emit_word(ATOB_FLAGS + 1);
        self.emit(&[0xFE, 2]); // CP 2
        self.emit(&[0xD2]); // JP NC, atob_done (already have 2 frac digits)
        self.fixup("atob_done");

        // It's a valid digit - process it
        self.emit(&[0x7E]); // LD A, (HL) - reload char
        self.push_hl();
        self.emit(&[0xD6, 0x30]); // SUB '0' (convert to digit)
        self.push_af();

        // Shift BCD_TEMP1 left by one digit (4 bits)
        self.emit(&[0x06, 4]); // LD B, 4
        self.label("atob_shift");
        self.emit(&[0x21]); // LD HL, BCD_TEMP1+3 (LSB)
        self.emit_word(BCD_TEMP1 + 3);
        self.or_a_a(); // clear carry
        self.emit(&[0xCB, 0x26]); // SLA (HL)
        self.emit(&[0x2B]); // DEC HL
        self.emit(&[0xCB, 0x16]); // RL (HL)
        self.emit(&[0x2B]); // DEC HL
        self.emit(&[0xCB, 0x16]); // RL (HL)
        self.emit(&[0x2B]); // DEC HL
        self.emit(&[0xCB, 0x16]); // RL (HL)
        self.emit(&[0x10]); // DJNZ
        self.emit_relative("atob_shift");

        // Add new digit to LSB
        self.pop_af();
        self.emit(&[0x21]); // LD HL, BCD_TEMP1+3
        self.emit_word(BCD_TEMP1 + 3);
        self.emit(&[0xB6]); // OR (HL)
        self.emit(&[0x77]); // LD (HL), A
        self.pop_hl();

        // If decimal was seen, increment frac digit count
        self.emit(&[0x3A]); // LD A, (ATOB_FLAGS)
        self.emit_word(ATOB_FLAGS);
        self.or_a_a();
        self.emit(&[0x20, 0x07]); // JR NZ, +7 (skip if decimal not seen, 0xFF)
        self.emit(&[0x3A]); // LD A, (ATOB_FLAGS+1) - 3 bytes
        self.emit_word(ATOB_FLAGS + 1);
        self.inc_a(); // 1 byte
        self.emit(&[0x32]); // LD (ATOB_FLAGS+1), A - 3 bytes
        self.emit_word(ATOB_FLAGS + 1);
        // Total: 7 bytes

        self.emit(&[0x23]); // INC HL (next input char)
        self.emit(&[0xC3]); // JP atob_loop
        self.fixup("atob_loop");

        // Done parsing - need to scale if fewer than 2 frac digits
        self.label("atob_done");
        self.emit(&[0x3A]); // LD A, (ATOB_FLAGS)
        self.emit_word(ATOB_FLAGS);
        self.or_a_a();
        self.emit(&[0x20, 0x03]); // JR NZ, atob_no_decimal (FF = no decimal seen)
        // Decimal was seen - check frac digit count
        self.emit(&[0xC3]); // JP atob_check_frac
        self.fixup("atob_check_frac");

        self.label("atob_no_decimal");
        // No decimal point - multiply by 100 (shift left 8 bits = 2 BCD digits)
        self.emit(&[0x06, 8]); // LD B, 8 (shift 8 bits)
        self.emit(&[0xC3]); // JP atob_scale_loop
        self.fixup("atob_scale_loop");

        self.label("atob_check_frac");
        self.emit(&[0x3A]); // LD A, (ATOB_FLAGS+1)
        self.emit_word(ATOB_FLAGS + 1);
        self.emit(&[0xFE, 2]); // CP 2
        self.ret_nc(); // >= 2 frac digits, done
        self.emit(&[0xFE, 1]); // CP 1
        self.emit(&[0xCA]); // JP Z, atob_scale_1
        self.fixup("atob_scale_1");
        // 0 frac digits (e.g., "123." entered) - multiply by 100
        self.emit(&[0x06, 8]); // LD B, 8
        self.emit(&[0xC3]); // JP atob_scale_loop
        self.fixup("atob_scale_loop");

        self.label("atob_scale_1");
        // 1 frac digit - multiply by 10 (shift left 4 bits)
        self.emit(&[0x06, 4]); // LD B, 4

        self.label("atob_scale_loop");
        self.emit(&[0x21]); // LD HL, BCD_TEMP1+3
        self.emit_word(BCD_TEMP1 + 3);
        self.or_a_a();
        self.emit(&[0xCB, 0x26]); // SLA (HL)
        self.emit(&[0x2B]); // DEC HL
        self.emit(&[0xCB, 0x16]); // RL (HL)
        self.emit(&[0x2B]); // DEC HL
        self.emit(&[0xCB, 0x16]); // RL (HL)
        self.emit(&[0x2B]); // DEC HL
        self.emit(&[0xCB, 0x16]); // RL (HL)
        self.emit(&[0x10]); // DJNZ atob_scale_loop
        self.emit_relative("atob_scale_loop");
        self.ret();

        // bcd_to_ascii: Convert packed BCD at BCD_TEMP1 to ASCII in INPUT_BUF
        // Format: 6 whole digits + '.' + 2 fractional digits (fixed point, 2 decimal places)
        // Sets INPUT_LEN = 9
        self.label("bcd_to_ascii");
        self.emit(&[0x21]); // LD HL, INPUT_BUF
        self.emit_word(INPUT_BUF);
        self.emit(&[0x11]); // LD DE, BCD_TEMP1
        self.emit_word(BCD_TEMP1);

        // Output first 3 BCD bytes (6 digits = whole part)
        self.emit(&[0x06, 3]); // LD B, 3
        self.label("btoa_whole_loop");
        self.emit(&[0x1A]); // LD A, (DE)
        self.emit(&[0xF5]); // PUSH AF (save byte)
        // High nibble
        self.emit(&[0xCB, 0x3F]); // SRL A x4
        self.emit(&[0xCB, 0x3F]);
        self.emit(&[0xCB, 0x3F]);
        self.emit(&[0xCB, 0x3F]);
        self.emit(&[0xC6, 0x30]); // ADD A, '0'
        self.emit(&[0x77]); // LD (HL), A
        self.emit(&[0x23]); // INC HL
        // Low nibble
        self.emit(&[0xF1]); // POP AF
        self.emit(&[0xE6, 0x0F]); // AND 0x0F
        self.emit(&[0xC6, 0x30]); // ADD A, '0'
        self.emit(&[0x77]); // LD (HL), A
        self.emit(&[0x23]); // INC HL
        self.emit(&[0x13]); // INC DE
        self.emit(&[0x10]); // DJNZ btoa_whole_loop
        self.emit_relative("btoa_whole_loop");

        // Output decimal point
        self.emit(&[0x3E, b'.']); // LD A, '.'
        self.emit(&[0x77]); // LD (HL), A
        self.emit(&[0x23]); // INC HL

        // Output last BCD byte (2 digits = fractional part)
        self.emit(&[0x1A]); // LD A, (DE)
        self.emit(&[0xF5]); // PUSH AF
        // High nibble
        self.emit(&[0xCB, 0x3F]); // SRL A x4
        self.emit(&[0xCB, 0x3F]);
        self.emit(&[0xCB, 0x3F]);
        self.emit(&[0xCB, 0x3F]);
        self.emit(&[0xC6, 0x30]); // ADD A, '0'
        self.emit(&[0x77]); // LD (HL), A
        self.emit(&[0x23]); // INC HL
        // Low nibble
        self.emit(&[0xF1]); // POP AF
        self.emit(&[0xE6, 0x0F]); // AND 0x0F
        self.emit(&[0xC6, 0x30]); // ADD A, '0'
        self.emit(&[0x77]); // LD (HL), A
        self.emit(&[0x23]); // INC HL

        // Null terminate
        self.xor_a();
        self.emit(&[0x77]); // LD (HL), 0

        // Store length = 9
        self.emit(&[0x3E, 9]); // LD A, 9
        self.emit(&[0x32]); // LD (INPUT_LEN), A
        self.emit_word(INPUT_LEN);
        self.ret();

        // btoa_digit: Output single BCD digit (A) to (HL), increment HL and C
        // Simplified version - always outputs, leading zero handling in post-processing
        self.label("btoa_digit");
        // Just output the digit unconditionally
        self.emit(&[0xC6, 0x30]); // ADD A, '0'
        self.emit(&[0x77]); // LD (HL), A
        self.emit(&[0x23]); // INC HL
        self.emit(&[0x0C]); // INC C (length)
        self.ret();

        // Dummy labels that were referenced but no longer needed
        self.label("btoa_skip");
        self.ret();
        self.label("btoa_output");
        self.ret();
    }

    /// Formula parsing and evaluation
    fn emit_formula(&mut self) {
        // Parse formula from INPUT_BUF
        // Formula storage format: null-terminated string + 2-byte value
        self.label("parse_formula");

        // Check for empty formula (just '=')
        self.emit(&[0x3A]); // LD A, (INPUT_LEN)
        self.emit_word(INPUT_LEN);
        self.emit(&[0xFE, 2]); // CP 2 (need at least '=' + 1 char)
        self.emit(&[0xDA]); // JP C, store_error
        self.fixup("store_error");

        // Save formula pointer (where we'll store the formula)
        self.emit(&[0x2A]); // LD HL, (FORMULA_PTR)
        self.emit_word(FORMULA_PTR);
        self.push_hl(); //save formula start address)

        // Copy formula text from INPUT_BUF to formula storage
        self.emit(&[0x11]); // LD DE, INPUT_BUF
        self.emit_word(INPUT_BUF);
        self.emit(&[0x3A]); // LD A, (INPUT_LEN)
        self.emit_word(INPUT_LEN);
        self.ld_b_a(); //counter)
        self.label("copy_formula_loop");
        self.emit(&[0x1A]); // LD A, (DE)
        self.ld_hl_ind_a();
        self.inc_de();
        self.inc_hl();
        self.emit(&[0x10]); // DJNZ copy_formula_loop
        let offset = self.rom().len();
        self.emit(&[0x00]); // placeholder
        self.rom_mut()[offset] = (self.get_label("copy_formula_loop").unwrap_or(0)
            .wrapping_sub(self.pos())) as u8;
        // Null terminate
        self.emit(&[0x36, 0x00]); // LD (HL), 0
        self.inc_hl();
        // HL now points to where we'll store the calculated value
        self.push_hl(); //save value address)

        // Evaluate the expression (skip the '=')
        self.emit(&[0x21]); // LD HL, INPUT_BUF + 1
        self.emit_word(INPUT_BUF + 1);
        self.emit(&[0xCD]); // CALL eval_expr
        self.fixup("eval_expr");
        // HL = result, carry set on error
        self.emit(&[0xDA]); // JP C, formula_eval_error
        self.fixup("formula_eval_error");

        // Store sign + 4-byte BCD value after formula string
        self.pop_hl(); // HL = value address
        // Store sign byte first
        self.emit(&[0x3A]); // LD A, (SIGN_ACCUM)
        self.emit_word(SIGN_ACCUM);
        self.emit(&[0x77]); // LD (HL), A
        self.inc_hl();
        // Store 4 BCD bytes
        self.emit(&[0x11]); // LD DE, BCD_TEMP1
        self.emit_word(BCD_TEMP1);
        self.emit(&[0x06, 4]); // LD B, 4
        self.label("store_formula_bcd");
        self.emit(&[0x1A]); // LD A, (DE)
        self.emit(&[0x77]); // LD (HL), A
        self.inc_hl();
        self.inc_de();
        self.emit(&[0x10]); // DJNZ store_formula_bcd
        self.emit_relative("store_formula_bcd");
        // Update FORMULA_PTR (HL now points past 5-byte value)
        self.emit(&[0x22]); // LD (FORMULA_PTR), HL
        self.emit_word(FORMULA_PTR);

        // Store formula pointer in cell
        self.pop_hl(); //formula start address)
        self.push_hl(); //save it again)
        self.emit(&[0x3A]); // LD A, (CURSOR_COL)
        self.emit_word(CURSOR_COL);
        self.ld_b_a();
        self.emit(&[0x3A]); // LD A, (CURSOR_ROW)
        self.emit_word(CURSOR_ROW);
        self.ld_c_a();
        self.emit(&[0xCD]); // CALL get_cell_addr
        self.fixup("get_cell_addr");
        self.emit(&[0x36, CELL_FORMULA]); // LD (HL), CELL_FORMULA
        self.inc_hl();
        self.emit(&[0x36, 0x00]); // LD (HL), 0 (flags)
        self.inc_hl();
        self.pop_de(); //formula address)
        self.emit(&[0x73]); // LD (HL), E
        self.inc_hl();
        self.emit(&[0x72]); // LD (HL), D
        self.ret();

        self.label("formula_eval_error");
        // Clean up stack and store error
        self.pop_hl(); //discard value address)
        self.pop_hl(); //discard formula address)
        self.emit(&[0xC3]); // JP store_error
        self.fixup("store_error");

        // Evaluate expression with chaining support (e.g., =A1+A2+A3)
        // Input: HL = pointer to expression string
        // Output: Result in BCD_TEMP1, carry set on error
        self.label("eval_expr");
        self.emit(&[0x22]); // LD (TEMP2), HL (save expr ptr)
        self.emit_word(TEMP2);

        // Parse first operand (result goes to BCD_TEMP1, sign in TEMP1)
        self.emit(&[0xCD]); // CALL parse_operand
        self.fixup("parse_operand");
        self.emit(&[0xD8]); // RET C (error)
        // Save first operand's sign as accumulator sign
        self.emit(&[0x3A]); // LD A, (TEMP1)
        self.emit_word(TEMP1);
        self.emit(&[0x32]); // LD (SIGN_ACCUM), A
        self.emit_word(SIGN_ACCUM);

        // Main evaluation loop - check for more operators
        self.label("eval_loop");
        // Save accumulator: copy BCD_TEMP1 to BCD_ACCUM
        self.emit(&[0x21]); // LD HL, BCD_ACCUM
        self.emit_word(BCD_ACCUM);
        self.emit(&[0x11]); // LD DE, BCD_TEMP1
        self.emit_word(BCD_TEMP1);
        self.emit(&[0xCD]); // CALL bcd_copy
        self.fixup("bcd_copy");

        self.emit(&[0x2A]); // LD HL, (TEMP2)
        self.emit_word(TEMP2);
        self.ld_a_hl_ind();
        self.or_a_a();
        self.emit(&[0xCA]); // JP Z, eval_done (no more operators)
        self.fixup("eval_done");

        // Save operator
        self.emit(&[0x32]); // LD (TEMP1+1), A
        self.emit_word(TEMP1 + 1);
        self.inc_hl(); // past operator
        self.emit(&[0x22]); // LD (TEMP2), HL
        self.emit_word(TEMP2);

        // Parse next operand (result goes to BCD_TEMP1, sign in TEMP1)
        self.emit(&[0xCD]); // CALL parse_operand
        self.fixup("parse_operand");
        self.emit(&[0xDA]); // JP C, eval_chain_error
        self.fixup("eval_chain_error");
        // Save operand's sign to SIGN_OP
        self.emit(&[0x3A]); // LD A, (TEMP1)
        self.emit_word(TEMP1);
        self.emit(&[0x32]); // LD (SIGN_OP), A
        self.emit_word(SIGN_OP);

        // Now: BCD_TEMP1 = new operand, BCD_ACCUM = old accumulator
        // Copy BCD_ACCUM to BCD_TEMP2 for operation
        self.emit(&[0x21]); // LD HL, BCD_TEMP2
        self.emit_word(BCD_TEMP2);
        self.emit(&[0x11]); // LD DE, BCD_ACCUM
        self.emit_word(BCD_ACCUM);
        self.emit(&[0xCD]); // CALL bcd_copy
        self.fixup("bcd_copy");
        // BCD_TEMP1 = new operand, BCD_TEMP2 = old accumulator

        // Get operator and dispatch
        self.emit(&[0x3A]); // LD A, (TEMP1+1)
        self.emit_word(TEMP1 + 1);
        self.emit(&[0xFE, b'+']);
        self.emit(&[0xCA]); // JP Z, eval_add
        self.fixup("eval_add");
        self.emit(&[0xFE, b'-']);
        self.emit(&[0xCA]); // JP Z, eval_sub
        self.fixup("eval_sub");
        self.emit(&[0xFE, b'*']);
        self.emit(&[0xCA]); // JP Z, eval_mul
        self.fixup("eval_mul");
        self.emit(&[0xFE, b'/']);
        self.emit(&[0xCA]); // JP Z, eval_div
        self.fixup("eval_div");
        // Unknown operator - error
        self.emit(&[0x37]); // SCF
        self.ret();

        self.label("eval_done");
        // Result is in BCD_TEMP1, copy back to BCD_ACCUM for formula storage
        // Actually, we need to return the BCD in a usable format
        self.or_a_a(); // clear carry
        self.ret();

        self.label("eval_chain_error");
        self.emit(&[0x37]); // SCF
        self.ret();

        // Signed addition: BCD_TEMP2 + BCD_TEMP1 -> BCD_TEMP1
        // SIGN_ACCUM = sign of TEMP2, SIGN_OP = sign of TEMP1
        self.label("eval_add");
        // Check if signs are the same
        self.emit(&[0x3A]); // LD A, (SIGN_ACCUM)
        self.emit_word(SIGN_ACCUM);
        self.ld_b_a();
        self.emit(&[0x3A]); // LD A, (SIGN_OP)
        self.emit_word(SIGN_OP);
        self.emit(&[0xB8]); // CP B (compare signs)
        self.emit(&[0xCA]); // JP Z, eval_add_same_sign
        self.fixup("eval_add_same_sign");

        // Different signs: need to subtract smaller from larger
        // Compare magnitudes: TEMP2 vs TEMP1
        self.emit(&[0x21]); // LD HL, BCD_TEMP1
        self.emit_word(BCD_TEMP1);
        self.emit(&[0x11]); // LD DE, BCD_TEMP2
        self.emit_word(BCD_TEMP2);
        self.emit(&[0xCD]); // CALL bcd_cmp (C set if TEMP2 < TEMP1)
        self.fixup("bcd_cmp");
        self.emit(&[0xDA]); // JP C, eval_add_op_larger (TEMP2 < TEMP1)
        self.fixup("eval_add_op_larger");

        // TEMP2 >= TEMP1: result = TEMP2 - TEMP1, sign = SIGN_ACCUM
        self.emit(&[0x21]); // LD HL, BCD_TEMP2
        self.emit_word(BCD_TEMP2);
        self.emit(&[0x11]); // LD DE, BCD_TEMP1
        self.emit_word(BCD_TEMP1);
        self.emit(&[0xCD]); // CALL bcd_sub (TEMP2 - TEMP1 -> TEMP2)
        self.fixup("bcd_sub");
        // Copy result from TEMP2 to TEMP1
        self.emit(&[0x21]); // LD HL, BCD_TEMP1
        self.emit_word(BCD_TEMP1);
        self.emit(&[0x11]); // LD DE, BCD_TEMP2
        self.emit_word(BCD_TEMP2);
        self.emit(&[0xCD]); // CALL bcd_copy
        self.fixup("bcd_copy");
        // Sign stays as SIGN_ACCUM (already set)
        self.emit(&[0xC3]); // JP eval_loop
        self.fixup("eval_loop");

        // TEMP1 > TEMP2: result = TEMP1 - TEMP2, sign = SIGN_OP
        self.label("eval_add_op_larger");
        self.emit(&[0x21]); // LD HL, BCD_TEMP1
        self.emit_word(BCD_TEMP1);
        self.emit(&[0x11]); // LD DE, BCD_TEMP2
        self.emit_word(BCD_TEMP2);
        self.emit(&[0xCD]); // CALL bcd_sub (TEMP1 - TEMP2 -> TEMP1)
        self.fixup("bcd_sub");
        // Set result sign to SIGN_OP
        self.emit(&[0x3A]); // LD A, (SIGN_OP)
        self.emit_word(SIGN_OP);
        self.emit(&[0x32]); // LD (SIGN_ACCUM), A
        self.emit_word(SIGN_ACCUM);
        self.emit(&[0xC3]); // JP eval_loop
        self.fixup("eval_loop");

        // Same signs: just add magnitudes, keep the sign
        self.label("eval_add_same_sign");
        self.emit(&[0x21]); // LD HL, BCD_TEMP1
        self.emit_word(BCD_TEMP1);
        self.emit(&[0x11]); // LD DE, BCD_TEMP2
        self.emit_word(BCD_TEMP2);
        self.emit(&[0xCD]); // CALL bcd_add
        self.fixup("bcd_add");
        // Sign stays as SIGN_ACCUM (same as SIGN_OP)
        self.emit(&[0xC3]); // JP eval_loop
        self.fixup("eval_loop");

        // Signed subtraction: A - B = A + (-B)
        // Just flip SIGN_OP and use addition logic
        self.label("eval_sub");
        self.emit(&[0x3A]); // LD A, (SIGN_OP)
        self.emit_word(SIGN_OP);
        self.emit(&[0xEE, 0x80]); // XOR 0x80 (flip sign)
        self.emit(&[0x32]); // LD (SIGN_OP), A
        self.emit_word(SIGN_OP);
        self.emit(&[0xC3]); // JP eval_add
        self.fixup("eval_add");

        // BCD_TEMP2 * BCD_TEMP1 -> BCD_TEMP1 (with sign handling)
        self.label("eval_mul");
        // Result sign = SIGN_ACCUM XOR SIGN_OP
        self.emit(&[0x3A]); // LD A, (SIGN_ACCUM)
        self.emit_word(SIGN_ACCUM);
        self.ld_b_a();
        self.emit(&[0x3A]); // LD A, (SIGN_OP)
        self.emit_word(SIGN_OP);
        self.emit(&[0xA8]); // XOR B
        self.emit(&[0x32]); // LD (SIGN_ACCUM), A (result sign)
        self.emit_word(SIGN_ACCUM);
        // Do the multiplication
        self.emit(&[0xCD]); // CALL bcd_mul
        self.fixup("bcd_mul");
        self.emit(&[0xC3]); // JP eval_loop
        self.fixup("eval_loop");

        // BCD_TEMP2 / BCD_TEMP1 -> BCD_TEMP1 (with sign handling)
        self.label("eval_div");
        // Result sign = SIGN_ACCUM XOR SIGN_OP
        self.emit(&[0x3A]); // LD A, (SIGN_ACCUM)
        self.emit_word(SIGN_ACCUM);
        self.ld_b_a();
        self.emit(&[0x3A]); // LD A, (SIGN_OP)
        self.emit_word(SIGN_OP);
        self.emit(&[0xA8]); // XOR B
        self.emit(&[0x32]); // LD (SIGN_ACCUM), A (result sign)
        self.emit_word(SIGN_ACCUM);
        // bcd_div: BCD_TEMP1 / BCD_TEMP2 -> BCD_TEMP1
        // We need: TEMP2 (old accum) / TEMP1 (new operand) -> TEMP1
        // Swap TEMP1 and TEMP2 first
        self.emit(&[0x21]); // LD HL, BCD_ACCUM (use as temp)
        self.emit_word(BCD_ACCUM);
        self.emit(&[0x11]); // LD DE, BCD_TEMP1
        self.emit_word(BCD_TEMP1);
        self.emit(&[0xCD]); // CALL bcd_copy (ACCUM = TEMP1)
        self.fixup("bcd_copy");
        self.emit(&[0x21]); // LD HL, BCD_TEMP1
        self.emit_word(BCD_TEMP1);
        self.emit(&[0x11]); // LD DE, BCD_TEMP2
        self.emit_word(BCD_TEMP2);
        self.emit(&[0xCD]); // CALL bcd_copy (TEMP1 = TEMP2)
        self.fixup("bcd_copy");
        self.emit(&[0x21]); // LD HL, BCD_TEMP2
        self.emit_word(BCD_TEMP2);
        self.emit(&[0x11]); // LD DE, BCD_ACCUM
        self.emit_word(BCD_ACCUM);
        self.emit(&[0xCD]); // CALL bcd_copy (TEMP2 = ACCUM, completing swap)
        self.fixup("bcd_copy");
        // Now TEMP1 has dividend, TEMP2 has divisor
        self.emit(&[0xCD]); // CALL bcd_div
        self.fixup("bcd_div");
        self.emit(&[0xC3]); // JP eval_loop
        self.fixup("eval_loop");

        // Parse an operand (cell reference or number)
        // Input: (TEMP2) = pointer to string
        // Output: HL = value, (TEMP2) updated, carry set on error
        // Supports absolute references: $A$1, $A1, A$1
        self.label("parse_operand");
        self.emit(&[0x2A]); // LD HL, (TEMP2)
        self.emit_word(TEMP2);
        self.ld_a_hl_ind();

        // Check for @ (function prefix)
        self.emit(&[0xFE, b'@']);
        self.emit(&[0xCA]); // JP Z, parse_func
        self.fixup("parse_func");

        // Skip leading $ (absolute column marker)
        self.emit(&[0xFE, b'$']);
        self.emit(&[0xC2]); // JP NZ, parse_op_no_dollar1
        self.fixup("parse_op_no_dollar1");
        self.inc_hl(); //skip $)
        self.ld_a_hl_ind();
        self.label("parse_op_no_dollar1");

        // Convert lowercase to uppercase (a-z -> A-Z)
        self.emit(&[0xFE, b'a']);
        self.emit(&[0xDA]); // JP C, parse_op_check_upper (< 'a')
        self.fixup("parse_op_check_upper");
        self.emit(&[0xFE, b'z' + 1]);
        self.emit(&[0xD2]); // JP NC, parse_op_check_upper (> 'z')
        self.fixup("parse_op_check_upper");
        self.emit(&[0xD6, 0x20]); // SUB 0x20 (convert to uppercase)

        self.label("parse_op_check_upper");
        // Check if it's a letter (cell reference A-P)
        self.emit(&[0xFE, b'A']);
        self.emit(&[0xDA]); // JP C, parse_op_number
        self.fixup("parse_op_number");
        self.emit(&[0xFE, b'P' + 1]);
        self.emit(&[0xD2]); // JP NC, parse_op_number
        self.fixup("parse_op_number");

        // It's a cell reference
        self.emit(&[0xD6, b'A']); // SUB 'A' (column)
        self.ld_b_a();
        self.inc_hl();
        // Skip $ before row (absolute row marker)
        self.ld_a_hl_ind();
        self.emit(&[0xFE, b'$']);
        self.emit(&[0xC2]); // JP NZ, parse_op_no_dollar2
        self.fixup("parse_op_no_dollar2");
        self.inc_hl(); //skip $)
        self.label("parse_op_no_dollar2");
        // Parse row number
        self.emit(&[0x0E, 0x00]); // LD C, 0 (accumulator)
        self.label("parse_row_loop");
        self.ld_a_hl_ind();
        self.emit(&[0xFE, b'0']);
        self.emit(&[0xDA]); // JP C, parse_row_done
        self.fixup("parse_row_done");
        self.emit(&[0xFE, b'9' + 1]);
        self.emit(&[0xD2]); // JP NC, parse_row_done
        self.fixup("parse_row_done");
        self.emit(&[0xD6, b'0']); // SUB '0'
        self.ld_e_a();
        self.ld_a_c();
        self.emit(&[0x87]); // ADD A, A (×2)
        self.emit(&[0x87]); // ADD A, A (×4)
        self.emit(&[0x81]); // ADD A, C (×5)
        self.emit(&[0x87]); // ADD A, A (×10)
        self.emit(&[0x83]); // ADD A, E
        self.ld_c_a();
        self.inc_hl();
        self.emit(&[0xC3]); // JP parse_row_loop
        self.fixup("parse_row_loop");

        self.label("parse_row_done");
        self.emit(&[0x22]); // LD (TEMP2), HL (update pointer)
        self.emit_word(TEMP2);
        // B = col, C = row (1-based), convert to 0-based
        self.dec_c();
        // Get cell value as BCD into BCD_TEMP1
        self.emit(&[0xCD]); // CALL get_cell_addr
        self.fixup("get_cell_addr");
        self.ld_a_hl_ind(); // type
        self.or_a_a();
        self.emit(&[0xCA]); // JP Z, parse_op_zero (empty cell = 0)
        self.fixup("parse_op_zero");
        // Check if formula (type 2)
        self.emit(&[0xFE, CELL_FORMULA]); // CP CELL_FORMULA
        self.emit(&[0xCA]); // JP Z, parse_op_formula
        self.fixup("parse_op_formula");
        // Number cell: copy sign and BCD from cell to BCD_TEMP1
        self.inc_hl();
        self.ld_a_hl_ind(); // sign
        self.emit(&[0x32]); // LD (BCD_SIGN), A - save sign for later
        self.emit_word(TEMP1); // using TEMP1 to store sign
        self.inc_hl();
        // Copy 4 BCD bytes to BCD_TEMP1
        self.emit(&[0x11]); // LD DE, BCD_TEMP1
        self.emit_word(BCD_TEMP1);
        self.emit(&[0x06, 4]); // LD B, 4
        self.label("load_cell_bcd");
        self.ld_a_hl_ind();
        self.emit(&[0x12]); // LD (DE), A
        self.inc_hl();
        self.inc_de();
        self.emit(&[0x10]); // DJNZ
        self.emit_relative("load_cell_bcd");
        self.or_a_a(); // clear carry
        self.ret();

        // Formula cell: get computed value from formula storage
        self.label("parse_op_formula");
        self.inc_hl(); // skip type
        self.inc_hl(); // skip flags
        // Get formula pointer
        self.emit(&[0x5E]); // LD E, (HL)
        self.inc_hl();
        self.emit(&[0x56]); // LD D, (HL)
        // DE = formula pointer, find end of string
        self.ex_de_hl();
        self.label("parse_op_find_end");
        self.ld_a_hl_ind();
        self.inc_hl();
        self.or_a_a();
        self.emit(&[0xC2]); // JP NZ, parse_op_find_end
        self.fixup("parse_op_find_end");
        // HL now points to sign byte, then 4 BCD bytes
        self.ld_a_hl_ind(); // load sign
        self.emit(&[0x32]); // LD (TEMP1), A
        self.emit_word(TEMP1);
        self.inc_hl(); // point to BCD
        self.emit(&[0x11]); // LD DE, BCD_TEMP1
        self.emit_word(BCD_TEMP1);
        self.emit(&[0x06, 4]); // LD B, 4
        self.label("load_formula_bcd_op");
        self.ld_a_hl_ind();
        self.emit(&[0x12]); // LD (DE), A
        self.inc_hl();
        self.inc_de();
        self.emit(&[0x10]); // DJNZ load_formula_bcd_op
        self.emit_relative("load_formula_bcd_op");
        self.or_a_a(); // clear carry
        self.ret();

        self.label("parse_op_zero");
        // Zero BCD_TEMP1
        self.emit(&[0x21]); // LD HL, BCD_TEMP1
        self.emit_word(BCD_TEMP1);
        self.emit(&[0xCD]); // CALL bcd_zero
        self.fixup("bcd_zero");
        self.emit(&[0xAF]); // XOR A
        self.emit(&[0x32]); // LD (TEMP1), A (sign = 0)
        self.emit_word(TEMP1);
        self.or_a_a();
        self.ret();

        // Parse number operand to BCD
        // Uses ascii_to_bcd which stops at non-digit chars
        self.label("parse_op_number");
        self.emit(&[0x2A]); // LD HL, (TEMP2)
        self.emit_word(TEMP2);
        self.emit(&[0xAF]); // XOR A (clear sign)
        self.emit(&[0x32]); // LD (TEMP1), A
        self.emit_word(TEMP1);

        // Check minus
        self.ld_a_hl_ind();
        self.emit(&[0xFE, b'-']);
        self.emit(&[0x20, 0x06]); // JR NZ, +6 (skip negative handling: 2+3+1=6 bytes)
        self.emit(&[0x3E, 0x80]); // LD A, 0x80 (negative sign) - 2 bytes
        self.emit(&[0x32]); // LD (TEMP1), A - 3 bytes with word
        self.emit_word(TEMP1);
        self.inc_hl(); // - 1 byte

        // Call ascii_to_bcd (HL points to digit string)
        // Result in BCD_TEMP1, HL updated past digits
        self.emit(&[0xCD]); // CALL ascii_to_bcd
        self.fixup("ascii_to_bcd");

        // Update TEMP2 with new position (scan past digits and decimal point)
        self.emit(&[0x2A]); // LD HL, (TEMP2)
        self.emit_word(TEMP2);
        self.ld_a_hl_ind();
        self.emit(&[0xFE, b'-']);
        self.emit(&[0x20, 0x01]); // JR NZ, +1
        self.inc_hl();
        self.label("parse_opn_scan");
        self.ld_a_hl_ind();
        // Check for decimal point
        self.emit(&[0xFE, b'.']);
        self.emit(&[0xCA]); // JP Z, parse_opn_next (skip decimal point)
        self.fixup("parse_opn_next");
        // Check for digit
        self.emit(&[0xFE, b'0']);
        self.emit(&[0xDA]); // JP C, parse_opn_done (< '0')
        self.fixup("parse_opn_done");
        self.emit(&[0xFE, b'9' + 1]);
        self.emit(&[0xD2]); // JP NC, parse_opn_done (> '9')
        self.fixup("parse_opn_done");
        self.label("parse_opn_next");
        self.inc_hl();
        self.emit(&[0xC3]); // JP parse_opn_scan
        self.fixup("parse_opn_scan");

        self.label("parse_opn_done");
        self.emit(&[0x22]); // LD (TEMP2), HL
        self.emit_word(TEMP2);
        self.or_a_a(); // clear carry
        self.ret();

        // Parse function like @SUM(A1:A5), @AVG, @MIN, @MAX, @COUNT
        // FUNC_TYPE: 0=SUM, 1=AVG, 2=MIN, 3=MAX, 4=COUNT
        self.label("parse_func");
        self.inc_hl(); //skip @)
        self.ld_a_hl_ind();
        self.emit(&[0xE6, 0xDF]); // AND 0xDF (uppercase)

        // Check first letter: S=SUM, A=AVG, M=MIN/MAX, C=COUNT
        self.emit(&[0xFE, b'S']);
        self.emit(&[0xCA]); // JP Z, pf_sum
        self.fixup("pf_sum");
        self.emit(&[0xFE, b'A']);
        self.emit(&[0xCA]); // JP Z, pf_avg
        self.fixup("pf_avg");
        self.emit(&[0xFE, b'M']);
        self.emit(&[0xCA]); // JP Z, pf_minmax
        self.fixup("pf_minmax");
        self.emit(&[0xFE, b'C']);
        self.emit(&[0xCA]); // JP Z, pf_count
        self.fixup("pf_count");
        self.emit(&[0xC3]); // JP pf_error
        self.fixup("pf_error");

        // @SUM - check "UM("
        self.label("pf_sum");
        self.emit(&[0x3E, 0x00]); // LD A, 0 (SUM type)
        self.emit(&[0x32]); // LD (FUNC_TYPE), A
        self.emit_word(FUNC_TYPE);
        self.inc_hl();
        self.ld_a_hl_ind();
        self.emit(&[0xE6, 0xDF]); // uppercase
        self.emit(&[0xFE, b'U']);
        self.emit(&[0xC2]); // JP NZ, pf_error
        self.fixup("pf_error");
        self.inc_hl();
        self.ld_a_hl_ind();
        self.emit(&[0xE6, 0xDF]);
        self.emit(&[0xFE, b'M']);
        self.emit(&[0xC2]); // JP NZ, pf_error
        self.fixup("pf_error");
        self.emit(&[0xC3]); // JP pf_parse_paren
        self.fixup("pf_parse_paren");

        // @AVG - check "VG("
        self.label("pf_avg");
        self.emit(&[0x3E, 0x01]); // LD A, 1 (AVG type)
        self.emit(&[0x32]); // LD (FUNC_TYPE), A
        self.emit_word(FUNC_TYPE);
        self.inc_hl();
        self.ld_a_hl_ind();
        self.emit(&[0xE6, 0xDF]);
        self.emit(&[0xFE, b'V']);
        self.emit(&[0xC2]); // JP NZ, pf_error
        self.fixup("pf_error");
        self.inc_hl();
        self.ld_a_hl_ind();
        self.emit(&[0xE6, 0xDF]);
        self.emit(&[0xFE, b'G']);
        self.emit(&[0xC2]); // JP NZ, pf_error
        self.fixup("pf_error");
        self.emit(&[0xC3]); // JP pf_parse_paren
        self.fixup("pf_parse_paren");

        // @MIN or @MAX - check "IN(" or "AX("
        self.label("pf_minmax");
        self.inc_hl();
        self.ld_a_hl_ind();
        self.emit(&[0xE6, 0xDF]);
        self.emit(&[0xFE, b'I']);
        self.emit(&[0xCA]); // JP Z, pf_min
        self.fixup("pf_min");
        self.emit(&[0xFE, b'A']);
        self.emit(&[0xC2]); // JP NZ, pf_error
        self.fixup("pf_error");
        // MAX
        self.emit(&[0x3E, 0x03]); // LD A, 3 (MAX type)
        self.emit(&[0x32]); // LD (FUNC_TYPE), A
        self.emit_word(FUNC_TYPE);
        self.inc_hl();
        self.ld_a_hl_ind();
        self.emit(&[0xE6, 0xDF]);
        self.emit(&[0xFE, b'X']);
        self.emit(&[0xC2]); // JP NZ, pf_error
        self.fixup("pf_error");
        self.emit(&[0xC3]); // JP pf_parse_paren
        self.fixup("pf_parse_paren");

        self.label("pf_min");
        self.emit(&[0x3E, 0x02]); // LD A, 2 (MIN type)
        self.emit(&[0x32]); // LD (FUNC_TYPE), A
        self.emit_word(FUNC_TYPE);
        self.inc_hl();
        self.ld_a_hl_ind();
        self.emit(&[0xE6, 0xDF]);
        self.emit(&[0xFE, b'N']);
        self.emit(&[0xC2]); // JP NZ, pf_error
        self.fixup("pf_error");
        self.emit(&[0xC3]); // JP pf_parse_paren
        self.fixup("pf_parse_paren");

        // @COUNT - check "OUNT("
        self.label("pf_count");
        self.emit(&[0x3E, 0x04]); // LD A, 4 (COUNT type)
        self.emit(&[0x32]); // LD (FUNC_TYPE), A
        self.emit_word(FUNC_TYPE);
        self.inc_hl();
        self.ld_a_hl_ind();
        self.emit(&[0xE6, 0xDF]);
        self.emit(&[0xFE, b'O']);
        self.emit(&[0xC2]); // JP NZ, pf_error
        self.fixup("pf_error");
        self.inc_hl();
        self.ld_a_hl_ind();
        self.emit(&[0xE6, 0xDF]);
        self.emit(&[0xFE, b'U']);
        self.emit(&[0xC2]); // JP NZ, pf_error
        self.fixup("pf_error");
        self.inc_hl();
        self.ld_a_hl_ind();
        self.emit(&[0xE6, 0xDF]);
        self.emit(&[0xFE, b'N']);
        self.emit(&[0xC2]); // JP NZ, pf_error
        self.fixup("pf_error");
        self.inc_hl();
        self.ld_a_hl_ind();
        self.emit(&[0xE6, 0xDF]);
        self.emit(&[0xFE, b'T']);
        self.emit(&[0xC2]); // JP NZ, pf_error
        self.fixup("pf_error");
        // fall through to pf_parse_paren

        // Parse opening paren
        self.label("pf_parse_paren");
        self.inc_hl();
        self.ld_a_hl_ind();
        self.emit(&[0xFE, b'(']);
        self.emit(&[0xC2]); // JP NZ, pf_error
        self.fixup("pf_error");
        self.inc_hl();

        // Parse first cell: col1, row1
        self.ld_a_hl_ind();
        self.emit(&[0xE6, 0xDF]); // AND 0xDF (uppercase)
        self.emit(&[0xFE, b'A']);
        self.emit(&[0xDA]); // JP C, pf_error
        self.fixup("pf_error");
        self.emit(&[0xFE, b'Q']);
        self.emit(&[0xD2]); // JP NC, pf_error
        self.fixup("pf_error");
        self.emit(&[0xD6, b'A']); // SUB 'A'
        self.emit(&[0x32]); // LD (TEMP1), A (col1)
        self.emit_word(TEMP1);
        self.inc_hl();
        // Parse row1
        self.emit(&[0x0E, 0x00]); // LD C, 0
        self.label("pf_row1_loop");
        self.ld_a_hl_ind();
        self.emit(&[0xFE, b'0']);
        self.emit(&[0xDA]); // JP C, pf_row1_done
        self.fixup("pf_row1_done");
        self.emit(&[0xFE, b'9' + 1]);
        self.emit(&[0xD2]); // JP NC, pf_row1_done
        self.fixup("pf_row1_done");
        self.emit(&[0xD6, b'0']); // digit
        self.ld_b_a();
        self.ld_a_c();
        self.emit(&[0x87]); // x2
        self.emit(&[0x4F]); // save
        self.emit(&[0x87]); // x4
        self.emit(&[0x87]); // x8
        self.emit(&[0x81]); // +x2 = x10
        self.emit(&[0x80]); // +digit
        self.ld_c_a();
        self.inc_hl();
        self.emit(&[0xC3]); // JP pf_row1_loop
        self.fixup("pf_row1_loop");
        self.label("pf_row1_done");
        self.ld_a_c();
        self.dec_a(); //0-based)
        self.emit(&[0x32]); // LD (TEMP1+1), A (row1)
        self.emit_word(TEMP1 + 1);

        // Check for :
        self.ld_a_hl_ind();
        self.emit(&[0xFE, b':']);
        self.emit(&[0xC2]); // JP NZ, pf_error
        self.fixup("pf_error");
        self.inc_hl();

        // Parse second cell - col2 and row2
        self.ld_a_hl_ind();
        self.emit(&[0xE6, 0xDF]); // uppercase
        self.emit(&[0xFE, b'A']);
        self.emit(&[0xDA]); // JP C, pf_error
        self.fixup("pf_error");
        self.emit(&[0xD6, b'A']); // SUB 'A'
        self.emit(&[0x32]); // LD (RANGE_COL2), A (col2)
        self.emit_word(RANGE_COL2);
        self.inc_hl();
        // Parse row2
        self.emit(&[0x0E, 0x00]); // LD C, 0
        self.label("pf_row2_loop");
        self.ld_a_hl_ind();
        self.emit(&[0xFE, b'0']);
        self.emit(&[0xDA]); // JP C, pf_row2_done
        self.fixup("pf_row2_done");
        self.emit(&[0xFE, b'9' + 1]);
        self.emit(&[0xD2]); // JP NC, pf_row2_done
        self.fixup("pf_row2_done");
        self.emit(&[0xD6, b'0']);
        self.ld_b_a();
        self.ld_a_c();
        self.emit(&[0x87]); // x2
        self.emit(&[0x4F]); // save
        self.emit(&[0x87]); // x4
        self.emit(&[0x87]); // x8
        self.emit(&[0x81]); // x10
        self.emit(&[0x80]); // +digit
        self.ld_c_a();
        self.inc_hl();
        self.emit(&[0xC3]); // JP pf_row2_loop
        self.fixup("pf_row2_loop");
        self.label("pf_row2_done");
        self.ld_a_c();
        self.dec_a(); //0-based)
        self.emit(&[0x32]); // LD (RANGE_ROW2), A (row2)
        self.emit_word(RANGE_ROW2);

        // Check for )
        self.ld_a_hl_ind();
        self.emit(&[0xFE, b')']);
        self.emit(&[0xC2]); // JP NZ, pf_error
        self.fixup("pf_error");
        self.inc_hl();
        self.emit(&[0x22]); // LD (TEMP2), HL (update pointer - overwrites low byte)
        self.emit_word(TEMP2);

        // Initialize accumulators for BCD functions
        // Clear FUNC_BCD (4-byte BCD sum/min/max accumulator)
        self.emit(&[0x21]); // LD HL, FUNC_BCD
        self.emit_word(FUNC_BCD);
        self.emit(&[0xCD]); // CALL bcd_zero
        self.fixup("bcd_zero");
        // Clear count and sign
        self.xor_a();
        self.emit(&[0x32]); // LD (FUNC_COUNT), A
        self.emit_word(FUNC_COUNT);
        self.emit(&[0x32]); // LD (FUNC_COUNT+1), A
        self.emit_word(FUNC_COUNT + 1);
        self.emit(&[0x32]); // LD (FUNC_SIGN), A (accumulator is positive)
        self.emit_word(FUNC_SIGN);

        // For MIN, initialize FUNC_BCD to max BCD value (99999999)
        self.emit(&[0x3A]); // LD A, (FUNC_TYPE)
        self.emit_word(FUNC_TYPE);
        self.emit(&[0xFE, 0x02]); // CP 2 (MIN)
        self.emit(&[0xC2]); // JP NZ, pf_init_done
        self.fixup("pf_init_done");
        // Set FUNC_BCD to 99 99 99 99 (max BCD value)
        self.emit(&[0x21]); // LD HL, FUNC_BCD
        self.emit_word(FUNC_BCD);
        self.emit(&[0x3E, 0x99]); // LD A, 0x99
        self.emit(&[0x77]); // LD (HL), A
        self.inc_hl();
        self.emit(&[0x77]); // LD (HL), A
        self.inc_hl();
        self.emit(&[0x77]); // LD (HL), A
        self.inc_hl();
        self.emit(&[0x77]); // LD (HL), A
        self.label("pf_init_done");

        // Initialize current column = col1
        self.emit(&[0x3A]); // LD A, (TEMP1) (col1)
        self.emit_word(TEMP1);
        self.emit(&[0x32]); // LD (RANGE_CUR_COL), A
        self.emit_word(RANGE_CUR_COL);

        // Outer loop: columns
        self.label("pf_col_loop");
        // C = row1 (reset for each column)
        self.emit(&[0x3A]); // LD A, (TEMP1+1) (row1)
        self.emit_word(TEMP1 + 1);
        self.ld_c_a();

        // Inner loop: rows
        self.label("pf_row_loop");
        // Get cell value at (current_col, C)
        self.emit(&[0x3A]); // LD A, (RANGE_CUR_COL)
        self.emit_word(RANGE_CUR_COL);
        self.ld_b_a(); // col
        self.push_bc(); // save row counter (C) and col (B)
        self.emit(&[0xCD]); // CALL get_cell_addr
        self.fixup("get_cell_addr");
        // HL = cell addr
        self.ld_a_hl_ind(); // type
        self.emit(&[0xFE, CELL_NUMBER]); // CP CELL_NUMBER
        self.emit(&[0xCA]); // JP Z, pf_is_number
        self.fixup("pf_is_number");
        self.emit(&[0xFE, CELL_FORMULA]); // CP CELL_FORMULA
        self.emit(&[0xCA]); // JP Z, pf_is_formula
        self.fixup("pf_is_formula");
        // Not a number or formula - skip
        self.emit(&[0xC3]); // JP pf_skip
        self.fixup("pf_skip");

        // Handle formula cell - get BCD value from formula storage
        self.label("pf_is_formula");
        self.inc_hl();
        self.inc_hl();
        self.emit(&[0x5E]); // LD E, (HL) - get formula pointer low
        self.inc_hl();
        self.emit(&[0x56]); // LD D, (HL) - get formula pointer high
        self.ex_de_hl(); // HL = formula pointer
        // Scan to end of formula string
        self.label("pf_scan_formula");
        self.ld_a_hl_ind();
        self.inc_hl();
        self.or_a_a();
        self.emit(&[0xC2]); // JP NZ, pf_scan_formula
        self.fixup("pf_scan_formula");
        // HL now points to sign byte after null terminator
        self.ld_a_hl_ind(); // read sign
        self.emit(&[0x32]); // LD (FUNC_SIGN2), A
        self.emit_word(FUNC_SIGN2);
        self.inc_hl(); // HL now points to BCD value
        self.emit(&[0xC3]); // JP pf_read_bcd
        self.fixup("pf_read_bcd");

        // Handle number cell - BCD is at bytes 2-5
        self.label("pf_is_number");
        self.inc_hl(); // skip type
        self.ld_a_hl_ind(); // read sign byte
        self.emit(&[0x32]); // LD (FUNC_SIGN2), A
        self.emit_word(FUNC_SIGN2);
        self.inc_hl(); // HL now points to BCD data

        // Common code to read BCD value (HL points to BCD data)
        self.label("pf_read_bcd");
        // Found a value - increment count
        self.push_hl(); // save BCD addr
        self.emit(&[0x2A]); // LD HL, (FUNC_COUNT)
        self.emit_word(FUNC_COUNT);
        self.inc_hl();
        self.emit(&[0x22]); // LD (FUNC_COUNT), HL
        self.emit_word(FUNC_COUNT);
        self.pop_hl(); // restore BCD addr

        // Copy 4-byte BCD to FUNC_BCD2
        self.emit(&[0x11]); // LD DE, FUNC_BCD2
        self.emit_word(FUNC_BCD2);
        self.emit(&[0x06, 4]); // LD B, 4
        self.label("pf_copy_bcd");
        self.ld_a_hl_ind();
        self.emit(&[0x12]); // LD (DE), A
        self.inc_hl();
        self.inc_de();
        self.emit(&[0x10]); // DJNZ pf_copy_bcd
        self.emit_relative("pf_copy_bcd");
        // FUNC_BCD2 now has the cell's BCD value

        // Check function type for SUM/AVG vs MIN/MAX
        self.emit(&[0x3A]); // LD A, (FUNC_TYPE)
        self.emit_word(FUNC_TYPE);
        self.emit(&[0xFE, 0x02]); // CP 2 (MIN)
        self.emit(&[0xCA]); // JP Z, pf_do_min
        self.fixup("pf_do_min");
        self.emit(&[0xFE, 0x03]); // CP 3 (MAX)
        self.emit(&[0xCA]); // JP Z, pf_do_max
        self.fixup("pf_do_max");

        // SUM/AVG/COUNT: signed add FUNC_BCD2 to FUNC_BCD
        // Set up for eval_add: FUNC_BCD → BCD_TEMP2, FUNC_BCD2 → BCD_TEMP1
        self.pop_bc(); // restore row counter
        self.push_bc(); // save it again for after eval_add

        // Copy FUNC_BCD to BCD_TEMP2 (accumulator to temp)
        // bcd_copy copies from (DE) to (HL)
        self.emit(&[0x21]); // LD HL, BCD_TEMP2 (dest)
        self.emit_word(BCD_TEMP2);
        self.emit(&[0x11]); // LD DE, FUNC_BCD (src)
        self.emit_word(FUNC_BCD);
        self.emit(&[0xCD]); // CALL bcd_copy
        self.fixup("bcd_copy");

        // Copy FUNC_BCD2 to BCD_TEMP1 (operand to temp)
        self.emit(&[0x21]); // LD HL, BCD_TEMP1 (dest)
        self.emit_word(BCD_TEMP1);
        self.emit(&[0x11]); // LD DE, FUNC_BCD2 (src)
        self.emit_word(FUNC_BCD2);
        self.emit(&[0xCD]); // CALL bcd_copy
        self.fixup("bcd_copy");

        // Copy signs: FUNC_SIGN → SIGN_ACCUM, FUNC_SIGN2 → SIGN_OP
        self.emit(&[0x3A]); // LD A, (FUNC_SIGN)
        self.emit_word(FUNC_SIGN);
        self.emit(&[0x32]); // LD (SIGN_ACCUM), A
        self.emit_word(SIGN_ACCUM);
        self.emit(&[0x3A]); // LD A, (FUNC_SIGN2)
        self.emit_word(FUNC_SIGN2);
        self.emit(&[0x32]); // LD (SIGN_OP), A
        self.emit_word(SIGN_OP);

        // Call signed addition (result in BCD_TEMP1, sign in SIGN_ACCUM)
        self.emit(&[0xCD]); // CALL signed_add
        self.fixup("signed_add");

        // Copy result back: BCD_TEMP1 → FUNC_BCD, SIGN_ACCUM → FUNC_SIGN
        // bcd_copy copies from (DE) to (HL)
        self.emit(&[0x21]); // LD HL, FUNC_BCD (dest)
        self.emit_word(FUNC_BCD);
        self.emit(&[0x11]); // LD DE, BCD_TEMP1 (src)
        self.emit_word(BCD_TEMP1);
        self.emit(&[0xCD]); // CALL bcd_copy
        self.fixup("bcd_copy");
        self.emit(&[0x3A]); // LD A, (SIGN_ACCUM)
        self.emit_word(SIGN_ACCUM);
        self.emit(&[0x32]); // LD (FUNC_SIGN), A
        self.emit_word(FUNC_SIGN);

        self.pop_bc(); // restore row counter
        self.emit(&[0xC3]); // JP pf_next
        self.fixup("pf_next");

        // MIN: if FUNC_BCD2 < FUNC_BCD, update FUNC_BCD
        self.label("pf_do_min");
        self.pop_bc(); // restore row counter
        // bcd_cmp returns C if (DE) < (HL), so check if FUNC_BCD2 < FUNC_BCD
        self.emit(&[0x21]); // LD HL, FUNC_BCD
        self.emit_word(FUNC_BCD);
        self.emit(&[0x11]); // LD DE, FUNC_BCD2
        self.emit_word(FUNC_BCD2);
        self.emit(&[0xCD]); // CALL bcd_cmp
        self.fixup("bcd_cmp");
        self.emit(&[0xD2]); // JP NC, pf_next (FUNC_BCD2 >= FUNC_BCD, don't update)
        self.fixup("pf_next");
        // FUNC_BCD2 < FUNC_BCD, copy FUNC_BCD2 to FUNC_BCD and sign
        self.emit(&[0x21]); // LD HL, FUNC_BCD
        self.emit_word(FUNC_BCD);
        self.emit(&[0x11]); // LD DE, FUNC_BCD2
        self.emit_word(FUNC_BCD2);
        self.emit(&[0xCD]); // CALL bcd_copy
        self.fixup("bcd_copy");
        // Copy sign too
        self.emit(&[0x3A]); // LD A, (FUNC_SIGN2)
        self.emit_word(FUNC_SIGN2);
        self.emit(&[0x32]); // LD (FUNC_SIGN), A
        self.emit_word(FUNC_SIGN);
        self.emit(&[0xC3]); // JP pf_next
        self.fixup("pf_next");

        // MAX: if FUNC_BCD2 > FUNC_BCD, update FUNC_BCD
        self.label("pf_do_max");
        self.pop_bc(); // restore row counter
        // bcd_cmp returns C if (DE) < (HL), so check if FUNC_BCD < FUNC_BCD2 (i.e., FUNC_BCD2 > FUNC_BCD)
        self.emit(&[0x21]); // LD HL, FUNC_BCD2
        self.emit_word(FUNC_BCD2);
        self.emit(&[0x11]); // LD DE, FUNC_BCD
        self.emit_word(FUNC_BCD);
        self.emit(&[0xCD]); // CALL bcd_cmp
        self.fixup("bcd_cmp");
        self.emit(&[0xD2]); // JP NC, pf_next (FUNC_BCD >= FUNC_BCD2, don't update)
        self.fixup("pf_next");
        // FUNC_BCD < FUNC_BCD2, so FUNC_BCD2 is larger - copy FUNC_BCD2 to FUNC_BCD and sign
        self.emit(&[0x21]); // LD HL, FUNC_BCD
        self.emit_word(FUNC_BCD);
        self.emit(&[0x11]); // LD DE, FUNC_BCD2
        self.emit_word(FUNC_BCD2);
        self.emit(&[0xCD]); // CALL bcd_copy
        self.fixup("bcd_copy");
        // Copy sign too
        self.emit(&[0x3A]); // LD A, (FUNC_SIGN2)
        self.emit_word(FUNC_SIGN2);
        self.emit(&[0x32]); // LD (FUNC_SIGN), A
        self.emit_word(FUNC_SIGN);
        self.emit(&[0xC3]); // JP pf_next (skip pf_skip to avoid double BC pop)
        self.fixup("pf_next");

        self.label("pf_skip");
        // Not a number - skip (just restore BC)
        self.pop_bc();

        self.label("pf_next");
        // Increment row first, then check if done with column (C > row2)
        self.inc_c();
        self.ld_a_c(); // current row (after increment)
        self.ld_b_a(); // save in B
        self.emit(&[0x3A]); // LD A, (RANGE_ROW2)
        self.emit_word(RANGE_ROW2);
        self.emit(&[0xB8]); // CP B
        self.emit(&[0xDA]); // JP C, pf_next_col (row2 < current = done with this column)
        self.fixup("pf_next_col");
        self.emit(&[0xC3]); // JP pf_row_loop
        self.fixup("pf_row_loop");

        // Move to next column
        self.label("pf_next_col");
        // Increment column first, then check if done (current_col > col2)
        self.emit(&[0x3A]); // LD A, (RANGE_CUR_COL)
        self.emit_word(RANGE_CUR_COL);
        self.inc_a();
        self.emit(&[0x32]); // LD (RANGE_CUR_COL), A
        self.emit_word(RANGE_CUR_COL);
        self.ld_b_a(); // save incremented value in B
        self.emit(&[0x3A]); // LD A, (RANGE_COL2)
        self.emit_word(RANGE_COL2);
        self.emit(&[0xB8]); // CP B
        self.emit(&[0xDA]); // JP C, pf_done (col2 < current = done)
        self.fixup("pf_done");
        // Continue to next column (already incremented above)
        self.emit(&[0xC3]); // JP pf_col_loop
        self.fixup("pf_col_loop");

        // Return result based on function type
        // Result must go in BCD_TEMP1 for consistency with parse_operand
        self.label("pf_done");
        self.emit(&[0x3A]); // LD A, (FUNC_TYPE)
        self.emit_word(FUNC_TYPE);

        // SUM (0): copy FUNC_BCD to BCD_TEMP1, FUNC_SIGN to TEMP1 (for eval_expr)
        self.or_a_a();
        self.emit(&[0xC2]); // JP NZ, pf_not_sum
        self.fixup("pf_not_sum");
        // bcd_copy copies from (DE) to (HL)
        self.emit(&[0x21]); // LD HL, BCD_TEMP1 (dest)
        self.emit_word(BCD_TEMP1);
        self.emit(&[0x11]); // LD DE, FUNC_BCD (src)
        self.emit_word(FUNC_BCD);
        self.emit(&[0xCD]); // CALL bcd_copy
        self.fixup("bcd_copy");
        // Copy sign to TEMP1 (where eval_expr expects it)
        self.emit(&[0x3A]); // LD A, (FUNC_SIGN)
        self.emit_word(FUNC_SIGN);
        self.emit(&[0x32]); // LD (TEMP1), A
        self.emit_word(TEMP1);
        self.or_a_a(); // clear carry
        self.ret();

        // AVG (1): FUNC_BCD / count -> BCD_TEMP1
        self.label("pf_not_sum");
        self.emit(&[0xFE, 0x01]); // CP 1
        self.emit(&[0xC2]); // JP NZ, pf_not_avg
        self.fixup("pf_not_avg");
        // Copy FUNC_BCD to BCD_TEMP1 (dividend)
        self.emit(&[0x21]); // LD HL, BCD_TEMP1
        self.emit_word(BCD_TEMP1);
        self.emit(&[0x11]); // LD DE, FUNC_BCD
        self.emit_word(FUNC_BCD);
        self.emit(&[0xCD]); // CALL bcd_copy
        self.fixup("bcd_copy");
        // Convert count to BCD in BCD_TEMP2
        self.emit(&[0x2A]); // LD HL, (FUNC_COUNT)
        self.emit_word(FUNC_COUNT);
        // Check for divide by zero
        self.emit(&[0x7C]); // LD A, H
        self.emit(&[0xB5]); // OR L
        self.emit(&[0xC2]); // JP NZ, pf_avg_div
        self.fixup("pf_avg_div");
        // Division by zero - zero the result (positive)
        self.emit(&[0x21]); // LD HL, BCD_TEMP1
        self.emit_word(BCD_TEMP1);
        self.emit(&[0xCD]); // CALL bcd_zero
        self.fixup("bcd_zero");
        self.xor_a();
        self.emit(&[0x32]); // LD (TEMP1), A (positive)
        self.emit_word(TEMP1);
        self.or_a_a();
        self.ret();
        self.label("pf_avg_div");
        // For AVG: divide sum by count (no ×100 scaling needed)
        // Convert count (in L) to BCD and store in BCD_TEMP2 byte 3 (LSB)
        self.emit(&[0x7D]); // LD A, L (count, assuming < 100)
        // Convert to BCD: tens in high nibble, ones in low nibble
        self.emit(&[0x06, 0x00]); // LD B, 0 (tens counter)
        self.label("pf_cvt_tens");
        self.emit(&[0xFE, 10]); // CP 10
        self.emit(&[0xDA]); // JP C, pf_cvt_done (< 10)
        self.fixup("pf_cvt_done");
        self.emit(&[0xD6, 10]); // SUB 10
        self.inc_b();
        self.emit(&[0xC3]); // JP pf_cvt_tens
        self.fixup("pf_cvt_tens");
        self.label("pf_cvt_done");
        // A = ones, B = tens
        self.emit(&[0x4F]); // LD C, A (ones)
        self.ld_a_b(); // tens
        self.emit(&[0x07]); // RLCA ×4
        self.emit(&[0x07]);
        self.emit(&[0x07]);
        self.emit(&[0x07]);
        self.emit(&[0xB1]); // OR C
        // A = BCD of count, store in BCD_TEMP2 byte 3 (LSB)
        self.push_af(); // save BCD count
        self.emit(&[0x21]); // LD HL, BCD_TEMP2
        self.emit_word(BCD_TEMP2);
        self.emit(&[0xCD]); // CALL bcd_zero
        self.fixup("bcd_zero");
        self.pop_af();
        self.emit(&[0x21]); // LD HL, BCD_TEMP2+3 (LSB)
        self.emit_word(BCD_TEMP2 + 3);
        self.emit(&[0x77]); // LD (HL), A
        // BCD_TEMP2 = count as BCD (e.g., 3 -> 00 00 00 03)
        // Call bcd_div_noscale: BCD_TEMP1 / BCD_TEMP2 -> BCD_TEMP1 (no ×100)
        self.emit(&[0xCD]); // CALL bcd_div_noscale
        self.fixup("bcd_div_noscale");
        // Copy sign to TEMP1 (AVG sign = SUM sign since count is positive)
        self.emit(&[0x3A]); // LD A, (FUNC_SIGN)
        self.emit_word(FUNC_SIGN);
        self.emit(&[0x32]); // LD (TEMP1), A
        self.emit_word(TEMP1);
        self.or_a_a();
        self.ret();

        // MIN (2) or MAX (3): copy FUNC_BCD to BCD_TEMP1
        self.label("pf_not_avg");
        self.emit(&[0xFE, 0x02]); // CP 2
        self.emit(&[0xCA]); // JP Z, pf_ret_bcd
        self.fixup("pf_ret_bcd");
        self.emit(&[0xFE, 0x03]); // CP 3
        self.emit(&[0xCA]); // JP Z, pf_ret_bcd
        self.fixup("pf_ret_bcd");

        // COUNT (4): convert count to BCD in BCD_TEMP1
        self.emit(&[0x2A]); // LD HL, (FUNC_COUNT)
        self.emit_word(FUNC_COUNT);
        // Convert to BCD (same as above, but put in byte 2 for display as X.00)
        self.emit(&[0x7D]); // LD A, L
        self.emit(&[0x06, 0x00]); // LD B, 0 (tens)
        self.label("pf_cnt_cvt");
        self.emit(&[0xFE, 10]); // CP 10
        self.emit(&[0xDA]); // JP C, pf_cnt_done
        self.fixup("pf_cnt_done");
        self.emit(&[0xD6, 10]); // SUB 10
        self.inc_b();
        self.emit(&[0xC3]); // JP pf_cnt_cvt
        self.fixup("pf_cnt_cvt");
        self.label("pf_cnt_done");
        self.emit(&[0x4F]); // LD C, A (ones)
        self.ld_a_b();
        self.emit(&[0x07]); // RLCA ×4
        self.emit(&[0x07]);
        self.emit(&[0x07]);
        self.emit(&[0x07]);
        self.emit(&[0xB1]); // OR C
        // A = BCD of count, store as count.00
        self.push_af();
        self.emit(&[0x21]); // LD HL, BCD_TEMP1
        self.emit_word(BCD_TEMP1);
        self.emit(&[0xCD]); // CALL bcd_zero
        self.fixup("bcd_zero");
        self.pop_af();
        self.emit(&[0x21]); // LD HL, BCD_TEMP1+2
        self.emit_word(BCD_TEMP1 + 2);
        self.emit(&[0x77]); // LD (HL), A
        // COUNT is always positive
        self.xor_a();
        self.emit(&[0x32]); // LD (TEMP1), A
        self.emit_word(TEMP1);
        self.or_a_a();
        self.ret();

        // pf_ret_bcd: copy FUNC_BCD to BCD_TEMP1 for MIN/MAX result
        self.label("pf_ret_bcd");
        // bcd_copy copies from (DE) to (HL)
        self.emit(&[0x21]); // LD HL, BCD_TEMP1 (dest)
        self.emit_word(BCD_TEMP1);
        self.emit(&[0x11]); // LD DE, FUNC_BCD (src)
        self.emit_word(FUNC_BCD);
        self.emit(&[0xCD]); // CALL bcd_copy
        self.fixup("bcd_copy");
        // Copy sign to TEMP1 for MIN/MAX result
        self.emit(&[0x3A]); // LD A, (FUNC_SIGN)
        self.emit_word(FUNC_SIGN);
        self.emit(&[0x32]); // LD (TEMP1), A
        self.emit_word(TEMP1);
        self.or_a_a();
        self.ret();

        // 16-bit division (legacy, may be unused): HL / DE -> HL (quotient)
        self.label("div16");
        self.emit(&[0x01, 0x00, 0x00]); // LD BC, 0 (quotient)
        self.label("div16_loop");
        self.or_a_a();
        self.emit(&[0xED, 0x52]); // SBC HL, DE
        self.emit(&[0xDA]); // JP C, div16_done
        self.fixup("div16_done");
        self.emit(&[0x03]); // INC BC
        self.emit(&[0xC3]); // JP div16_loop
        self.fixup("div16_loop");
        self.label("div16_done");
        self.add_hl_de(); //restore)
        self.emit(&[0x60]); // LD H, B
        self.emit(&[0x69]); // LD L, C
        self.ret();

        self.label("pf_error");
        self.emit(&[0x21, 0x00, 0x00]); // LD HL, 0
        self.emit(&[0x37]); // SCF (set carry = error)
        self.ret();
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
        self.ret();

        // Put character to output
        // MC6850: bit 1 of status = TX ready
        self.label("putchar");
        self.push_af(); // save char
        self.label("putchar_wait");
        self.emit(&[0xDB, 0x80]); // IN A, (0x80) - status
        self.emit(&[0xE6, 0x02]); // AND 0x02 - TX ready bit
        self.emit(&[0x28, 0xFA]); // JR Z, putchar_wait (-6)
        self.pop_af(); // restore char
        self.emit(&[0xD3, 0x81]); // OUT (0x81), A - data
        self.ret();

        // Print newline
        self.label("newline");
        self.emit(&[0x3E, 0x0D]); // LD A, CR
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0x3E, 0x0A]); // LD A, LF
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.ret();

        // Convert 16-bit integer in HL to string in INPUT_BUF
        // Sets INPUT_LEN and INPUT_POS
        // Uses TEMP1 for offset, TEMP1+1 for digit count
        self.label("int_to_str");
        self.xor_a();
        self.emit(&[0x32]); // LD (TEMP1), A  ; offset = 0
        self.emit_word(TEMP1);
        self.emit(&[0x32]); // LD (TEMP1+1), A  ; digit count = 0
        self.emit_word(TEMP1 + 1);

        // Check if negative
        self.emit(&[0x7C]); // LD A, H
        self.or_a_a();
        self.emit(&[0xF2]); // JP P, int_to_str_pos
        self.fixup("int_to_str_pos");
        // Negative - store minus and negate
        self.emit(&[0x3E, b'-']); // LD A, '-'
        self.emit(&[0x32]); // LD (INPUT_BUF), A
        self.emit_word(INPUT_BUF);
        self.emit(&[0x3E, 0x01]); // LD A, 1
        self.emit(&[0x32]); // LD (TEMP1), A  ; offset = 1
        self.emit_word(TEMP1);
        // Negate HL
        self.emit(&[0x7C]); // LD A, H
        self.cpl();
        self.emit(&[0x67]); // LD H, A
        self.emit(&[0x7D]); // LD A, L
        self.cpl();
        self.emit(&[0x6F]); // LD L, A
        self.inc_hl();

        self.label("int_to_str_pos");
        // Extract digits in reverse order onto stack
        self.label("int_to_str_extract");
        // Divide HL by 10
        self.emit(&[0x11]); // LD DE, 10
        self.emit_word(10);
        self.emit(&[0x01, 0x00, 0x00]); // LD BC, 0 (quotient)
        self.label("int_to_str_div");
        self.or_a_a();
        self.emit(&[0xED, 0x52]); // SBC HL, DE
        self.emit(&[0xDA]); // JP C, int_to_str_div_done
        self.fixup("int_to_str_div_done");
        self.emit(&[0x03]); // INC BC
        self.emit(&[0xC3]); // JP int_to_str_div
        self.fixup("int_to_str_div");
        self.label("int_to_str_div_done");
        self.add_hl_de(); //restore remainder)
        // L = remainder (digit 0-9), BC = quotient
        self.emit(&[0x7D]); // LD A, L
        self.emit(&[0xC6, b'0']); // ADD A, '0'
        self.push_af(); //save digit)
        // Increment digit count
        self.emit(&[0x3A]); // LD A, (TEMP1+1)
        self.emit_word(TEMP1 + 1);
        self.inc_a();
        self.emit(&[0x32]); // LD (TEMP1+1), A
        self.emit_word(TEMP1 + 1);
        // HL = quotient, check if zero
        self.emit(&[0x60]); // LD H, B
        self.emit(&[0x69]); // LD L, C
        self.emit(&[0x7C]); // LD A, H
        self.or_l();
        self.emit(&[0xC2]); // JP NZ, int_to_str_extract
        self.fixup("int_to_str_extract");

        // Pop digits and store in INPUT_BUF
        // DE = INPUT_BUF + offset
        self.emit(&[0x3A]); // LD A, (TEMP1)
        self.emit_word(TEMP1);
        self.ld_e_a();
        self.emit(&[0x16, 0x00]); // LD D, 0
        self.emit(&[0x21]); // LD HL, INPUT_BUF
        self.emit_word(INPUT_BUF);
        self.add_hl_de(); //HL = output ptr)
        // B = digit count
        self.emit(&[0x3A]); // LD A, (TEMP1+1)
        self.emit_word(TEMP1 + 1);
        self.ld_b_a();
        self.label("int_to_str_pop");
        self.pop_af();
        self.ld_hl_ind_a();
        self.inc_hl();
        self.emit(&[0x10]); // DJNZ int_to_str_pop
        let offset = self.rom().len();
        self.emit(&[0x00]); // placeholder
        self.rom_mut()[offset] = (self.get_label("int_to_str_pop").unwrap_or(0)
            .wrapping_sub(self.pos())) as u8;

        // Set INPUT_LEN = offset + digit count
        self.emit(&[0x3A]); // LD A, (TEMP1)
        self.emit_word(TEMP1);
        self.ld_b_a();
        self.emit(&[0x3A]); // LD A, (TEMP1+1)
        self.emit_word(TEMP1 + 1);
        self.emit(&[0x80]); // ADD A, B
        self.emit(&[0x32]); // LD (INPUT_LEN), A
        self.emit_word(INPUT_LEN);
        self.emit(&[0x32]); // LD (INPUT_POS), A
        self.emit_word(INPUT_POS);
        self.ret();

        // === VT220/ANSI Escape Sequence Routines ===

        // Clear screen: ESC[2J ESC[H
        self.label("clear_screen");
        self.emit(&[0x3E, 0x1B]); // LD A, ESC
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0x3E, b'[']); // LD A, '['
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0x3E, b'2']); // LD A, '2'
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0x3E, b'J']); // LD A, 'J'
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        // Fall through to cursor_home

        // Cursor home: ESC[H
        self.label("cursor_home");
        self.emit(&[0x3E, 0x1B]); // LD A, ESC
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0x3E, b'[']); // LD A, '['
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0x3E, b'H']); // LD A, 'H'
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.ret();

        // Cursor position: ESC[row;colH  (B=row 1-based, C=col 1-based)
        self.label("cursor_pos");
        self.emit(&[0x3E, 0x1B]); // LD A, ESC
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0x3E, b'[']); // LD A, '['
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.ld_a_b(); //row)
        self.emit(&[0xCD]); // CALL print_byte_dec
        self.fixup("print_byte_dec");
        self.emit(&[0x3E, b';']); // LD A, ';'
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.ld_a_c(); //col)
        self.emit(&[0xCD]); // CALL print_byte_dec
        self.fixup("print_byte_dec");
        self.emit(&[0x3E, b'H']); // LD A, 'H'
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.ret();

        // Clear to end of line: ESC[K
        self.label("clear_to_eol");
        self.emit(&[0x3E, 0x1B]); // LD A, ESC
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0x3E, b'[']); // LD A, '['
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0x3E, b'K']); // LD A, 'K'
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.ret();

        // Hide cursor: ESC[?25l
        self.label("cursor_hide");
        self.emit(&[0x3E, 0x1B]); // LD A, ESC
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0x3E, b'[']); // LD A, '['
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0x3E, b'?']); // LD A, '?'
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0x3E, b'2']); // LD A, '2'
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0x3E, b'5']); // LD A, '5'
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0x3E, b'l']); // LD A, 'l'
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.ret();

        // Show cursor: ESC[?25h
        self.label("cursor_show");
        self.emit(&[0x3E, 0x1B]); // LD A, ESC
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0x3E, b'[']); // LD A, '['
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0x3E, b'?']); // LD A, '?'
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0x3E, b'2']); // LD A, '2'
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0x3E, b'5']); // LD A, '5'
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0x3E, b'h']); // LD A, 'h'
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.ret();

        // Print byte in A as decimal (1-255, no leading zeros)
        self.label("print_byte_dec");
        self.push_af();
        self.emit(&[0xFE, 100]); // CP 100
        self.emit(&[0xDA]); // JP C, pbd_tens (skip hundreds if < 100)
        self.fixup("pbd_tens");
        // Print hundreds digit (value >= 100)
        self.emit(&[0x06, 0x00]); // LD B, 0
        self.label("pbd_hundreds_loop");
        self.emit(&[0xD6, 100]); // SUB 100
        self.inc_b();
        self.emit(&[0xFE, 100]); // CP 100
        self.emit(&[0xD2]); // JP NC, pbd_hundreds_loop
        self.fixup("pbd_hundreds_loop");
        self.push_af(); //save remainder)
        self.ld_a_b();
        self.emit(&[0xC6, b'0']); // ADD A, '0'
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.pop_af();
        self.emit(&[0xC3]); // JP pbd_tens_force (must print tens after hundreds)
        self.fixup("pbd_tens_force");

        self.label("pbd_tens");
        self.emit(&[0xFE, 10]); // CP 10
        self.emit(&[0xDA]); // JP C, pbd_ones (skip tens if < 10)
        self.fixup("pbd_ones");
        self.label("pbd_tens_force");
        self.emit(&[0x06, 0x00]); // LD B, 0
        self.label("pbd_tens_loop");
        self.emit(&[0xD6, 10]); // SUB 10
        self.inc_b();
        self.emit(&[0xFE, 10]); // CP 10
        self.emit(&[0xD2]); // JP NC, pbd_tens_loop
        self.fixup("pbd_tens_loop");
        self.push_af();
        self.ld_a_b();
        self.emit(&[0xC6, b'0']); // ADD A, '0'
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.pop_af();

        self.label("pbd_ones");
        self.emit(&[0xC6, b'0']); // ADD A, '0'
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.pop_af(); //restore original)
        self.ret();

        // Print null-terminated string at HL
        self.label("print_string");
        self.ld_a_hl_ind();
        self.or_a_a();
        self.ret_z();
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.inc_hl();
        self.emit(&[0xC3]); // JP print_string
        self.fixup("print_string");

        // Print 16-bit integer in HL
        self.label("print_int");
        // Check if negative
        self.emit(&[0x7C]); // LD A, H
        self.or_a_a();
        self.emit(&[0xF2]); // JP P, print_int_pos
        self.fixup("print_int_pos");
        // Negative - print minus and negate
        self.emit(&[0x3E, b'-']);
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0x7C]); // LD A, H
        self.cpl();
        self.emit(&[0x67]); // LD H, A
        self.emit(&[0x7D]); // LD A, L
        self.cpl();
        self.emit(&[0x6F]); // LD L, A
        self.inc_hl();

        self.label("print_int_pos");
        // Convert to decimal and print (C = started flag, 0 = no digits yet)
        self.emit(&[0x0E, 0x00]); // LD C, 0 (no digits printed yet)
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
        // Last digit (always print)
        self.emit(&[0x7D]); // LD A, L
        self.emit(&[0xC6, b'0']); // ADD A, '0'
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.ret();

        // Print one digit, HL = value, DE = divisor, C = started flag
        // Updates HL to remainder, C to 1 if digit printed
        self.label("print_digit");
        self.emit(&[0x06, 0x00]); // LD B, 0 (count)
        self.label("print_digit_loop");
        self.or_a_a(); //clear carry)
        self.emit(&[0xED, 0x52]); // SBC HL, DE
        self.emit(&[0xDA]); // JP C, print_digit_done
        self.fixup("print_digit_done");
        self.inc_b();
        self.emit(&[0xC3]); // JP print_digit_loop
        self.fixup("print_digit_loop");
        self.label("print_digit_done");
        self.add_hl_de(); //restore)
        // Check if we should print this digit
        self.ld_a_b();
        self.or_a_a(); //check if B > 0)
        self.emit(&[0xC2]); // JP NZ, print_digit_out (B > 0, print it)
        self.fixup("print_digit_out");
        self.ld_a_c(); //check started flag)
        self.or_a_a();
        self.ret_z(); //C == 0 and B == 0, skip this digit)
        self.ld_a_b(); //B is 0 here)
        self.label("print_digit_out");
        self.emit(&[0x0E, 0x01]); // LD C, 1 (mark as started)
        self.emit(&[0xC6, b'0']); // ADD A, '0'
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.ret();

        // Print integer padded to 4 chars (for row numbers)
        self.label("print_int_padded");
        // For simplicity, just print with leading spaces
        self.emit(&[0x7C]); // LD A, H
        self.or_a_a();
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

        // Print integer in cell (right-aligned in CELL_WIDTH-2 = 7 chars)
        // Input: HL = 16-bit signed value
        self.label("print_int_cell");
        // Calculate number width and print leading spaces
        // B will hold the width needed
        self.emit(&[0x06, 1]); // LD B, 1 (minimum width = 1 digit)

        // Check if negative
        self.emit(&[0x7C]); // LD A, H
        self.or_a_a();
        self.emit(&[0xF2]); // JP P, print_cell_calc_width
        self.fixup("print_cell_calc_width");
        // Negative - add 1 for minus sign
        self.inc_b();
        // Negate for magnitude check (but keep original in HL for later)
        self.push_hl();
        self.emit(&[0x7C]); // LD A, H
        self.cpl();
        self.emit(&[0x67]); // LD H, A
        self.emit(&[0x7D]); // LD A, L
        self.cpl();
        self.emit(&[0x6F]); // LD L, A
        self.inc_hl();
        self.emit(&[0xC3]); // JP print_cell_check_mag
        self.fixup("print_cell_check_mag");

        self.label("print_cell_calc_width");
        self.push_hl(); //save original)

        self.label("print_cell_check_mag");
        // HL = absolute value, B = current width (1 or 2 if negative)
        // Check >= 10
        self.emit(&[0x11]); // LD DE, 10
        self.emit_word(10);
        self.or_a_a();
        self.emit(&[0xED, 0x52]); // SBC HL, DE
        self.emit(&[0xDA]); // JP C, print_cell_do_pad (< 10)
        self.fixup("print_cell_do_pad");
        self.inc_b(); //width++)
        // Check >= 100
        self.emit(&[0x11]); // LD DE, 90 (already subtracted 10)
        self.emit_word(90);
        self.or_a_a();
        self.emit(&[0xED, 0x52]); // SBC HL, DE
        self.emit(&[0xDA]); // JP C, print_cell_do_pad (< 100)
        self.fixup("print_cell_do_pad");
        self.inc_b();
        // Check >= 1000
        self.emit(&[0x11]); // LD DE, 900
        self.emit_word(900);
        self.or_a_a();
        self.emit(&[0xED, 0x52]); // SBC HL, DE
        self.emit(&[0xDA]); // JP C, print_cell_do_pad (< 1000)
        self.fixup("print_cell_do_pad");
        self.inc_b();
        // Check >= 10000
        self.emit(&[0x11]); // LD DE, 9000
        self.emit_word(9000);
        self.or_a_a();
        self.emit(&[0xED, 0x52]); // SBC HL, DE
        self.emit(&[0xDA]); // JP C, print_cell_do_pad (< 10000)
        self.fixup("print_cell_do_pad");
        self.inc_b(); //5 digits)

        self.label("print_cell_do_pad");
        // B = width of number, need to print (CELL_WIDTH-2 - B) spaces
        self.emit(&[0x3E, CELL_WIDTH - 2]); // LD A, CELL_WIDTH-2 (7)
        self.emit(&[0x90]); // SUB B
        self.emit(&[0xDA]); // JP C, print_cell_no_pad (B > 7, no padding)
        self.fixup("print_cell_no_pad");
        self.emit(&[0xCA]); // JP Z, print_cell_no_pad (B == 7)
        self.fixup("print_cell_no_pad");
        // A = number of spaces to print
        self.ld_b_a();
        self.label("print_cell_pad_loop");
        self.emit(&[0x3E, b' ']); // LD A, ' '
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0x10]); // DJNZ print_cell_pad_loop
        let offset = self.rom().len();
        self.emit(&[0x00]); // placeholder
        self.rom_mut()[offset] = (self.get_label("print_cell_pad_loop").unwrap_or(0)
            .wrapping_sub(self.pos())) as u8;

        self.label("print_cell_no_pad");
        self.pop_hl(); //restore original value)
        self.emit(&[0xC3]); // JP print_int
        self.fixup("print_int");

        // Print BCD value from INPUT_BUF (right-aligned in CELL_WIDTH-2 = 7 chars)
        // INPUT_BUF contains "XXXXXX.XX" (9 chars: 6 whole + '.' + 2 frac)
        // Skip leading zeros in whole part (positions 0-4), keep at least pos 5
        // Minimum display: "X.XX" (4 chars)
        // print_bcd_cell_signed: Print BCD with sign support
        // Input: C = sign (0x00 positive, 0x80 negative), ASCII in INPUT_BUF
        self.label("print_bcd_cell_signed");
        self.ld_a_c();
        self.or_a_a();
        self.emit(&[0xCA]); // JP Z, print_bcd_cell (positive)
        self.fixup("print_bcd_cell");
        // Negative - need to handle minus sign
        // Scan for leading zeros first
        self.emit(&[0x21]); // LD HL, INPUT_BUF
        self.emit_word(INPUT_BUF);
        self.emit(&[0x06, 5]); // LD B, 5
        self.label("skip_zeros_neg");
        self.ld_a_hl_ind();
        self.emit(&[0xFE, b'0']);
        self.emit(&[0xC2]); // JP NZ, skip_zeros_neg_done
        self.fixup("skip_zeros_neg_done");
        self.inc_hl();
        self.emit(&[0x10]); // DJNZ
        self.emit_relative("skip_zeros_neg");
        self.label("skip_zeros_neg_done");
        // Calculate chars: 4 + B
        self.ld_a_b();
        self.emit(&[0xC6, 4]); // ADD A, 4
        self.inc_a(); // +1 for minus sign
        self.ld_b_a(); // B = total length with minus
        // Padding: CELL_WIDTH-2 - length
        self.emit(&[0x3E, CELL_WIDTH - 2]); // LD A, 7
        self.emit(&[0x90]); // SUB B
        self.emit(&[0xDA]); // JP C, print_neg_no_pad
        self.fixup("print_neg_no_pad");
        self.emit(&[0xCA]); // JP Z, print_neg_no_pad
        self.fixup("print_neg_no_pad");
        // Print padding
        self.push_hl();
        self.ld_b_a();
        self.label("print_neg_pad");
        self.emit(&[0x3E, b' ']);
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0x10]); // DJNZ
        self.emit_relative("print_neg_pad");
        self.pop_hl();
        self.label("print_neg_no_pad");
        // Print minus sign
        self.emit(&[0x3E, b'-']);
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        // Print digits
        self.emit(&[0xCD]); // CALL print_string
        self.fixup("print_string");
        self.ret();

        self.label("print_bcd_cell");
        // Scan INPUT_BUF positions 0-4 for leading zeros
        self.emit(&[0x21]); // LD HL, INPUT_BUF
        self.emit_word(INPUT_BUF);
        self.emit(&[0x06, 5]); // LD B, 5 (max zeros to skip in positions 0-4)
        self.label("skip_zeros_loop");
        self.ld_a_hl_ind();
        self.emit(&[0xFE, b'0']); // CP '0'
        self.emit(&[0xC2]); // JP NZ, skip_zeros_done (found non-zero)
        self.fixup("skip_zeros_done");
        self.inc_hl();
        self.emit(&[0x10]); // DJNZ skip_zeros_loop
        self.emit_relative("skip_zeros_loop");
        // If we get here, positions 0-4 were all zeros, HL points to position 5

        self.label("skip_zeros_done");
        // HL points to first significant digit (or position 5 if all zeros)
        // Calculate chars to print: 9 - skipped = 9 - (5 - B) = 4 + B
        self.ld_a_b();
        self.emit(&[0xC6, 4]); // ADD A, 4 = chars to print
        self.ld_b_a(); // B = length of number to print
        // Calculate padding: CELL_WIDTH-2 - length
        self.emit(&[0x3E, CELL_WIDTH - 2]); // LD A, 7
        self.emit(&[0x90]); // SUB B
        self.emit(&[0xDA]); // JP C, print_bcd_no_pad (length > 7)
        self.fixup("print_bcd_no_pad");
        self.emit(&[0xCA]); // JP Z, print_bcd_no_pad (length == 7)
        self.fixup("print_bcd_no_pad");
        // A = padding spaces needed
        self.push_hl(); // save start of significant digits
        self.ld_b_a();
        self.label("print_bcd_pad_loop");
        self.emit(&[0x3E, b' ']); // LD A, ' '
        self.emit(&[0xCD]); // CALL putchar
        self.fixup("putchar");
        self.emit(&[0x10]); // DJNZ
        self.emit_relative("print_bcd_pad_loop");
        self.pop_hl();
        self.emit(&[0xC3]); // JP print_bcd_digits
        self.fixup("print_bcd_digits");

        self.label("print_bcd_no_pad");
        // No padding needed, HL already points to start

        self.label("print_bcd_digits");
        // Print the number from HL (first significant digit)
        self.emit(&[0xCD]); // CALL print_string
        self.fixup("print_string");
        self.ret();
    }

    /// String constants
    fn emit_strings(&mut self) {
        self.label("welcome_msg");
        self.emit_string("kz80_calc v0.1\r\n");

        self.label("title_str");
        self.emit_string("kz80_calc v0.1 - Z80 Spreadsheet");

        self.label("help_str");
        self.emit_string("Arrows:move  Enter:edit  /:cmd  !:recalc  q:quit");

        self.label("cmd_help_str");
        self.emit_string("/G:go /C:clr /R:cpy /-:fil /W:wid /Q:q");

        self.label("goto_prompt");
        self.emit_string("Goto cell (e.g. B5): ");

        self.label("repeat_prompt");
        self.emit_string("Fill char: ");

        self.label("copy_to_prompt");
        self.emit_string("Copy to (e.g. B5): ");

        self.label("width_prompt");
        self.emit_string("Width (5-15): ");

        self.label("quit_msg");
        self.emit_string("\r\nGoodbye!\r\n");

        self.label("error_str");
        self.emit_string(" #ERR ");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate() {
        let mut codegen = SpreadsheetCodeGen::new();
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
        // Cell (1,0) should be at CELL_DATA + 6
        // Cell (0,1) should be at CELL_DATA + 96
        // Formula: CELL_DATA + (row * 16 + col) * 6
        let base = CELL_DATA;
        assert_eq!(base + (0 * 16 + 0) * 6, 0x2000);
        assert_eq!(base + (0 * 16 + 1) * 6, 0x2006);
        assert_eq!(base + (1 * 16 + 0) * 6, 0x2060);
    }
}
