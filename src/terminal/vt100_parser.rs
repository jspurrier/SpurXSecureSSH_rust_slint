use std::collections::VecDeque;
use vte::{Params, Parser, Perform};

pub struct TerminalBuffer {
    pub scrollback: VecDeque<String>,
    pub screen: Vec<String>,
    pub max_scrollback: usize,
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub saved_cursor: (usize, usize),
    pub cols: usize,
    pub rows: usize,
    pub visible_rows: usize,
    pub scroll_offset: usize,
    pub show_cursor: bool,
    parser: Parser,
}

impl TerminalBuffer {
    pub fn new(cols: usize, rows: usize, max_scrollback: usize) -> Self {
        let actual_cols = cols.max(20);
        let actual_rows = rows.clamp(2, 300);
        let mut screen = Vec::with_capacity(actual_rows);
        for _ in 0..actual_rows {
            screen.push(String::with_capacity(128));
        }

        Self {
            scrollback: VecDeque::new(),
            screen,
            max_scrollback: max_scrollback.clamp(500, 50_000),
            cursor_row: 0,
            cursor_col: 0,
            saved_cursor: (0, 0),
            cols: actual_cols,
            rows: actual_rows,
            visible_rows: actual_rows,
            scroll_offset: 0,
            show_cursor: true,
            parser: Parser::new(),
        }
    }

    pub fn set_size(&mut self, cols: usize, rows: usize) {
        self.cols = cols.max(20);
        let new_rows = rows.clamp(2, 300);

        if new_rows > self.screen.len() {
            let diff = new_rows - self.screen.len();
            // Pull recent lines from scrollback to top of screen if available
            let take = diff.min(self.scrollback.len());
            for _ in 0..take {
                if let Some(line) = self.scrollback.pop_back() {
                    self.screen.insert(0, line);
                    self.cursor_row += 1;
                }
            }
            while self.screen.len() < new_rows {
                self.screen.push(String::with_capacity(128));
            }
        } else if new_rows < self.screen.len() {
            let diff = self.screen.len() - new_rows;
            for _ in 0..diff {
                if !self.screen.is_empty() {
                    let top = self.screen.remove(0);
                    self.scrollback.push_back(top);
                    self.cursor_row = self.cursor_row.saturating_sub(1);
                }
            }
            self.trim_scrollback();
        }

        self.visible_rows = new_rows;
        self.rows = new_rows;
        self.cursor_row = self.cursor_row.min(self.visible_rows.saturating_sub(1));
        self.cursor_col = self.cursor_col.min(self.cols.saturating_sub(1));
    }

    pub fn set_visible_rows(&mut self, rows: usize) {
        self.set_size(self.cols, rows);
    }

    pub fn process_bytes(&mut self, bytes: &[u8]) {
        let mut performer = BufferPerformer { buffer: self };
        let mut parser = std::mem::replace(&mut performer.buffer.parser, Parser::new());
        parser.advance(&mut performer, bytes);
        performer.buffer.parser = parser;
    }

    pub fn get_lines(&self) -> Vec<String> {
        let mut out = Vec::with_capacity(self.scrollback.len() + self.screen.len());
        out.extend(self.scrollback.iter().cloned());
        out.extend(self.screen.iter().cloned());
        out
    }

