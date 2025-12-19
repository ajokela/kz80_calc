//! Z80 code generator for kz80_calc spreadsheet
//!
//! Built on the retroshield-z80 framework.
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

use std::ops::{Deref, DerefMut};
use retroshield_z80_workbench::CodeGen;

/// Memory constants
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
const FORMULA_PTR: u16 = 0x35FC;    // Next free position in formula storage
const RECALC_FLAG: u16 = 0x35FE;    // Force recalculation flag
const COL_WIDTH_VAR: u16 = 0x35FF;  // Column width (default 9)
const RANGE_ROW2: u16 = 0x3600;     // Range function end row
const FUNC_TYPE: u16 = 0x3601;      // Function type: 0=SUM, 1=AVG, 2=MIN, 3=MAX, 4=COUNT
const FUNC_COUNT: u16 = 0x3602;     // Cell count for AVG
const FUNC_MINMAX: u16 = 0x3604;    // Min/max accumulator (16-bit)

// Display constants
const SCREEN_COLS: u8 = 80;         // Terminal width
const SCREEN_ROWS: u8 = 24;         // Terminal height
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
const ROW_NUM_WIDTH: u8 = 5;        // Width for row numbers (space + 4 digits)

// Grid size
const GRID_COLS: u8 = 16;           // A-P
const GRID_ROWS: u8 = 64;           // 1-64

