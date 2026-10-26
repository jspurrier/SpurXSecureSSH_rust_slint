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
    pub in_alternate_screen: bool,
    pub application_cursor_keys: bool,
    pub application_keypad: bool,
    pub scroll_region: (usize, usize),
    saved_primary_screen: Vec<String>,
    saved_primary_cursor: (usize, usize),
    parser: Parser,
}

fn row_col_to_screen_offset(visible_text: &str, target_row: usize, target_column: usize) -> usize {
    let mut current_row = 0;
    let mut line_start_byte = 0;
    for (byte_index, byte_value) in visible_text.bytes().enumerate() {
        if current_row == target_row {
            break;
        }
        if byte_value == b'\n' {
            current_row += 1;
            line_start_byte = byte_index + 1;
        }
    }
    if current_row == target_row && line_start_byte <= visible_text.len() {
        let remaining_text = &visible_text[line_start_byte..];
        let line_byte_length = remaining_text.find('\n').unwrap_or(remaining_text.len());
        let line_slice = &remaining_text[..line_byte_length];
        let column_byte_offset = line_slice
            .char_indices()
            .nth(target_column)
            .map(|(char_byte_offset, _)| char_byte_offset)
            .unwrap_or(line_byte_length);
        return line_start_byte + column_byte_offset;
    }
    visible_text.len()
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
            in_alternate_screen: false,
            application_cursor_keys: false,
            application_keypad: false,
            scroll_region: (0, actual_rows.saturating_sub(1)),
            saved_primary_screen: Vec::new(),
            saved_primary_cursor: (0, 0),
            parser: Parser::new(),
        }
    }

    pub fn enter_alternate_screen(&mut self, clear_and_save: bool) {
        if !self.in_alternate_screen {
            self.in_alternate_screen = true;
            self.saved_primary_screen = self.screen.clone();
            self.saved_primary_cursor = (self.cursor_row, self.cursor_col);
            self.scroll_offset = 0;
            self.scroll_region = (0, self.visible_rows.saturating_sub(1));
            if clear_and_save {
                for line in self.screen.iter_mut() {
                    line.clear();
                }
                self.cursor_row = 0;
                self.cursor_col = 0;
            }
        }
    }

    pub fn exit_alternate_screen(&mut self, restore_cursor: bool) {
        if self.in_alternate_screen {
            self.in_alternate_screen = false;
            self.application_cursor_keys = false;
            self.application_keypad = false;
            if !self.saved_primary_screen.is_empty() {
                self.screen = std::mem::take(&mut self.saved_primary_screen);
            }
            while self.screen.len() < self.rows {
                self.screen.push(String::with_capacity(128));
            }
            if restore_cursor {
                self.cursor_row = self.saved_primary_cursor.0.min(self.visible_rows.saturating_sub(1));
                self.cursor_col = self.saved_primary_cursor.1.min(self.cols.saturating_sub(1));
            }
            self.scroll_offset = 0;
            self.scroll_region = (0, self.visible_rows.saturating_sub(1));
        }
    }

    pub fn set_size(&mut self, cols: usize, rows: usize) {
        self.cols = cols.max(20);
        let new_rows = rows.clamp(2, 300);
        self.scroll_region = (0, new_rows.saturating_sub(1));

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
        let mut all_lines = Vec::with_capacity(self.scrollback.len() + self.screen.len());
        all_lines.extend(self.scrollback.iter().cloned());
        all_lines.extend(self.screen.iter().cloned());
        all_lines
    }

    pub fn get_visible_text(&self) -> (String, usize, usize, usize, usize) {
        let total_line_count = if self.in_alternate_screen {
            self.screen.len()
        } else {
            self.scrollback.len() + self.screen.len()
        };
        let visible_row_count = self.visible_rows.max(2).min(300);

        if self.scroll_offset == 0 || self.in_alternate_screen {
            let visible_text = self.screen.join("\n");
            let cursor_line = self.cursor_row.min(self.screen.len().saturating_sub(1));
            let mut cursor_offset = 0;
            for row_index in 0..cursor_line {
                cursor_offset += self.screen[row_index].len() + 1;
            }
            if let Some(line_text) = self.screen.get(cursor_line) {
                cursor_offset += self.cursor_col.min(line_text.len());
            }
            (visible_text, cursor_offset, total_line_count, 0, visible_row_count)
        } else {
            let end_line_index = total_line_count
                .saturating_sub(self.scroll_offset)
                .min(total_line_count)
                .max(1);
            let start_line_index = end_line_index.saturating_sub(visible_row_count);

            let mut visible_lines = Vec::with_capacity(end_line_index - start_line_index);
            for line_index in start_line_index..end_line_index {
                if line_index < self.scrollback.len() {
                    visible_lines.push(self.scrollback[line_index].as_str());
                } else {
                    let screen_row_index = line_index - self.scrollback.len();
                    if screen_row_index < self.screen.len() {
                        visible_lines.push(self.screen[screen_row_index].as_str());
                    }
                }
            }
            let visible_text = visible_lines.join("\n");
            let cursor_offset = visible_text.len();
            (
                visible_text,
                cursor_offset,
                total_line_count,
                self.scroll_offset,
                visible_row_count,
            )
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

    pub fn scroll_up_in_region(&mut self, count: usize) {
        let (top, bottom) = self.scroll_region;
        let count = count.min(bottom.saturating_sub(top) + 1);
        for _ in 0..count {
            if top == 0 && bottom + 1 >= self.screen.len() && !self.in_alternate_screen {
                if !self.screen.is_empty() {
                    let old_top = self.screen.remove(0);
                    self.scrollback.push_back(old_top);
                    self.trim_scrollback();
                    self.screen.push(String::with_capacity(128));
                    if self.scroll_offset > 0 {
                        self.scroll_offset = (self.scroll_offset + 1).min(self.scrollback.len());
                    }
                }
            } else {
                if top < self.screen.len() {
                    self.screen.remove(top);
                    let insert_at = bottom.min(self.screen.len());
                    self.screen.insert(insert_at, String::with_capacity(128));
                }
            }
        }
    }

    pub fn scroll_down_in_region(&mut self, count: usize) {
        let (top, bottom) = self.scroll_region;
        let count = count.min(bottom.saturating_sub(top) + 1);
        for _ in 0..count {
            if bottom < self.screen.len() {
                self.screen.remove(bottom);
                self.screen.insert(top, String::with_capacity(128));
            }
        }
    }

    pub fn visible_line_range(&self) -> (usize, usize) {
        let total_line_count = self.scrollback.len() + self.screen.len();
        let visible_row_count = self.visible_rows.max(2).min(300);
        let end_line_index = total_line_count
            .saturating_sub(self.scroll_offset)
            .min(total_line_count)
            .max(1);
        let start_line_index = end_line_index.saturating_sub(visible_row_count);
        (start_line_index, end_line_index)
    }

    pub fn screen_offset_to_buffer_pos(&self, visible_text: &str, byte_offset: usize) -> (usize, usize) {
        let (start_line_index, _) = self.visible_line_range();
        let clamped_offset = byte_offset.min(visible_text.len());
        let mut char_boundary_offset = clamped_offset;
        while char_boundary_offset > 0 && !visible_text.is_char_boundary(char_boundary_offset) {
            char_boundary_offset -= 1;
        }
        let text_before_offset = &visible_text[..char_boundary_offset];
        let relative_row = text_before_offset
            .bytes()
            .filter(|&byte_value| byte_value == b'\n')
            .count();
        let line_start_byte = text_before_offset
            .rfind('\n')
            .map(|last_newline_index| last_newline_index + 1)
            .unwrap_or(0);
        let character_column = text_before_offset[line_start_byte..].chars().count();
        (start_line_index + relative_row, character_column)
    }

    pub fn viewport_cell_to_buffer_pos(&self, viewport_row: i32, viewport_column: i32) -> (usize, usize) {
        let (start_line_index, end_line_index) = self.visible_line_range();
        let visible_row_count = end_line_index.saturating_sub(start_line_index).max(1);
        if viewport_row < 0 {
            (start_line_index, 0)
        } else if (viewport_row as usize) >= visible_row_count {
            let bottom_row = end_line_index.saturating_sub(1);
            let line_char_count = self
                .line_at(bottom_row)
                .map(|line_text| line_text.chars().count())
                .unwrap_or(0);
            (bottom_row, line_char_count)
        } else {
            let global_row = start_line_index + (viewport_row as usize);
            let line_char_count = self
                .line_at(global_row)
                .map(|line_text| line_text.chars().count())
                .unwrap_or(0);
            let clamped_column = (viewport_column.max(0) as usize).min(line_char_count);
            (global_row, clamped_column)
        }
    }

    pub fn buffer_range_to_screen_offsets(
        &self,
        visible_text: &str,
        selection_start: (usize, usize),
        selection_end: (usize, usize),
    ) -> (usize, usize) {
        let is_forward_selection = selection_start <= selection_end;
        let (ordered_start, ordered_end) = if is_forward_selection {
            (selection_start, selection_end)
        } else {
            (selection_end, selection_start)
        };

        let (start_line_index, end_line_index) = self.visible_line_range();
        if ordered_end.0 < start_line_index {
            return (0, 0);
        }
        if ordered_start.0 >= end_line_index {
            let text_byte_length = visible_text.len();
            return (text_byte_length, text_byte_length);
        }

        let start_byte_offset = if ordered_start.0 < start_line_index {
            0
        } else {
            row_col_to_screen_offset(
                visible_text,
                ordered_start.0 - start_line_index,
                ordered_start.1,
            )
        };

        let end_byte_offset = if ordered_end.0 >= end_line_index {
            visible_text.len()
        } else {
            row_col_to_screen_offset(
                visible_text,
                ordered_end.0 - start_line_index,
                ordered_end.1,
            )
        };

        if is_forward_selection {
            (start_byte_offset, end_byte_offset)
        } else {
            (end_byte_offset, start_byte_offset)
        }
    }

    pub fn line_at(&self, global_row: usize) -> Option<&str> {
        let scrollback_length = self.scrollback.len();
        if global_row < scrollback_length {
            self.scrollback.get(global_row).map(|line_text| line_text.as_str())
        } else {
            self.screen
                .get(global_row - scrollback_length)
                .map(|line_text| line_text.as_str())
        }
    }

    pub fn total_lines(&self) -> usize {
        self.scrollback.len() + self.screen.len()
    }

    pub fn get_text_in_cell_range(
        &self,
        first_cell: (usize, usize),
        second_cell: (usize, usize),
    ) -> String {
        let (start_cell, end_cell) = if first_cell <= second_cell {
            (first_cell, second_cell)
        } else {
            (second_cell, first_cell)
        };
        if start_cell == end_cell {
            return String::new();
        }
        let total_line_count = self.total_lines();
        if total_line_count == 0 || start_cell.0 >= total_line_count {
            return String::new();
        }
        let (end_row, end_column) = if end_cell.0 >= total_line_count {
            (total_line_count - 1, usize::MAX)
        } else {
            end_cell
        };

        let slice_chars = |line_text: &str, start_column: usize, end_column: usize| -> String {
            line_text
                .chars()
                .skip(start_column)
                .take(end_column.saturating_sub(start_column))
                .collect()
        };

        let mut extracted_lines: Vec<String> = Vec::with_capacity(end_row - start_cell.0 + 1);
        for row_index in start_cell.0..=end_row {
            let line_text = self.line_at(row_index).unwrap_or("");
            let start_column = if row_index == start_cell.0 { start_cell.1 } else { 0 };
            let finish_column = if row_index == end_row { end_column } else { usize::MAX };
            extracted_lines.push(
                slice_chars(line_text, start_column, finish_column)
                    .trim_end()
                    .to_string(),
            );
        }
        extracted_lines.join("\n")
    }

    pub fn word_bounds_at(&self, global_row: usize, character_column: usize) -> Option<(usize, usize)> {
        let line_text = self.line_at(global_row)?;
        let line_characters: Vec<char> = line_text.chars().collect();
        if character_column >= line_characters.len() {
            return None;
        }
        let is_word_character = |character: char| {
            character.is_alphanumeric() || "._-/:@~%+".contains(character)
        };
        if !is_word_character(line_characters[character_column]) {
            return None;
        }
        let mut word_start_column = character_column;
        while word_start_column > 0 && is_word_character(line_characters[word_start_column - 1]) {
            word_start_column -= 1;
        }
        let mut word_end_column = character_column + 1;
        while word_end_column < line_characters.len()
            && is_word_character(line_characters[word_end_column])
        {
            word_end_column += 1;
        }
        Some((word_start_column, word_end_column))
    }

    pub fn get_selected_text(
        &self,
        selection_start: (usize, usize),
        selection_end: (usize, usize),
    ) -> String {
        let (ordered_start, ordered_end) = if selection_start <= selection_end {
            (selection_start, selection_end)
        } else {
            (selection_end, selection_start)
        };
        if ordered_start == ordered_end {
            return String::new();
        }
        let lines = self.get_lines();
        if lines.is_empty() || ordered_start.0 >= lines.len() {
            return String::new();
        }
        let end_line_index = ordered_end.0.min(lines.len() - 1);

        if ordered_start.0 == end_line_index {
            let line_text = &lines[ordered_start.0];
            let start_column = ordered_start.1.min(line_text.len());
            let end_column = ordered_end.1.min(line_text.len());
            if start_column < end_column {
                let mut start_byte_index = start_column;
                while start_byte_index > 0 && !line_text.is_char_boundary(start_byte_index) {
                    start_byte_index -= 1;
                }
                let mut end_byte_index = end_column;
                while end_byte_index < line_text.len() && !line_text.is_char_boundary(end_byte_index) {
                    end_byte_index += 1;
                }
                return line_text[start_byte_index..end_byte_index].to_string();
            } else {
                return String::new();
            }
        }

        let mut selected_lines = Vec::with_capacity(end_line_index - ordered_start.0 + 1);
        for line_index in ordered_start.0..=end_line_index {
            let line_text = &lines[line_index];
            if line_index == ordered_start.0 {
                let mut start_byte_index = ordered_start.1.min(line_text.len());
                while start_byte_index > 0 && !line_text.is_char_boundary(start_byte_index) {
                    start_byte_index -= 1;
                }
                selected_lines.push(line_text[start_byte_index..].to_string());
            } else if line_index == end_line_index {
                let mut end_byte_index = ordered_end.1.min(line_text.len());
                while end_byte_index < line_text.len() && !line_text.is_char_boundary(end_byte_index) {
                    end_byte_index += 1;
                }
                selected_lines.push(line_text[..end_byte_index].to_string());
            } else {
                selected_lines.push(line_text.clone());
            }
        }
        selected_lines.join("\n")
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
    fn print(&mut self, character: char) {
        if self.buffer.screen.is_empty() {
            for _ in 0..self.buffer.visible_rows {
                self.buffer.screen.push(String::with_capacity(128));
            }
        }

        if self.buffer.cursor_col >= self.buffer.cols {
            self.execute(b'\n');
            self.buffer.cursor_col = 0;
        }

        let target_row = self
            .buffer
            .cursor_row
            .min(self.buffer.screen.len().saturating_sub(1));
        let target_column = self.buffer.cursor_col;
        let line_text = &mut self.buffer.screen[target_row];

        let char_count = line_text.chars().count();
        if target_column >= char_count {
            let padding_count = (target_column - char_count).min(512);
            for _ in 0..padding_count {
                line_text.push(' ');
            }
            line_text.push(character);
        } else if character.is_ascii() && line_text.is_ascii() {
            let line_bytes = unsafe { line_text.as_bytes_mut() };
            if target_column < line_bytes.len() {
                line_bytes[target_column] = character as u8;
            }
        } else {
            let mut updated_line = String::with_capacity(line_text.len() + 4);
            let mut char_index = 0;
            let mut replaced = false;
            for existing_char in line_text.chars() {
                if char_index == target_column {
                    updated_line.push(character);
                    replaced = true;
                } else {
                    updated_line.push(existing_char);
                }
                char_index += 1;
            }
            if !replaced {
                updated_line.push(character);
            }
            *line_text = updated_line;
        }
        self.buffer.cursor_col += 1;
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\n' | 0x0b | 0x0c => {
                let (_top_margin, bottom_margin) = self.buffer.scroll_region;
                if self.buffer.cursor_row < bottom_margin {
                    if self.buffer.cursor_row + 1 < self.buffer.visible_rows {
                        self.buffer.cursor_row += 1;
                    }
                } else if self.buffer.cursor_row == bottom_margin {
                    self.buffer.scroll_up_in_region(1);
                } else if self.buffer.cursor_row + 1 < self.buffer.visible_rows {
                    self.buffer.cursor_row += 1;
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
                    let target_row = self
                        .buffer
                        .cursor_row
                        .min(self.buffer.screen.len().saturating_sub(1));
                    if let Some(line_text) = self.buffer.screen.get_mut(target_row) {
                        let target_column = self.buffer.cursor_col;
                        let line_characters: Vec<char> = line_text.chars().collect();
                        if target_column < line_characters.len() {
                            let mut updated_line = String::with_capacity(line_text.len());
                            for (char_index, existing_char) in
                                line_characters.into_iter().enumerate()
                            {
                                if char_index != target_column {
                                    updated_line.push(existing_char);
                                }
                            }
                            *line_text = updated_line;
                        }
                    }
                }
            }
            b'\t' => {
                let next_tab_stop = ((self.buffer.cursor_col / 8) + 1) * 8;
                self.buffer.cursor_col = next_tab_stop.min(self.buffer.cols.saturating_sub(1));
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
            .and_then(|param_group| param_group.first().copied())
            .unwrap_or(1) as usize;
        let count = if first_param == 0 { 1 } else { first_param };

        match action {
            'A' => {
                self.buffer.cursor_row = self.buffer.cursor_row.saturating_sub(count);
            }
            'B' | 'e' => {
                let max_row = self.buffer.visible_rows.saturating_sub(1);
                self.buffer.cursor_row = (self.buffer.cursor_row + count).min(max_row);
            }
            'C' | 'a' => {
                let max_col = self.buffer.cols.saturating_sub(1);
                self.buffer.cursor_col = (self.buffer.cursor_col + count).min(max_col);
            }
            'D' => {
                self.buffer.cursor_col = self.buffer.cursor_col.saturating_sub(count);
            }
            'H' | 'f' => {
                let mut param_iter = params.iter();
                let target_row = param_iter
                    .next()
                    .and_then(|param_group| param_group.first().copied())
                    .unwrap_or(1)
                    .max(1) as usize
                    - 1;
                let target_column = param_iter
                    .next()
                    .and_then(|param_group| param_group.first().copied())
                    .unwrap_or(1)
                    .max(1) as usize
                    - 1;

                self.buffer.cursor_row = target_row.min(self.buffer.visible_rows.saturating_sub(1));
                self.buffer.cursor_col = target_column.min(self.buffer.cols.saturating_sub(1));
            }
            'G' | '`' => {
                let target_column = count.max(1) - 1;
                self.buffer.cursor_col = target_column.min(self.buffer.cols.saturating_sub(1));
            }
            'd' => {
                let target_row = count.max(1) - 1;
                self.buffer.cursor_row = target_row.min(self.buffer.visible_rows.saturating_sub(1));
            }
            'E' => {
                self.buffer.cursor_col = 0;
                let max_row = self.buffer.visible_rows.saturating_sub(1);
                self.buffer.cursor_row = (self.buffer.cursor_row + count).min(max_row);
            }
            'F' => {
                self.buffer.cursor_col = 0;
                self.buffer.cursor_row = self.buffer.cursor_row.saturating_sub(count);
            }
            'K' => {
                let target_row = self
                    .buffer
                    .cursor_row
                    .min(self.buffer.screen.len().saturating_sub(1));
                if let Some(line_text) = self.buffer.screen.get_mut(target_row) {
                    let erase_mode = params
                        .iter()
                        .next()
                        .and_then(|param_group| param_group.first().copied())
                        .unwrap_or(0);
                    let cursor_column = self.buffer.cursor_col;
                    let line_characters: Vec<char> = line_text.chars().collect();
                    match erase_mode {
                        0 => {
                            if cursor_column < line_characters.len() {
                                *line_text = line_characters[..cursor_column].iter().collect();
                            }
                        }
                        1 => {
                            let mut updated_line =
                                " ".repeat(cursor_column.min(line_characters.len()));
                            if cursor_column < line_characters.len() {
                                updated_line.extend(line_characters[cursor_column..].iter());
                            }
                            *line_text = updated_line;
                        }
                        2 => {
                            line_text.clear();
                        }
                        _ => {}
                    }
                }
            }
            'J' => {
                let erase_mode = params
                    .iter()
                    .next()
                    .and_then(|param_group| param_group.first().copied())
                    .unwrap_or(0);
                match erase_mode {
                    0 => {
                        let cursor_row = self
                            .buffer
                            .cursor_row
                            .min(self.buffer.screen.len().saturating_sub(1));
                        let cursor_column = self.buffer.cursor_col;
                        if let Some(line_text) = self.buffer.screen.get_mut(cursor_row) {
                            let line_characters: Vec<char> = line_text.chars().collect();
                            if cursor_column < line_characters.len() {
                                *line_text = line_characters[..cursor_column].iter().collect();
                            }
                        }
                        for row_index in (cursor_row + 1)..self.buffer.screen.len() {
                            self.buffer.screen[row_index].clear();
                        }
                    }
                    1 => {
                        let cursor_row = self
                            .buffer
                            .cursor_row
                            .min(self.buffer.screen.len().saturating_sub(1));
                        for row_index in 0..cursor_row {
                            if row_index < self.buffer.screen.len() {
                                self.buffer.screen[row_index].clear();
                            }
                        }
                        let cursor_column = self.buffer.cursor_col;
                        if let Some(line_text) = self.buffer.screen.get_mut(cursor_row) {
                            let line_characters: Vec<char> = line_text.chars().collect();
                            let mut updated_line =
                                " ".repeat(cursor_column.min(line_characters.len()));
                            if cursor_column < line_characters.len() {
                                updated_line.extend(line_characters[cursor_column..].iter());
                            }
                            *line_text = updated_line;
                        }
                    }
                    2 => {
                        for line_text in self.buffer.screen.iter_mut() {
                            line_text.clear();
                        }
                    }
                    3 => {
                        self.buffer.scrollback.clear();
                    }
                    _ => {}
                }
            }
            'L' => {
                let (top_margin, bottom_margin) = self.buffer.scroll_region;
                let cursor_row = self.buffer.cursor_row;
                if cursor_row >= top_margin && cursor_row <= bottom_margin {
                    for _ in 0..count {
                        if bottom_margin < self.buffer.screen.len() {
                            self.buffer.screen.remove(bottom_margin);
                            self.buffer
                                .screen
                                .insert(cursor_row, String::with_capacity(128));
                        }
                    }
                }
            }
            'M' => {
                let (top_margin, bottom_margin) = self.buffer.scroll_region;
                let cursor_row = self.buffer.cursor_row;
                if cursor_row >= top_margin && cursor_row <= bottom_margin {
                    for _ in 0..count {
                        if cursor_row < self.buffer.screen.len() {
                            self.buffer.screen.remove(cursor_row);
                            let insert_row = bottom_margin.min(self.buffer.screen.len());
                            self.buffer
                                .screen
                                .insert(insert_row, String::with_capacity(128));
                        }
                    }
                }
            }
            'S' => {
                self.buffer.scroll_up_in_region(count);
            }
            'T' => {
                self.buffer.scroll_down_in_region(count);
            }
            'r' => {
                if _intermediates.is_empty() {
                    let mut param_iter = params.iter();
                    let top_param = param_iter
                        .next()
                        .and_then(|param_group| param_group.first().copied())
                        .unwrap_or(1) as usize;
                    let bottom_param = param_iter
                        .next()
                        .and_then(|param_group| param_group.first().copied())
                        .unwrap_or(self.buffer.visible_rows as u16) as usize;
                    let top_index = top_param.max(1).saturating_sub(1);
                    let bottom_index = bottom_param
                        .max(top_param)
                        .min(self.buffer.visible_rows)
                        .saturating_sub(1);
                    if top_index < bottom_index {
                        self.buffer.scroll_region = (top_index, bottom_index);
                    } else {
                        self.buffer.scroll_region =
                            (0, self.buffer.visible_rows.saturating_sub(1));
                    }
                    self.buffer.cursor_row = self.buffer.scroll_region.0;
                    self.buffer.cursor_col = 0;
                }
            }
            'P' => {
                let target_row = self
                    .buffer
                    .cursor_row
                    .min(self.buffer.screen.len().saturating_sub(1));
                if let Some(line_text) = self.buffer.screen.get_mut(target_row) {
                    let cursor_column = self.buffer.cursor_col;
                    let mut line_characters: Vec<char> = line_text.chars().collect();
                    if cursor_column < line_characters.len() {
                        line_characters
                            .drain(cursor_column..usize::min(cursor_column + count, line_characters.len()));
                        *line_text = line_characters.into_iter().collect();
                    }
                }
            }
            '@' => {
                let target_row = self
                    .buffer
                    .cursor_row
                    .min(self.buffer.screen.len().saturating_sub(1));
                if let Some(line_text) = self.buffer.screen.get_mut(target_row) {
                    let cursor_column = self.buffer.cursor_col.min(line_text.chars().count());
                    let mut line_characters: Vec<char> = line_text.chars().collect();
                    let spaces = vec![' '; count];
                    line_characters.splice(cursor_column..cursor_column, spaces);
                    *line_text = line_characters.into_iter().collect();
                }
            }
            's' => {
                self.buffer.saved_cursor = (self.buffer.cursor_row, self.buffer.cursor_col);
            }
            'u' => {
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
            'h' => {
                if _intermediates.contains(&b'?') {
                    for param_slice in params.iter() {
                        if let Some(&mode_number) = param_slice.first() {
                            match mode_number {
                                1 => {
                                    self.buffer.application_cursor_keys = true;
                                }
                                1049 | 47 | 1047 => {
                                    self.buffer.enter_alternate_screen(mode_number == 1049);
                                }
                                25 => {
                                    self.buffer.show_cursor = true;
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
            'l' => {
                if _intermediates.contains(&b'?') {
                    for param_slice in params.iter() {
                        if let Some(&mode_number) = param_slice.first() {
                            match mode_number {
                                1 => {
                                    self.buffer.application_cursor_keys = false;
                                }
                                1049 | 47 | 1047 => {
                                    self.buffer.exit_alternate_screen(mode_number == 1049);
                                }
                                25 => {
                                    self.buffer.show_cursor = false;
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, byte: u8) {
        match byte {
            b'M' => {
                let (top_margin, _bottom_margin) = self.buffer.scroll_region;
                if self.buffer.cursor_row > top_margin {
                    self.buffer.cursor_row -= 1;
                } else if self.buffer.cursor_row == top_margin {
                    self.buffer.scroll_down_in_region(1);
                } else if self.buffer.cursor_row > 0 {
                    self.buffer.cursor_row -= 1;
                }
            }
            b'D' => {
                self.execute(b'\n');
            }
            b'E' => {
                self.buffer.cursor_col = 0;
                self.execute(b'\n');
            }
            b'=' => {
                self.buffer.application_keypad = true;
            }
            b'>' => {
                self.buffer.application_keypad = false;
            }
            b'7' => {
                self.buffer.saved_cursor = (self.buffer.cursor_row, self.buffer.cursor_col);
            }
            b'8' => {
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
        let mut terminal_buffer = TerminalBuffer::new(80, 24, 1000);
        terminal_buffer.process_bytes(b"Hello world\r\nLine 2");
        assert_eq!(terminal_buffer.cursor_row, 1);
        assert_eq!(terminal_buffer.cursor_col, 6);
        assert_eq!(terminal_buffer.screen[0], "Hello world");
        assert_eq!(terminal_buffer.screen[1], "Line 2");
    }

    #[test]
    fn test_carriage_return_overwrite_and_line_clear() {
        let mut terminal_buffer = TerminalBuffer::new(80, 24, 1000);
        terminal_buffer
            .process_bytes(b"--more--(31%)--(lines 48-70/239)--\r\x1b[KPort Id    Description");
        assert_eq!(terminal_buffer.screen[0], "Port Id    Description");
    }

    #[test]
    fn test_nokia_cursor_repositioning_pagination() {
        let mut terminal_buffer = TerminalBuffer::new(80, 5, 1000);
        terminal_buffer.process_bytes(b"Line 1\r\nLine 2\r\nLine 3\r\nLine 4\r\n--more--");
        assert_eq!(terminal_buffer.screen[4], "--more--");

        terminal_buffer.process_bytes(b"\x1b[1;1H\x1b[2JPage 2 Line 1\r\nPage 2 Line 2\r\nPage 2 Line 3\r\nPage 2 Line 4\r\nPage 2 Line 5");
        assert_eq!(terminal_buffer.screen[0], "Page 2 Line 1");
        assert_eq!(terminal_buffer.screen[4], "Page 2 Line 5");
    }

    #[test]
    fn test_screen_scrolling_into_scrollback() {
        let mut terminal_buffer = TerminalBuffer::new(80, 3, 1000);
        terminal_buffer.process_bytes(b"Row 1\r\nRow 2\r\nRow 3\r\nRow 4\r\nRow 5");
        assert_eq!(terminal_buffer.scrollback.len(), 2);
        assert_eq!(terminal_buffer.scrollback[0], "Row 1");
        assert_eq!(terminal_buffer.scrollback[1], "Row 2");
        assert_eq!(terminal_buffer.screen[0], "Row 3");
        assert_eq!(terminal_buffer.screen[1], "Row 4");
        assert_eq!(terminal_buffer.screen[2], "Row 5");
    }

    #[test]
    fn test_dynamic_resizing() {
        let mut terminal_buffer = TerminalBuffer::new(80, 24, 1000);
        terminal_buffer.process_bytes(b"Line 1\r\nLine 2\r\nLine 3");
        terminal_buffer.set_size(120, 40);
        assert_eq!(terminal_buffer.cols, 120);
        assert_eq!(terminal_buffer.visible_rows, 40);
        assert_eq!(terminal_buffer.screen.len(), 40);
        assert_eq!(terminal_buffer.screen[0], "Line 1");
        assert_eq!(terminal_buffer.screen[1], "Line 2");
        assert_eq!(terminal_buffer.screen[2], "Line 3");
    }

    #[test]
    fn test_screen_offset_to_buffer_pos() {
        let mut terminal_buffer = TerminalBuffer::new(80, 3, 1000);
        terminal_buffer.process_bytes(b"Row 1\r\nRow 2\r\nRow 3\r\nRow 4\r\nRow 5");
        let (visible_text, _, _, _, _) = terminal_buffer.get_visible_text();
        assert_eq!(visible_text, "Row 3\nRow 4\nRow 5");

        let first_pos = terminal_buffer.screen_offset_to_buffer_pos(&visible_text, 0);
        assert_eq!(first_pos, (2, 0));

        let second_pos = terminal_buffer.screen_offset_to_buffer_pos(&visible_text, 2);
        assert_eq!(second_pos, (2, 2));

        let third_pos = terminal_buffer.screen_offset_to_buffer_pos(&visible_text, 6);
        assert_eq!(third_pos, (3, 0));

        terminal_buffer.scroll_by(2);
        let (history_text, _, _, _, _) = terminal_buffer.get_visible_text();
        let history_pos = terminal_buffer.screen_offset_to_buffer_pos(&history_text, 0);
        assert_eq!(history_pos, (0, 0));
    }

    #[test]
    fn test_buffer_range_to_screen_offsets() {
        let mut terminal_buffer = TerminalBuffer::new(80, 3, 1000);
        terminal_buffer.process_bytes(b"Line 0\r\nLine 1\r\nLine 2\r\nLine 3\r\nLine 4");
        let (visible_text, _, _, _, _) = terminal_buffer.get_visible_text();
        assert_eq!(visible_text, "Line 2\nLine 3\nLine 4");

        let (start_offset, end_offset) =
            terminal_buffer.buffer_range_to_screen_offsets(&visible_text, (2, 2), (3, 4));
        assert_eq!(start_offset, 2);
        assert_eq!(end_offset, 7 + 4);

        let (above_start_offset, above_end_offset) =
            terminal_buffer.buffer_range_to_screen_offsets(&visible_text, (0, 0), (3, 4));
        assert_eq!(above_start_offset, 0);
        assert_eq!(above_end_offset, 11);

        terminal_buffer.scroll_by(2);
        let (history_text, _, _, _, _) = terminal_buffer.get_visible_text();
        let (below_start_offset, below_end_offset) =
            terminal_buffer.buffer_range_to_screen_offsets(&history_text, (1, 2), (4, 3));
        assert_eq!(below_start_offset, 7 + 2);
        assert_eq!(below_end_offset, history_text.len());
    }

    #[test]
    fn test_get_selected_text_multi_screen() {
        let mut terminal_buffer = TerminalBuffer::new(80, 3, 1000);
        terminal_buffer.process_bytes(b"Config Alpha\r\nInterface GigabitEthernet0/1\r\n ip address 10.0.0.1\r\n no shutdown\r\nexit");
        let all_lines = terminal_buffer.get_lines();
        assert_eq!(all_lines.len(), 5);

        let selected_text = terminal_buffer.get_selected_text((1, 0), (3, 12));
        assert_eq!(
            selected_text,
            "Interface GigabitEthernet0/1\n ip address 10.0.0.1\n no shutdown"
        );

        let partial_text = terminal_buffer.get_selected_text((1, 10), (1, 25));
        assert_eq!(partial_text, "GigabitEthernet");
    }

    #[test]
    fn test_cell_range_copy_is_independent_of_scroll() {
        let mut terminal_buffer = TerminalBuffer::new(80, 3, 1000);
        terminal_buffer.process_bytes(b"Line 0\r\nLine 1\r\nLine 2\r\nLine 3\r\nLine 4");
        let expected_text = "ne 1\nLine 2\nLine 3\nLi";
        assert_eq!(
            terminal_buffer.get_text_in_cell_range((1, 2), (4, 2)),
            expected_text
        );
        terminal_buffer.scroll_by(2);
        assert_eq!(
            terminal_buffer.get_text_in_cell_range((1, 2), (4, 2)),
            expected_text
        );
        assert_eq!(
            terminal_buffer.get_text_in_cell_range((4, 2), (1, 2)),
            expected_text
        );
        assert_eq!(
            terminal_buffer.get_text_in_cell_range((3, 0), (99, 9999)),
            "Line 3\nLine 4"
        );
    }

    #[test]
    fn test_cell_range_copy_utf8_and_trailing_space() {
        let mut terminal_buffer = TerminalBuffer::new(80, 3, 1000);
        terminal_buffer.process_bytes("├── eth0   \r\n└── eth1".as_bytes());
        assert_eq!(
            terminal_buffer.get_text_in_cell_range((0, 4), (0, 100)),
            "eth0"
        );
        assert_eq!(
            terminal_buffer.get_text_in_cell_range((0, 0), (1, 3)),
            "├── eth0\n└──"
        );
    }

    #[test]
    fn test_word_bounds_at() {
        let mut terminal_buffer = TerminalBuffer::new(80, 3, 1000);
        terminal_buffer.process_bytes(b"Router# ping 10.0.0.1 via Gi0/1");
        let line_text = terminal_buffer.line_at(0).unwrap().to_string();
        let ip_column = line_text.find("10.0").unwrap() + 3;
        let (word_start, word_end) = terminal_buffer.word_bounds_at(0, ip_column).unwrap();
        assert_eq!(&line_text[word_start..word_end], "10.0.0.1");
        let interface_column = line_text.find("Gi0").unwrap();
        let (word_start, word_end) = terminal_buffer.word_bounds_at(0, interface_column).unwrap();
        assert_eq!(&line_text[word_start..word_end], "Gi0/1");
        let (word_start, word_end) = terminal_buffer.word_bounds_at(0, 1).unwrap();
        assert_eq!(&line_text[word_start..word_end], "Router");
        assert!(terminal_buffer
            .word_bounds_at(0, line_text.find(' ').unwrap())
            .is_none());
    }

    #[test]
    fn test_alternate_screen_enter_and_exit() {
        let mut terminal_buffer = TerminalBuffer::new(80, 24, 1000);
        terminal_buffer.process_bytes(b"user@host:~$ ls\r\nfile1.txt\r\n");
        assert!(!terminal_buffer.in_alternate_screen);
        assert_eq!(terminal_buffer.screen[0], "user@host:~$ ls");

        terminal_buffer.process_bytes(b"\x1b[?1049h");
        assert!(terminal_buffer.in_alternate_screen);
        assert_eq!(terminal_buffer.screen[0], "");

        terminal_buffer.process_bytes(b"GNU nano 7.2\r\nFile: test.txt");
        assert_eq!(terminal_buffer.screen[0], "GNU nano 7.2");
        assert_eq!(terminal_buffer.screen[1], "File: test.txt");

        terminal_buffer.process_bytes(b"\x1b[?1049l");
        assert!(!terminal_buffer.in_alternate_screen);
        assert_eq!(terminal_buffer.screen[0], "user@host:~$ ls");
    }

    #[test]
    fn test_scrolling_region_decstbm() {
        let mut terminal_buffer = TerminalBuffer::new(80, 24, 1000);
        let mut initial_text = String::new();
        for line_number in 0..24 {
            if line_number > 0 {
                initial_text.push_str("\r\n");
            }
            initial_text.push_str(&format!("Line {:02}", line_number));
        }
        terminal_buffer.process_bytes(initial_text.as_bytes());
        assert_eq!(terminal_buffer.screen[0], "Line 00");
        assert_eq!(terminal_buffer.screen[1], "Line 01");
        assert_eq!(terminal_buffer.screen[21], "Line 21");
        assert_eq!(terminal_buffer.screen[22], "Line 22");
        assert_eq!(terminal_buffer.screen[23], "Line 23");

        terminal_buffer.process_bytes(b"\x1b[2;22r");
        assert_eq!(terminal_buffer.scroll_region, (1, 21));

        terminal_buffer.process_bytes(b"\x1b[22d\n");

        assert_eq!(terminal_buffer.screen[0], "Line 00");
        assert_eq!(terminal_buffer.screen[1], "Line 02");
        assert_eq!(terminal_buffer.screen[21], "");
        assert_eq!(terminal_buffer.screen[22], "Line 22");
        assert_eq!(terminal_buffer.screen[23], "Line 23");
    }

    #[test]
    fn test_nano_raw_stream() {
        if let Ok(bytes) = std::fs::read("target/nano_raw.bin") {
            let mut terminal_buffer = TerminalBuffer::new(80, 24, 1000);
            terminal_buffer.process_bytes(&bytes);
            for row_index in 0..24 {
                println!("Line {:02}: {:?}", row_index, terminal_buffer.screen[row_index]);
            }
            assert!(terminal_buffer.in_alternate_screen);
        }
    }

    #[test]
    fn test_decckm_and_keypad_modes() {
        let mut terminal_buffer = TerminalBuffer::new(80, 24, 1000);
        assert!(!terminal_buffer.application_cursor_keys);
        assert!(!terminal_buffer.application_keypad);

        terminal_buffer.process_bytes(b"\x1b[?1h");
        assert!(terminal_buffer.application_cursor_keys);

        terminal_buffer.process_bytes(b"\x1b=");
        assert!(terminal_buffer.application_keypad);

        terminal_buffer.process_bytes(b"\x1b>");
        assert!(!terminal_buffer.application_keypad);

        terminal_buffer.process_bytes(b"\x1b[?1l");
        assert!(!terminal_buffer.application_cursor_keys);

        terminal_buffer.process_bytes(b"\x1b[?1h\x1b=\x1b[?1049h");
        assert!(terminal_buffer.application_cursor_keys);
        assert!(terminal_buffer.application_keypad);
        assert!(terminal_buffer.in_alternate_screen);

        terminal_buffer.process_bytes(b"\x1b[?1049l");
        assert!(!terminal_buffer.application_cursor_keys);
        assert!(!terminal_buffer.application_keypad);
    }

    #[test]
    fn test_viewport_cell_to_buffer_pos_and_multi_screen_scroll_select() {
        let mut terminal_buffer = TerminalBuffer::new(80, 3, 1000);
        terminal_buffer.process_bytes(
            b"Line 00 Alpha\r\nLine 01 Beta\r\nLine 02 Gamma\r\nLine 03 Delta\r\nLine 04 Epsilon\r\nLine 05 Zeta\r\nLine 06 Eta\r\nLine 07 Theta\r\nLine 08 Iota",
        );

        let anchor_cell = terminal_buffer.viewport_cell_to_buffer_pos(2, 7);
        assert_eq!(anchor_cell, (8, 7));

        let top_clamped_cell = terminal_buffer.viewport_cell_to_buffer_pos(-3, -5);
        assert_eq!(top_clamped_cell, (6, 0));

        terminal_buffer.scroll_by(3);
        let middle_cell = terminal_buffer.viewport_cell_to_buffer_pos(-2, 0);
        assert_eq!(middle_cell, (3, 0));

        terminal_buffer.scroll_by(3);
        let final_cell = terminal_buffer.viewport_cell_to_buffer_pos(1, 8);
        assert_eq!(final_cell, (1, 8));

        let copied_text = terminal_buffer.get_text_in_cell_range(anchor_cell, final_cell);
        assert_eq!(
            copied_text,
            "Beta\nLine 02 Gamma\nLine 03 Delta\nLine 04 Epsilon\nLine 05 Zeta\nLine 06 Eta\nLine 07 Theta\nLine 08"
        );

        let bottom_clamped_cell = terminal_buffer.viewport_cell_to_buffer_pos(10, 50);
        assert_eq!(bottom_clamped_cell, (2, "Line 02 Gamma".len()));
    }
}

