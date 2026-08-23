use std::collections::VecDeque;
use vte::{Params, Parser, Perform};

pub struct TerminalBuffer {
    pub lines: VecDeque<String>,
    pub max_scrollback: usize,
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub cols: usize,
    pub rows: usize,
    pub visible_rows: usize,
    pub scroll_offset: usize,
    pub show_cursor: bool,
    parser: Parser,
}

impl TerminalBuffer {
    pub fn new(cols: usize, _rows: usize, max_scrollback: usize) -> Self {
        let mut lines = VecDeque::new();
        lines.push_back(String::with_capacity(128));
        Self {
            lines,
            max_scrollback: max_scrollback.clamp(500, 50_000),
            cursor_row: 0,
            cursor_col: 0,
            cols: cols.max(20),
            rows: 24,
            visible_rows: 24,
            scroll_offset: 0,
            show_cursor: true,
            parser: Parser::new(),
        }
    }

    pub fn set_visible_rows(&mut self, rows: usize) {
        self.visible_rows = rows.clamp(10, 300);
        self.rows = self.visible_rows;
    }

    pub fn process_bytes(&mut self, bytes: &[u8]) {
        let mut performer = BufferPerformer { buffer: self };
        let mut parser = std::mem::replace(&mut performer.buffer.parser, Parser::new());
        parser.advance(&mut performer, bytes);
        performer.buffer.parser = parser;
    }

    pub fn get_lines(&self) -> Vec<String> {
        self.lines.iter().cloned().collect()
    }

    pub fn get_visible_text(&self) -> (String, usize, usize, usize, usize) {
        let total = self.lines.len();
        let rows = self.visible_rows.max(10).min(300);

        let end_idx = total.saturating_sub(self.scroll_offset).min(total).max(1);
        let start_idx = end_idx.saturating_sub(rows);

        // Collect only the visible slice — O(visible_rows), not O(total)
        let mut parts = Vec::with_capacity(end_idx - start_idx);
        for i in start_idx..end_idx {
            parts.push(self.lines[i].as_str());
        }
        let text = parts.join("\n");

        // Cursor offset calculation (only used for selection, not typed in ticker)
        let cursor_line = self.cursor_row.min(total.saturating_sub(1));
        let cursor_offset = if cursor_line >= start_idx && cursor_line < end_idx {
            let mut off = 0;
            for i in start_idx..cursor_line {
                off += self.lines[i].len() + 1; // byte len, not char count — faster
            }
            if let Some(line) = self.lines.get(cursor_line) {
                off += self.cursor_col.min(line.len());
            }
            off
        } else {
            text.len()
        };

        (text, cursor_offset, total, self.scroll_offset, rows)
    }

    pub fn scroll_by(&mut self, delta_lines: isize) {
        let total = self.lines.len();
        let max_scroll = total.saturating_sub(self.visible_rows);
        if delta_lines > 0 {
            self.scroll_offset = (self.scroll_offset + delta_lines as usize).min(max_scroll);
        } else if delta_lines < 0 {
            self.scroll_offset = self
                .scroll_offset
                .saturating_sub(delta_lines.unsigned_abs());
        }
    }

    pub fn scroll_to_ratio(&mut self, ratio: f32) {
        let total = self.lines.len();
        let max_scroll = total.saturating_sub(self.visible_rows);
        let clamped = (1.0 - ratio.clamp(0.0, 1.0)) * (max_scroll as f32);
        self.scroll_offset = (clamped.round() as usize).min(max_scroll);
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
    }

    pub fn clear(&mut self) {
        self.lines.clear();
        self.lines.push_back(String::with_capacity(128));
        self.cursor_row = 0;
        self.cursor_col = 0;
        self.scroll_offset = 0;
    }

    // Trim oldest lines in batches of 64 to avoid per-line reallocation churn
    fn trim_scrollback(&mut self) {
        if self.lines.len() > self.max_scrollback + 64 {
            let excess = self.lines.len() - self.max_scrollback;
            self.lines.drain(..excess);
            self.cursor_row = self.cursor_row.saturating_sub(excess);
        }
    }

    fn ensure_row(&mut self, row: usize) {
        while self.lines.len() <= row && self.lines.len() < self.max_scrollback {
            self.lines.push_back(String::with_capacity(128));
        }
    }
}

struct BufferPerformer<'a> {
    buffer: &'a mut TerminalBuffer,
}