    pub fn get_visible_text(&self) -> (String, usize, usize, usize, usize) {
        let total = self.scrollback.len() + self.screen.len();
        let rows = self.visible_rows.max(2).min(300);

        if self.scroll_offset == 0 {
            // Viewing active screen directly
            let text = self.screen.join("\n");
            let cursor_line = self.cursor_row.min(self.screen.len().saturating_sub(1));
            let mut cursor_offset = 0;
            for i in 0..cursor_line {
                cursor_offset += self.screen[i].len() + 1;
            }
            if let Some(line) = self.screen.get(cursor_line) {
                cursor_offset += self.cursor_col.min(line.len());
            }
            (text, cursor_offset, total, 0, rows)
        } else {
            // Viewing scrollback history
            let end_idx = total.saturating_sub(self.scroll_offset).min(total).max(1);
            let start_idx = end_idx.saturating_sub(rows);

            let mut parts = Vec::with_capacity(end_idx - start_idx);
            for i in start_idx..end_idx {
                if i < self.scrollback.len() {
                    parts.push(self.scrollback[i].as_str());
                } else {
                    let screen_idx = i - self.scrollback.len();
                    if screen_idx < self.screen.len() {
                        parts.push(self.screen[screen_idx].as_str());
                    }
                }
            }
            let text = parts.join("\n");
            let cursor_offset = text.len();
            (text, cursor_offset, total, self.scroll_offset, rows)
        }
    }

    pub fn scroll_by(&mut self, delta_lines: isize) {
        let max_scroll = self.scrollback.len();
        if delta_lines > 0 {
            self.scroll_offset = (self.scroll_offset + delta_lines as usize).min(max_scroll);
        } else if delta_lines < 0 {
            self.scroll_offset = self
                .scroll_offset
                .saturating_sub(delta_lines.unsigned_abs());
        }
    }

    pub fn scroll_to_ratio(&mut self, ratio: f32) {
        let max_scroll = self.scrollback.len();
        self.scroll_offset =
            ((ratio.clamp(0.0, 1.0) * (max_scroll as f32)).round() as usize).min(max_scroll);
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
    }

    pub fn clear(&mut self) {
        self.scrollback.clear();
        self.screen.clear();
        for _ in 0..self.visible_rows {
            self.screen.push(String::with_capacity(128));
        }
        self.cursor_row = 0;
        self.cursor_col = 0;
        self.scroll_offset = 0;
    }

    fn trim_scrollback(&mut self) {
        if self.scrollback.len() > self.max_scrollback + 64 {
            let excess = self.scrollback.len() - self.max_scrollback;
            self.scrollback.drain(..excess);
        }
    }
}

struct BufferPerformer<'a> {
    buffer: &'a mut TerminalBuffer,
}