// Cell types
const CELL_EMPTY: u8 = 0;
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
        self.ld_bc(4096); // 1024 cells × 4 bytes
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
        // Store new value (HL) at (DE)
        self.ex_de_hl(); //HL = storage ptr, DE = value)
        self.emit(&[0x73]); // LD (HL), E
        self.inc_hl();
        self.emit(&[0x72]); // LD (HL), D

        // Restore cell pointer high byte position
        self.pop_hl();

        self.label("recalc_next");
        self.pop_de(); //restore counter)
        self.pop_hl(); //restore cell pointer)
        // Move to next cell (4 bytes)
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
        // Get value from bytes 2-3
        self.inc_hl();
        self.inc_hl();
        self.emit(&[0x5E]); // LD E, (HL)
        self.inc_hl();
        self.emit(&[0x56]); // LD D, (HL)
        self.ex_de_hl();
        // HL = value, print right-aligned
        self.emit(&[0xCD]); // CALL print_int_cell
        self.fixup("print_int_cell");
        self.ret();

        self.label("print_cell_error");
        self.emit(&[0x21]); // LD HL, error_str
        self.fixup("error_str");
        self.emit(&[0xCD]); // CALL print_string
        self.fixup("print_string");
        self.ret();

        // Formula cell - get pointer and read calculated value
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
        // HL now points to calculated value (2 bytes after null)
        self.emit(&[0x5E]); // LD E, (HL)
        self.inc_hl();
        self.emit(&[0x56]); // LD D, (HL)
        self.ex_de_hl(); //HL = value)
        self.emit(&[0xCD]); // CALL print_int_cell
        self.fixup("print_int_cell");
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
        // Number - print value
        self.inc_hl();
        self.inc_hl();
        self.emit(&[0x5E]); // LD E, (HL)
        self.inc_hl();
        self.emit(&[0x56]); // LD D, (HL)
        self.ex_de_hl();
        self.emit(&[0xCD]); // CALL print_int
        self.fixup("print_int");
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
        // HL = parsed number, carry set if error
        self.emit(&[0xDA]); // JP C, store_error
        self.fixup("store_error");
        // Store as number in current cell
        self.push_hl(); //save value)
        self.emit(&[0x3A]); // LD A, (CURSOR_COL)
        self.emit_word(CURSOR_COL);
        self.ld_b_a();
        self.emit(&[0x3A]); // LD A, (CURSOR_ROW)
        self.emit_word(CURSOR_ROW);
        self.ld_c_a();
        self.emit(&[0xCD]); // CALL get_cell_addr
        self.fixup("get_cell_addr");
        self.emit(&[0x36, CELL_NUMBER]); // LD (HL), CELL_NUMBER
        self.inc_hl();
        self.emit(&[0x36, 0x00]); // LD (HL), 0 (flags)
        self.inc_hl();
        self.pop_de(); //value)
        self.emit(&[0x73]); // LD (HL), E
        self.inc_hl();
        self.emit(&[0x72]); // LD (HL), D
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

        // Parse number from INPUT_BUF
        // Returns value in HL, carry set on error
        self.label("parse_number");
        self.emit(&[0x21, 0x00, 0x00]); // LD HL, 0 (accumulator)
        self.emit(&[0x11]); // LD DE, INPUT_BUF
        self.emit_word(INPUT_BUF);
        self.emit(&[0x3A]); // LD A, (INPUT_LEN)
        self.emit_word(INPUT_LEN);
        self.ld_b_a(); //counter)
        self.emit(&[0x0E, 0x00]); // LD C, 0 (negative flag)

        // Check for minus sign
        self.emit(&[0x1A]); // LD A, (DE)
        self.emit(&[0xFE, b'-']);
        self.emit(&[0xC2]); // JP NZ, parse_num_loop
        self.fixup("parse_num_loop");
        self.emit(&[0x0E, 0x01]); // LD C, 1 (negative)
        self.inc_de();
        self.dec_b();
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
        self.push_de(); //save buffer ptr)
        self.emit(&[0xD6, b'0']); // SUB '0' (convert to value)
        self.push_af(); //save digit)

        // Multiply HL by 10: HL = HL*2 + HL*8
        self.add_hl_hl(); //*2)
        self.emit(&[0x54]); // LD D, H
        self.emit(&[0x5D]); // LD E, L (DE = HL*2)
        self.add_hl_hl(); //*4)
        self.add_hl_hl(); //*8)
        self.add_hl_de(); //*8 + *2 = *10)

        // Add digit
        self.pop_af(); //restore digit)
        self.ld_e_a();
        self.emit(&[0x16, 0x00]); // LD D, 0
        self.add_hl_de();

        // Move to next character
        self.pop_de(); //restore buffer ptr)
        self.inc_de();
        self.dec_b();
        self.emit(&[0xC2]); // JP NZ, parse_num_loop
        self.fixup("parse_num_loop");

        // Check negative flag
        self.ld_a_c();
        self.or_a_a();
        self.emit(&[0xCA]); // JP Z, parse_num_done
        self.fixup("parse_num_done");
        // Negate HL
        self.emit(&[0x7C]); // LD A, H
        self.cpl();
        self.emit(&[0x67]); // LD H, A
        self.emit(&[0x7D]); // LD A, L
        self.cpl();
        self.emit(&[0x6F]); // LD L, A
        self.inc_hl();

        self.label("parse_num_done");
        self.or_a_a(); //clear carry)
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
        // Address = CELL_DATA + (row * 16 + col) * 4
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
        self.add_hl_hl(); // x2
        self.add_hl_hl(); // x4
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

        // Store calculated value after formula string
        self.ex_de_hl(); //DE = result)
        self.pop_hl(); //value address)
        self.emit(&[0x73]); // LD (HL), E
        self.inc_hl();
        self.emit(&[0x72]); // LD (HL), D
        self.inc_hl();
        // Update FORMULA_PTR
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
        // Output: HL = result, carry set on error
        self.label("eval_expr");
        self.emit(&[0x22]); // LD (TEMP2), HL (save expr ptr)
        self.emit_word(TEMP2);

        // Parse first operand (cell ref or number)
        self.emit(&[0xCD]); // CALL parse_operand
        self.fixup("parse_operand");
        self.emit(&[0xD8]); // RET C (error)
        // HL = accumulator (running result)

        // Main evaluation loop - check for more operators
        self.label("eval_loop");
        self.push_hl(); //save accumulator)
        self.emit(&[0x2A]); // LD HL, (TEMP2)
        self.emit_word(TEMP2);
        self.ld_a_hl_ind();
        self.or_a_a();
        self.emit(&[0xCA]); // JP Z, eval_done (no more operators)
        self.fixup("eval_done");

        // Save operator
        self.emit(&[0x32]); // LD (TEMP1+1), A
        self.emit_word(TEMP1 + 1);
        self.inc_hl(); //past operator)
        self.emit(&[0x22]); // LD (TEMP2), HL
        self.emit_word(TEMP2);

        // Parse next operand
        self.emit(&[0xCD]); // CALL parse_operand
        self.fixup("parse_operand");
        self.emit(&[0xDA]); // JP C, eval_chain_error
        self.fixup("eval_chain_error");
        // HL = next operand
        self.ex_de_hl(); //DE = next operand)
        self.pop_hl(); //accumulator)
        // HL = accumulator, DE = next operand

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
        self.pop_hl(); //accumulator = final result)
        self.or_a_a(); //clear carry)
        self.ret();

        self.label("eval_chain_error");
        self.pop_hl(); //discard accumulator)
        self.emit(&[0x37]); // SCF
        self.ret();

        // HL + DE -> HL, then loop
        self.label("eval_add");
        self.add_hl_de();
        self.emit(&[0xC3]); // JP eval_loop
        self.fixup("eval_loop");

        // HL - DE -> HL, then loop
        self.label("eval_sub");
        self.or_a_a(); //clear carry for SBC)
        self.emit(&[0xED, 0x52]); // SBC HL, DE
        self.emit(&[0xC3]); // JP eval_loop
        self.fixup("eval_loop");

        // HL * DE (16-bit multiply)
        self.label("eval_mul");
        // Save sign: XOR high bytes to get result sign
        self.emit(&[0x7C]); // LD A, H
        self.emit(&[0xAA]); // XOR D
        self.push_af(); //save sign in bit 7)
        // Make both positive
        self.emit(&[0x7C]); // LD A, H
        self.or_a_a();
        self.emit(&[0xF2]); // JP P, mul_hl_pos
        self.fixup("mul_hl_pos");
        // Negate HL
        self.emit(&[0x7C]); // LD A, H
        self.cpl();
        self.emit(&[0x67]); // LD H, A
        self.emit(&[0x7D]); // LD A, L
        self.cpl();
        self.emit(&[0x6F]); // LD L, A
        self.inc_hl();
        self.label("mul_hl_pos");
        self.ld_a_d();
        self.or_a_a();
        self.emit(&[0xF2]); // JP P, mul_de_pos
        self.fixup("mul_de_pos");
        // Negate DE
        self.ld_a_d();
        self.cpl();
        self.ld_d_a();
        self.ld_a_e();
        self.cpl();
        self.ld_e_a();
        self.inc_de();
        self.label("mul_de_pos");
        // Now HL and DE are positive, multiply
        self.push_bc();
        self.emit(&[0x44]); // LD B, H
        self.emit(&[0x4D]); // LD C, L (BC = multiplicand)
        self.emit(&[0x21, 0x00, 0x00]); // LD HL, 0 (result)
        self.emit(&[0x3E, 16]); // LD A, 16 (bit counter)
        self.label("mul_loop");
        self.add_hl_hl(); //shift result left)
        self.emit(&[0xCB, 0x13]); // RL E
        self.emit(&[0xCB, 0x12]); // RL D (shift DE left, high bit to carry)
        self.emit(&[0x30, 0x01]); // JR NC, +1 (skip add if bit was 0)
        self.add_hl_bc();
        self.dec_a();
        self.emit(&[0xC2]); // JP NZ, mul_loop
        self.fixup("mul_loop");
        self.pop_bc();
        // Apply sign
        self.pop_af(); //sign in bit 7)
        self.or_a_a();
        self.emit(&[0xF2]); // JP P, mul_done
        self.fixup("mul_done");
        // Negate result
        self.emit(&[0x7C]); // LD A, H
        self.cpl();
        self.emit(&[0x67]); // LD H, A
        self.emit(&[0x7D]); // LD A, L
        self.cpl();
        self.emit(&[0x6F]); // LD L, A
        self.inc_hl();
        self.label("mul_done");
        self.emit(&[0xC3]); // JP eval_loop
        self.fixup("eval_loop");

        // HL / DE (16-bit divide)
        self.label("eval_div");
        // Check for divide by zero
        self.ld_a_d();
        self.emit(&[0xB3]); // OR E
        self.emit(&[0xC2]); // JP NZ, div_ok
        self.fixup("div_ok");
        self.emit(&[0x37]); // SCF (divide by zero error)
        self.ret();
        self.label("div_ok");
        // Save sign
        self.emit(&[0x7C]); // LD A, H
        self.emit(&[0xAA]); // XOR D
        self.push_af();
        // Make both positive
        self.emit(&[0x7C]); // LD A, H
        self.or_a_a();
        self.emit(&[0xF2]); // JP P, div_hl_pos
        self.fixup("div_hl_pos");
        self.emit(&[0x7C]); // LD A, H
        self.cpl();
        self.emit(&[0x67]); // LD H, A
        self.emit(&[0x7D]); // LD A, L
        self.cpl();
        self.emit(&[0x6F]); // LD L, A
        self.inc_hl();
        self.label("div_hl_pos");
        self.ld_a_d();
        self.or_a_a();
        self.emit(&[0xF2]); // JP P, div_de_pos
        self.fixup("div_de_pos");
        self.ld_a_d();
        self.cpl();
        self.ld_d_a();
        self.ld_a_e();
        self.cpl();
        self.ld_e_a();
        self.inc_de();
        self.label("div_de_pos");
        // Divide HL by DE using repeated subtraction
        self.push_bc();
        self.emit(&[0x01, 0x00, 0x00]); // LD BC, 0 (quotient)
        self.label("div_loop");
        self.or_a_a();
        self.emit(&[0xED, 0x52]); // SBC HL, DE
        self.emit(&[0xDA]); // JP C, div_restore
        self.fixup("div_restore");
        self.emit(&[0x03]); // INC BC
        self.emit(&[0xC3]); // JP div_loop
        self.fixup("div_loop");
        self.label("div_restore");
        self.add_hl_de(); //restore remainder)
        self.emit(&[0x60]); // LD H, B
        self.emit(&[0x69]); // LD L, C (HL = quotient)
        self.pop_bc();
        // Apply sign
        self.pop_af();
        self.or_a_a();
        self.emit(&[0xF2]); // JP P, div_done
        self.fixup("div_done");
        self.emit(&[0x7C]); // LD A, H
        self.cpl();
        self.emit(&[0x67]); // LD H, A
        self.emit(&[0x7D]); // LD A, L
        self.cpl();
        self.emit(&[0x6F]); // LD L, A
        self.inc_hl();
        self.label("div_done");
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
        // Get cell value
        self.emit(&[0xCD]); // CALL get_cell_addr
        self.fixup("get_cell_addr");
        self.ld_a_hl_ind(); // type
        self.or_a_a();
        self.emit(&[0xCA]); // JP Z, parse_op_zero (empty cell = 0)
        self.fixup("parse_op_zero");
        self.inc_hl();
        self.inc_hl();
        self.emit(&[0x5E]); // LD E, (HL)
        self.inc_hl();
        self.emit(&[0x56]); // LD D, (HL)
        self.ex_de_hl();
        self.or_a_a(); //clear carry)
        self.ret();

        self.label("parse_op_zero");
        self.emit(&[0x21, 0x00, 0x00]); // LD HL, 0
        self.or_a_a();
        self.ret();

        // Parse number operand
        self.label("parse_op_number");
        self.emit(&[0x2A]); // LD HL, (TEMP2)
        self.emit_word(TEMP2);
        self.emit(&[0x11, 0x00, 0x00]); // LD DE, 0 (accumulator)
        self.emit(&[0x0E, 0x00]); // LD C, 0 (negative flag)

        // Check minus
        self.ld_a_hl_ind();
        self.emit(&[0xFE, b'-']);
        self.emit(&[0xC2]); // JP NZ, parse_opn_loop
        self.fixup("parse_opn_loop");
        self.emit(&[0x0E, 0x01]); // LD C, 1
        self.inc_hl();

        self.label("parse_opn_loop");
        self.ld_a_hl_ind();
        self.emit(&[0xFE, b'0']);
        self.emit(&[0xDA]); // JP C, parse_opn_done
        self.fixup("parse_opn_done");
        self.emit(&[0xFE, b'9' + 1]);
        self.emit(&[0xD2]); // JP NC, parse_opn_done
        self.fixup("parse_opn_done");
        self.emit(&[0xD6, b'0']); // SUB '0'
        self.push_af();
        // Multiply DE by 10: x10 = x2 + x8
        self.ex_de_hl(); // HL = accumulator
        self.add_hl_hl(); // x2
        self.emit(&[0x54]); // LD D, H (save x2 in DE)
        self.emit(&[0x5D]); // LD E, L
        self.add_hl_hl(); // x4
        self.add_hl_hl(); // x8
        self.add_hl_de(); // x8 + x2 = x10
        self.ex_de_hl(); // DE = accumulator
        // Add digit
        self.pop_af();
        self.emit(&[0x6F]); // LD L, A
        self.emit(&[0x26, 0x00]); // LD H, 0
        self.add_hl_de();
        self.ex_de_hl();
        self.emit(&[0x2A]); // LD HL, (TEMP2)
        self.emit_word(TEMP2);
        self.inc_hl();
        self.emit(&[0x22]); // LD (TEMP2), HL
        self.emit_word(TEMP2);
        self.emit(&[0xC3]); // JP parse_opn_loop
        self.fixup("parse_opn_loop");

        self.label("parse_opn_done");
        self.emit(&[0x22]); // LD (TEMP2), HL
        self.emit_word(TEMP2);
        self.ex_de_hl();
        // Check negative
        self.ld_a_c();
        self.or_a_a();
        self.emit(&[0xCA]); // JP Z, parse_opn_ret
        self.fixup("parse_opn_ret");
        // Negate
        self.emit(&[0x7C]); // LD A, H
        self.cpl();
        self.emit(&[0x67]); // LD H, A
        self.emit(&[0x7D]); // LD A, L
        self.cpl();
        self.emit(&[0x6F]); // LD L, A
        self.inc_hl();

        self.label("parse_opn_ret");
        self.or_a_a();
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

        // Parse second cell - skip col, parse row2 directly
        self.ld_a_hl_ind();
        self.emit(&[0xE6, 0xDF]); // uppercase
        self.emit(&[0xFE, b'A']);
        self.emit(&[0xDA]); // JP C, pf_error
        self.fixup("pf_error");
        self.inc_hl(); //skip col letter)
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

        // Initialize accumulators based on function type
        // DE = sum (for SUM/AVG), FUNC_COUNT = count, FUNC_MINMAX = min/max
        self.emit(&[0x11, 0x00, 0x00]); // LD DE, 0 (sum)
        self.xor_a();
        self.emit(&[0x32]); // LD (FUNC_COUNT), A
        self.emit_word(FUNC_COUNT);
        self.emit(&[0x32]); // LD (FUNC_COUNT+1), A
        self.emit_word(FUNC_COUNT + 1);

        // Initialize min to 32767, max to 0
        // (For spreadsheet use, values are typically positive)
        self.emit(&[0x21]); // LD HL, 32767
        self.emit_word(32767);
        self.emit(&[0x22]); // LD (FUNC_MINMAX), HL
        self.emit_word(FUNC_MINMAX);
        // For MAX, initialize to 0 instead
        self.emit(&[0x3A]); // LD A, (FUNC_TYPE)
        self.emit_word(FUNC_TYPE);
        self.emit(&[0xFE, 0x03]); // CP 3 (MAX)
        self.emit(&[0xC2]); // JP NZ, pf_init_done
        self.fixup("pf_init_done");
        self.emit(&[0x21, 0x00, 0x00]); // LD HL, 0
        self.emit(&[0x22]); // LD (FUNC_MINMAX), HL
        self.emit_word(FUNC_MINMAX);
        self.label("pf_init_done");

        // C = current row
        self.emit(&[0x3A]); // LD A, (TEMP1+1) (row1)
        self.emit_word(TEMP1 + 1);
        self.ld_c_a();

        self.label("pf_loop");
        // Get cell value at (col1, C)
        self.push_de(); //save sum)
        self.emit(&[0x3A]); // LD A, (TEMP1)
        self.emit_word(TEMP1);
        self.ld_b_a(); //col)
        self.push_bc(); //save row counter)
        self.emit(&[0xCD]); // CALL get_cell_addr
        self.fixup("get_cell_addr");
        // HL = cell addr
        self.ld_a_hl_ind(); // type
        self.emit(&[0xFE, CELL_NUMBER]); // CP CELL_NUMBER
        self.emit(&[0xC2]); // JP NZ, pf_skip (not a number)
        self.fixup("pf_skip");

        // Found a number - increment count
        self.push_hl(); //save cell addr)
        self.emit(&[0x2A]); // LD HL, (FUNC_COUNT)
        self.emit_word(FUNC_COUNT);
        self.inc_hl();
        self.emit(&[0x22]); // LD (FUNC_COUNT), HL
        self.emit_word(FUNC_COUNT);
        self.pop_hl(); //restore cell addr)

        // Get cell value into DE
        self.inc_hl();
        self.inc_hl();
        self.emit(&[0x5E]); // LD E, (HL)
        self.inc_hl();
        self.emit(&[0x56]); // LD D, (HL)
        // DE = cell value

        // Check function type for SUM/AVG vs MIN/MAX
        self.emit(&[0x3A]); // LD A, (FUNC_TYPE)
        self.emit_word(FUNC_TYPE);
        self.emit(&[0xFE, 0x02]); // CP 2 (MIN)
        self.emit(&[0xCA]); // JP Z, pf_do_min
        self.fixup("pf_do_min");
        self.emit(&[0xFE, 0x03]); // CP 3 (MAX)
        self.emit(&[0xCA]); // JP Z, pf_do_max
        self.fixup("pf_do_max");

        // SUM/AVG/COUNT: add to sum
        self.pop_bc();
        self.pop_hl(); //sum in HL)
        self.add_hl_de();
        self.ex_de_hl(); //sum back in DE)
        self.emit(&[0xC3]); // JP pf_next
        self.fixup("pf_next");

        // MIN: if DE < (FUNC_MINMAX), update
        self.label("pf_do_min");
        self.emit(&[0x2A]); // LD HL, (FUNC_MINMAX)
        self.emit_word(FUNC_MINMAX);
        // Compare DE vs HL (signed): DE - HL
        self.or_a_a(); //clear carry)
        self.emit(&[0xED, 0x52]); // SBC HL, DE (HL = HL - DE)
        // If HL > 0 (was bigger), DE is smaller, update
        self.emit(&[0xFA]); // JP M, pf_min_skip (HL negative = DE was bigger)
        self.fixup("pf_min_skip");
        self.emit(&[0xED, 0x53]); // LD (FUNC_MINMAX), DE
        self.emit_word(FUNC_MINMAX);
        self.label("pf_min_skip");
        self.pop_bc();
        self.pop_de(); //restore sum)
        self.emit(&[0xC3]); // JP pf_next
        self.fixup("pf_next");

        // MAX: if DE > (FUNC_MINMAX), update
        self.label("pf_do_max");
        self.emit(&[0x2A]); // LD HL, (FUNC_MINMAX)
        self.emit_word(FUNC_MINMAX);
        // Compare DE vs HL: if DE > HL, update
        self.or_a_a();
        self.emit(&[0xED, 0x52]); // SBC HL, DE (HL = HL - DE)
        // If HL < 0 (was smaller), DE is bigger, update
        self.emit(&[0xF2]); // JP P, pf_max_skip (HL positive = DE was smaller)
        self.fixup("pf_max_skip");
        self.emit(&[0xED, 0x53]); // LD (FUNC_MINMAX), DE
        self.emit_word(FUNC_MINMAX);
        self.label("pf_max_skip");
        self.pop_bc();
        self.pop_de();
        self.emit(&[0xC3]); // JP pf_next
        self.fixup("pf_next");

        self.label("pf_skip");
        // Not a number - skip
        self.pop_bc();
        self.pop_de();

        self.label("pf_next");
        // Check if done (C > row2)
        self.ld_a_c(); //current row)
        self.ld_b_a(); //save in B)
        self.emit(&[0x3A]); // LD A, (RANGE_ROW2)
        self.emit_word(RANGE_ROW2);
        self.emit(&[0xB8]); // CP B
        self.emit(&[0xDA]); // JP C, pf_done (row2 < current = done)
        self.fixup("pf_done");
        self.inc_c();
        self.emit(&[0xC3]); // JP pf_loop
        self.fixup("pf_loop");

        // Return result based on function type
        self.label("pf_done");
        self.emit(&[0x3A]); // LD A, (FUNC_TYPE)
        self.emit_word(FUNC_TYPE);

        // SUM (0): return sum in DE
        self.or_a_a();
        self.emit(&[0xC2]); // JP NZ, pf_not_sum
        self.fixup("pf_not_sum");
        self.ex_de_hl();
        self.or_a_a(); //clear carry)
        self.ret();

        // AVG (1): return sum / count
        self.label("pf_not_sum");
        self.emit(&[0xFE, 0x01]); // CP 1
        self.emit(&[0xC2]); // JP NZ, pf_not_avg
        self.fixup("pf_not_avg");
        // HL = sum (in DE), divide by count
        self.ex_de_hl(); //sum in HL)
        self.emit(&[0xED, 0x5B]); // LD DE, (FUNC_COUNT)
        self.emit_word(FUNC_COUNT);
        // Check for divide by zero
        self.ld_a_d();
        self.emit(&[0xB3]); // OR E
        self.emit(&[0xC2]); // JP NZ, pf_div_ok
        self.fixup("pf_div_ok");
        self.emit(&[0x21, 0x00, 0x00]); // LD HL, 0
        self.or_a_a();
        self.ret();
        self.label("pf_div_ok");
        // HL / DE -> HL (simple unsigned division)
        self.emit(&[0xCD]); // CALL div16
        self.fixup("div16");
        self.or_a_a();
        self.ret();

        // MIN (2) or MAX (3): return FUNC_MINMAX
        self.label("pf_not_avg");
        self.emit(&[0xFE, 0x02]); // CP 2
        self.emit(&[0xCA]); // JP Z, pf_ret_minmax
        self.fixup("pf_ret_minmax");
        self.emit(&[0xFE, 0x03]); // CP 3
        self.emit(&[0xCA]); // JP Z, pf_ret_minmax
        self.fixup("pf_ret_minmax");

        // COUNT (4): return count
        self.emit(&[0x2A]); // LD HL, (FUNC_COUNT)
        self.emit_word(FUNC_COUNT);
        self.or_a_a();
        self.ret();

        self.label("pf_ret_minmax");
        self.emit(&[0x2A]); // LD HL, (FUNC_MINMAX)
        self.emit_word(FUNC_MINMAX);
        self.or_a_a();
        self.ret();

        // 16-bit division: HL / DE -> HL (quotient), remainder discarded
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
        // Cell (1,0) should be at CELL_DATA + 4
        // Cell (0,1) should be at CELL_DATA + 64
        // Formula: CELL_DATA + (row * 16 + col) * 4
        let base = CELL_DATA;
        assert_eq!(base + (0 * 16 + 0) * 4, 0x2000);
        assert_eq!(base + (0 * 16 + 1) * 4, 0x2004);
        assert_eq!(base + (1 * 16 + 0) * 4, 0x2040);
    }
}