impl<'a> Perform for BufferPerformer<'a> {
    fn print(&mut self, c: char) {
        if self.buffer.lines.is_empty() {
            self.buffer.lines.push_back(String::with_capacity(128));
            self.buffer.cursor_row = 0;
        }
        let row = self.buffer.cursor_row.min(self.buffer.lines.len() - 1);
        let col = self.buffer.cursor_col;
        let line = &mut self.buffer.lines[row];

        // 99.9% common case: Appending at the end of the line (e.g. streaming output)
        if col >= line.len() {
            if col > line.len() {
                let pad = (col - line.len()).min(256);
                for _ in 0..pad {
                    line.push(' ');
                }
            }
            line.push(c);
        } else {
            // Overwriting in the middle of a line
            if c.is_ascii() && line.is_ascii() {
                let bytes = unsafe { line.as_bytes_mut() };
                if col < bytes.len() {
                    bytes[col] = c as u8;
                }
            } else {
                let mut new_line = String::with_capacity(line.len() + 4);
                let mut char_idx = 0;
                let mut replaced = false;
                for existing in line.chars() {
                    if char_idx == col {
                        new_line.push(c);
                        replaced = true;
                    } else {
                        new_line.push(existing);
                    }
                    char_idx += 1;
                }
                if !replaced {
                    new_line.push(c);
                }
                *line = new_line;
            }
        }
        self.buffer.cursor_col += 1;
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\n' => {
                self.buffer.cursor_row += 1;
                if self.buffer.cursor_row >= self.buffer.lines.len() {
                    self.buffer.lines.push_back(String::with_capacity(128));
                    self.buffer.trim_scrollback();
                    self.buffer.cursor_row = self.buffer.lines.len().saturating_sub(1);
                }
            }
            b'\r' => {
                self.buffer.cursor_col = 0;
            }
            b'\x08' => {
                if self.buffer.cursor_col > 0 {
                    self.buffer.cursor_col -= 1;
                }
            }
            0x7f => {
                let row = self
                    .buffer
                    .cursor_row
                    .min(self.buffer.lines.len().saturating_sub(1));
                if let Some(line) = self.buffer.lines.get_mut(row) {
                    if self.buffer.cursor_col > 0 && self.buffer.cursor_col <= line.len() {
                        self.buffer.cursor_col -= 1;
                        line.remove(self.buffer.cursor_col);
                    }
                }
            }
            b'\t' => {
                let next_tab = (self.buffer.cursor_col / 8 + 1) * 8;
                self.buffer.cursor_col = next_tab;
            }
            _ => {}
        }
    }

    fn hook(&mut self, _params: &Params, _intermediates: &[u8], _ignore: bool, _action: char) {}
    fn put(&mut self, _byte: u8) {}
    fn unhook(&mut self) {}
    fn osc_dispatch(&mut self, _params: &[&[u8]], _bell_terminated: bool) {}

    fn csi_dispatch(
        &mut self,
        params: &Params,
        _intermediates: &[u8],
        _ignore: bool,
        action: char,
    ) {
        let first_param = params
            .iter()
            .next()
            .and_then(|p| p.first().copied())
            .unwrap_or(1) as usize;
        let count = if first_param == 0 { 1 } else { first_param };

        match action {
            'A' => {
                let min_row = self
                    .buffer
                    .lines
                    .len()
                    .saturating_sub(self.buffer.visible_rows);
                self.buffer.cursor_row = self.buffer.cursor_row.saturating_sub(count).max(min_row);
            }
            'B' => {
                let max_row = self.buffer.lines.len().saturating_sub(1);
                self.buffer.cursor_row = (self.buffer.cursor_row + count).min(max_row);
            }
            'C' => {
                self.buffer.cursor_col += count;
            }
            'D' => {
                self.buffer.cursor_col = self.buffer.cursor_col.saturating_sub(count);
            }
            'H' | 'f' => {
                let mut iter = params.iter();
                let r = iter
                    .next()
                    .and_then(|p| p.first().copied())
                    .unwrap_or(1)
                    .max(1) as usize
                    - 1;
                let c = iter
                    .next()
                    .and_then(|p| p.first().copied())
                    .unwrap_or(1)
                    .max(1) as usize
                    - 1;
                let base_row = self
                    .buffer
                    .lines
                    .len()
                    .saturating_sub(self.buffer.visible_rows);
                let target_row = base_row + r;
                self.buffer.ensure_row(target_row);
                self.buffer.cursor_row = target_row.min(self.buffer.lines.len().saturating_sub(1));
                self.buffer.cursor_col = c;
            }
            'K' => {
                let row = self
                    .buffer
                    .cursor_row
                    .min(self.buffer.lines.len().saturating_sub(1));
                if let Some(line) = self.buffer.lines.get_mut(row) {
                    let mode = params
                        .iter()
                        .next()
                        .and_then(|p| p.first().copied())
                        .unwrap_or(0);
                    match mode {
                        0 => {
                            line.truncate(self.buffer.cursor_col);
                        }
                        1 => {
                            let preserved: String =
                                line.chars().skip(self.buffer.cursor_col).collect();
                            let spaces = " ".repeat(self.buffer.cursor_col);
                            *line = format!("{}{}", spaces, preserved);
                        }
                        2 => {
                            line.clear();
                        }
                        _ => {}
                    }
                }
            }
            'J' => {
                let mode = params
                    .iter()
                    .next()
                    .and_then(|p| p.first().copied())
                    .unwrap_or(0);
                if mode == 2 || mode == 3 {
                    self.buffer.clear();
                }
            }
            'P' => {
                let row = self
                    .buffer
                    .cursor_row
                    .min(self.buffer.lines.len().saturating_sub(1));
                if let Some(line) = self.buffer.lines.get_mut(row) {
                    let col = self.buffer.cursor_col;
                    if col < line.len() {
                        line.drain(col..usize::min(col + count, line.len()));
                    }
                }
            }
            '@' => {
                let row = self
                    .buffer
                    .cursor_row
                    .min(self.buffer.lines.len().saturating_sub(1));
                if let Some(line) = self.buffer.lines.get_mut(row) {
                    let col = self.buffer.cursor_col.min(line.len());
                    let spaces = " ".repeat(count);
                    line.insert_str(col, &spaces);
                }
            }
            _ => {}
        }
    }

    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, byte: u8) {
        if byte == b'M' {
            self.buffer.cursor_row = self.buffer.cursor_row.saturating_sub(1);
        } else if byte == b'E' {
            self.buffer.lines.push_back(String::new());
            self.buffer.trim_scrollback();
            self.buffer.cursor_row = self.buffer.lines.len().saturating_sub(1);
            self.buffer.cursor_col = 0;
        }
    }
}