impl<'a> Perform for BufferPerformer<'a> {
    fn print(&mut self, c: char) {
        if self.buffer.screen.is_empty() {
            for _ in 0..self.buffer.visible_rows {
                self.buffer.screen.push(String::with_capacity(128));
            }
        }

        // Wrap to next line if cursor exceeds line width
        if self.buffer.cursor_col >= self.buffer.cols {
            self.execute(b'\n');
            self.buffer.cursor_col = 0;
        }

        let row = self
            .buffer
            .cursor_row
            .min(self.buffer.screen.len().saturating_sub(1));
        let col = self.buffer.cursor_col;
        let line = &mut self.buffer.screen[row];

        let char_count = line.chars().count();
        if col >= char_count {
            let pad = (col - char_count).min(512);
            for _ in 0..pad {
                line.push(' ');
            }
            line.push(c);
        } else {
            // Overwrite in the middle of a line
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
            b'\n' | 0x0b | 0x0c => {
                if self.buffer.cursor_row + 1 < self.buffer.visible_rows {
                    self.buffer.cursor_row += 1;
                } else {
                    // Scroll up: remove top line of active screen and push into scrollback
                    if !self.buffer.screen.is_empty() {
                        let old_top = self.buffer.screen.remove(0);
                        self.buffer.scrollback.push_back(old_top);
                        self.buffer.trim_scrollback();
                        self.buffer.screen.push(String::with_capacity(128));
                    }
                    self.buffer.cursor_row = self.buffer.visible_rows.saturating_sub(1);
                }
            }
            b'\r' => {
                self.buffer.cursor_col = 0;
            }
            b'\x08' => {
                self.buffer.cursor_col = self.buffer.cursor_col.saturating_sub(1);
            }
            0x7f => {
                if self.buffer.cursor_col > 0 {
                    self.buffer.cursor_col -= 1;
                    let row = self
                        .buffer
                        .cursor_row
                        .min(self.buffer.screen.len().saturating_sub(1));
                    if let Some(line) = self.buffer.screen.get_mut(row) {
                        let col = self.buffer.cursor_col;
                        let chars: Vec<char> = line.chars().collect();
                        if col < chars.len() {
                            let mut new_s = String::with_capacity(line.len());
                            for (i, ch) in chars.into_iter().enumerate() {
                                if i != col {
                                    new_s.push(ch);
                                }
                            }
                            *line = new_s;
                        }
                    }
                }
            }
            b'\t' => {
                let next_tab = ((self.buffer.cursor_col / 8) + 1) * 8;
                self.buffer.cursor_col = next_tab.min(self.buffer.cols.saturating_sub(1));
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
                // Cursor Up
                self.buffer.cursor_row = self.buffer.cursor_row.saturating_sub(count);
            }
            'B' | 'e' => {
                // Cursor Down
                let max_row = self.buffer.visible_rows.saturating_sub(1);
                self.buffer.cursor_row = (self.buffer.cursor_row + count).min(max_row);
            }
            'C' | 'a' => {
                // Cursor Forward
                let max_col = self.buffer.cols.saturating_sub(1);
                self.buffer.cursor_col = (self.buffer.cursor_col + count).min(max_col);
            }
            'D' => {
                // Cursor Backward
                self.buffer.cursor_col = self.buffer.cursor_col.saturating_sub(count);
            }
            'H' | 'f' => {
                // Cursor Position (row, col) - 1-based, relative to active screen
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

                self.buffer.cursor_row = r.min(self.buffer.visible_rows.saturating_sub(1));
                self.buffer.cursor_col = c.min(self.buffer.cols.saturating_sub(1));
            }
            'G' | '`' => {
                // Cursor Horizontal Absolute
                let c = count.max(1) - 1;
                self.buffer.cursor_col = c.min(self.buffer.cols.saturating_sub(1));
            }
            'd' => {
                // Line Position Absolute
                let r = count.max(1) - 1;
                self.buffer.cursor_row = r.min(self.buffer.visible_rows.saturating_sub(1));
            }
            'E' => {
                // Cursor Next Line
                self.buffer.cursor_col = 0;
                let max_row = self.buffer.visible_rows.saturating_sub(1);
                self.buffer.cursor_row = (self.buffer.cursor_row + count).min(max_row);
            }
            'F' => {
                // Cursor Previous Line
                self.buffer.cursor_col = 0;
                self.buffer.cursor_row = self.buffer.cursor_row.saturating_sub(count);
            }
            'K' => {
                // Erase in Line
                let row = self
                    .buffer
                    .cursor_row
                    .min(self.buffer.screen.len().saturating_sub(1));
                if let Some(line) = self.buffer.screen.get_mut(row) {
                    let mode = params
                        .iter()
                        .next()
                        .and_then(|p| p.first().copied())
                        .unwrap_or(0);
                    let col = self.buffer.cursor_col;
                    let chars: Vec<char> = line.chars().collect();
                    match mode {
                        0 => {
                            // Erase from cursor to end of line
                            if col < chars.len() {
                                *line = chars[..col].iter().collect();
                            }
                        }
                        1 => {
                            // Erase from start of line to cursor
                            let spaces = " ".repeat(col.min(chars.len()));
                            let mut new_s = spaces;
                            if col < chars.len() {
                                new_s.extend(chars[col..].iter());
                            }
                            *line = new_s;
                        }
                        2 => {
                            // Erase entire line
                            line.clear();
                        }
                        _ => {}
                    }
                }
            }
            'J' => {
                // Erase in Display
                let mode = params
                    .iter()
                    .next()
                    .and_then(|p| p.first().copied())
                    .unwrap_or(0);
                match mode {
                    0 => {
                        // Erase from cursor to end of screen
                        let row = self
                            .buffer
                            .cursor_row
                            .min(self.buffer.screen.len().saturating_sub(1));
                        let col = self.buffer.cursor_col;
                        if let Some(line) = self.buffer.screen.get_mut(row) {
                            let chars: Vec<char> = line.chars().collect();
                            if col < chars.len() {
                                *line = chars[..col].iter().collect();
                            }
                        }
                        for r in (row + 1)..self.buffer.screen.len() {
                            self.buffer.screen[r].clear();
                        }
                    }
                    1 => {
                        // Erase from start of screen to cursor
                        let row = self
                            .buffer
                            .cursor_row
                            .min(self.buffer.screen.len().saturating_sub(1));
                        for r in 0..row {
                            if r < self.buffer.screen.len() {
                                self.buffer.screen[r].clear();
                            }
                        }
                        let col = self.buffer.cursor_col;
                        if let Some(line) = self.buffer.screen.get_mut(row) {
                            let chars: Vec<char> = line.chars().collect();
                            let spaces = " ".repeat(col.min(chars.len()));
                            let mut new_s = spaces;
                            if col < chars.len() {
                                new_s.extend(chars[col..].iter());
                            }
                            *line = new_s;
                        }
                    }
                    2 => {
                        // Erase entire visible screen (scrollback preserved)
                        for line in self.buffer.screen.iter_mut() {
                            line.clear();
                        }
                    }
                    3 => {
                        // Erase scrollback
                        self.buffer.scrollback.clear();
                    }
                    _ => {}
                }
            }
            'L' => {
                // Insert lines
                let row = self.buffer.cursor_row;
                for _ in 0..count {
                    if row < self.buffer.screen.len() {
                        self.buffer.screen.insert(row, String::with_capacity(128));
                        self.buffer.screen.pop();
                    }
                }
            }
            'M' => {
                // Delete lines
                let row = self.buffer.cursor_row;
                for _ in 0..count {
                    if row < self.buffer.screen.len() {
                        self.buffer.screen.remove(row);
                        self.buffer.screen.push(String::with_capacity(128));
                    }
                }
            }
            'P' => {
                // Delete characters
                let row = self
                    .buffer
                    .cursor_row
                    .min(self.buffer.screen.len().saturating_sub(1));
                if let Some(line) = self.buffer.screen.get_mut(row) {
                    let col = self.buffer.cursor_col;
                    let mut chars: Vec<char> = line.chars().collect();
                    if col < chars.len() {
                        chars.drain(col..usize::min(col + count, chars.len()));
                        *line = chars.into_iter().collect();
                    }
                }
            }
            '@' => {
                // Insert characters (spaces)
                let row = self
                    .buffer
                    .cursor_row
                    .min(self.buffer.screen.len().saturating_sub(1));
                if let Some(line) = self.buffer.screen.get_mut(row) {
                    let col = self.buffer.cursor_col.min(line.chars().count());
                    let mut chars: Vec<char> = line.chars().collect();
                    let spaces = vec![' '; count];
                    chars.splice(col..col, spaces);
                    *line = chars.into_iter().collect();
                }
            }
            's' => {
                // Save Cursor
                self.buffer.saved_cursor = (self.buffer.cursor_row, self.buffer.cursor_col);
            }
            'u' => {
                // Restore Cursor
                self.buffer.cursor_row = self
                    .buffer
                    .saved_cursor
                    .0
                    .min(self.buffer.visible_rows.saturating_sub(1));
                self.buffer.cursor_col = self
                    .buffer
                    .saved_cursor
                    .1
                    .min(self.buffer.cols.saturating_sub(1));
            }
            _ => {}
        }
    }

    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, byte: u8) {
        match byte {
            b'M' => {
                // Reverse Index (scroll down if at top, else cursor up)
                if self.buffer.cursor_row > 0 {
                    self.buffer.cursor_row -= 1;
                } else if !self.buffer.screen.is_empty() {
                    self.buffer.screen.pop();
                    self.buffer.screen.insert(0, String::with_capacity(128));
                }
            }
            b'E' => {
                // Next Line
                self.buffer.cursor_col = 0;
                self.execute(b'\n');
            }
            b'7' => {
                // DECSC - Save cursor
                self.buffer.saved_cursor = (self.buffer.cursor_row, self.buffer.cursor_col);
            }
            b'8' => {
                // DECRC - Restore cursor
                self.buffer.cursor_row = self
                    .buffer
                    .saved_cursor
                    .0
                    .min(self.buffer.visible_rows.saturating_sub(1));
                self.buffer.cursor_col = self
                    .buffer
                    .saved_cursor
                    .1
                    .min(self.buffer.cols.saturating_sub(1));
            }
            b'c' => {
                // RIS - Full Reset
                self.buffer.screen.clear();
                for _ in 0..self.buffer.visible_rows {
                    self.buffer.screen.push(String::with_capacity(128));
                }
                self.buffer.cursor_row = 0;
                self.buffer.cursor_col = 0;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_output_and_newline() {
        let mut buf = TerminalBuffer::new(80, 24, 1000);
        buf.process_bytes(b"Hello world\r\nLine 2");
        assert_eq!(buf.cursor_row, 1);
        assert_eq!(buf.cursor_col, 6);
        assert_eq!(buf.screen[0], "Hello world");
        assert_eq!(buf.screen[1], "Line 2");
    }

    #[test]
    fn test_carriage_return_overwrite_and_line_clear() {
        let mut buf = TerminalBuffer::new(80, 24, 1000);
        buf.process_bytes(b"--more--(31%)--(lines 48-70/239)--\r\x1b[KPort Id    Description");
        assert_eq!(buf.screen[0], "Port Id    Description");
    }

    #[test]
    fn test_nokia_cursor_repositioning_pagination() {
        let mut buf = TerminalBuffer::new(80, 5, 1000);
        // Page 1
        buf.process_bytes(b"Line 1\r\nLine 2\r\nLine 3\r\nLine 4\r\n--more--");
        assert_eq!(buf.screen[4], "--more--");

        // Nokia sends cursor home / clear to redraw page 2
        buf.process_bytes(b"\x1b[1;1H\x1b[2JPage 2 Line 1\r\nPage 2 Line 2\r\nPage 2 Line 3\r\nPage 2 Line 4\r\nPage 2 Line 5");
        assert_eq!(buf.screen[0], "Page 2 Line 1");
        assert_eq!(buf.screen[4], "Page 2 Line 5");
    }

    #[test]
    fn test_screen_scrolling_into_scrollback() {
        let mut buf = TerminalBuffer::new(80, 3, 1000);
        buf.process_bytes(b"Row 1\r\nRow 2\r\nRow 3\r\nRow 4\r\nRow 5");
        assert_eq!(buf.scrollback.len(), 2);
        assert_eq!(buf.scrollback[0], "Row 1");
        assert_eq!(buf.scrollback[1], "Row 2");
        assert_eq!(buf.screen[0], "Row 3");
        assert_eq!(buf.screen[1], "Row 4");
        assert_eq!(buf.screen[2], "Row 5");
    }

    #[test]
    fn test_dynamic_resizing() {
        let mut buf = TerminalBuffer::new(80, 24, 1000);
        buf.process_bytes(b"Line 1\r\nLine 2\r\nLine 3");
        buf.set_size(120, 40);
        assert_eq!(buf.cols, 120);
        assert_eq!(buf.visible_rows, 40);
        assert_eq!(buf.screen.len(), 40);
        assert_eq!(buf.screen[0], "Line 1");
        assert_eq!(buf.screen[1], "Line 2");
        assert_eq!(buf.screen[2], "Line 3");
    }
}

