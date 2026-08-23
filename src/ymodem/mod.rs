use serde::Serialize;
use std::fs;
use std::path::Path;
use tokio::sync::mpsc;

pub const SOH: u8 = 0x01;
pub const STX: u8 = 0x02;
pub const EOT: u8 = 0x04;
pub const ACK: u8 = 0x06;
pub const NAK: u8 = 0x15;
pub const CAN: u8 = 0x18;
pub const C_CHAR: u8 = 0x43;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YmodemState {
    WaitingForInitialC,
    WaitingForHeaderAck,
    WaitingForHeaderC,
    SendingDataBlocks,
    WaitingForFirstEotAck,
    WaitingForSecondEotAck,
    WaitingForTermC,
    SendingTermBlock,
    WaitingForTermAck,
    Finished,
    Failed,
}

#[derive(Serialize, Clone, Debug)]
pub struct YmodemProgress {
    pub bytes_sent: usize,
    pub total_bytes: usize,
    pub speed: f64,
    pub filename: String,
    pub state: String,
}

pub struct YmodemSender {
    pub file_name: String,
    pub file_size: usize,
    pub data: Vec<u8>,
    pub current_block: usize,
    pub offset: usize,
    pub state: YmodemState,
    pub session_id: String,
    pub progress_tx: Option<mpsc::UnboundedSender<YmodemProgress>>,
    pub start_time: std::time::Instant,
    pub error_msg: Option<String>,
    pub retries: usize,
    pub max_retries: usize,
}

impl YmodemSender {
    pub fn new(
        file_path: &str,
        session_id: String,
        progress_tx: Option<mpsc::UnboundedSender<YmodemProgress>>,
    ) -> Result<Self, String> {
        let path = Path::new(file_path);
        let file_name = path
            .file_name()
            .ok_or_else(|| "Invalid file path".to_string())?
            .to_string_lossy()
            .to_string();

        let data = fs::read(path).map_err(|e| format!("Failed to read file: {}", e))?;
        let file_size = data.len();

        Ok(Self {
            file_name,
            file_size,
            data,
            current_block: 0,
            offset: 0,
            state: YmodemState::WaitingForInitialC,
            session_id,
            progress_tx,
            start_time: std::time::Instant::now(),
            error_msg: None,
            retries: 0,
            max_retries: 10,
        })
    }

    pub fn handle_byte(&mut self, byte: u8) -> Option<Vec<u8>> {
        if self.state == YmodemState::Finished || self.state == YmodemState::Failed {
            return None;
        }

        if byte == CAN {
            self.state = YmodemState::Failed;
            self.error_msg = Some("Transfer cancelled by remote".to_string());
            self.emit_progress("Cancelled by remote");
            return None;
        }

        match self.state {
            YmodemState::WaitingForInitialC => {
                if byte == C_CHAR {
                    let packet = self.make_header_block();
                    self.state = YmodemState::WaitingForHeaderAck;
                    self.retries = 0;
                    self.emit_progress("Sending header");
                    return Some(packet);
                }
            }
            YmodemState::WaitingForHeaderAck => {
                if byte == ACK {
                    self.state = YmodemState::WaitingForHeaderC;
                    self.retries = 0;
                    self.emit_progress("Header acknowledged");
                } else if byte == NAK {
                    if self.retries < self.max_retries {
                        self.retries += 1;
                        return Some(self.make_header_block());
                    } else {
                        self.fail("Too many NAKs sending header");
                    }
                }
            }
            YmodemState::WaitingForHeaderC => {
                if byte == C_CHAR {
                    self.state = YmodemState::SendingDataBlocks;
                    self.current_block = 1;
                    self.offset = 0;
                    self.retries = 0;
                    self.emit_progress("Sending data");
                    return Some(self.make_data_block());
                }
            }
            YmodemState::SendingDataBlocks => {
                if byte == ACK {
                    let sent_in_this_block = std::cmp::min(1024, self.file_size - self.offset);
                    self.offset += sent_in_this_block;
                    self.emit_progress("Sending data");

                    if self.offset >= self.file_size {
                        self.state = YmodemState::WaitingForFirstEotAck;
                        self.retries = 0;
                        return Some(vec![EOT]);
                    } else {
                        self.current_block += 1;
                        self.retries = 0;
                        return Some(self.make_data_block());
                    }
                } else if byte == NAK {
                    if self.retries < self.max_retries {
                        self.retries += 1;
                        return Some(self.make_data_block());
                    } else {
                        self.fail("Too many NAKs sending data block");
                    }
                }
            }
            YmodemState::WaitingForFirstEotAck => {
                if byte == NAK {
                    self.state = YmodemState::WaitingForSecondEotAck;
                    self.retries = 0;
                    return Some(vec![EOT]);
                } else if byte == ACK {
                    self.state = YmodemState::WaitingForTermC;
                    self.retries = 0;
                }
            }
            YmodemState::WaitingForSecondEotAck => {
                if byte == ACK {
                    self.state = YmodemState::WaitingForTermC;
                    self.retries = 0;
                } else if byte == NAK || byte == C_CHAR {
                    self.state = YmodemState::SendingTermBlock;
                    self.retries = 0;
                    return Some(self.make_term_block());
                }
            }
            YmodemState::WaitingForTermC => {
                if byte == C_CHAR {
                    self.state = YmodemState::WaitingForTermAck;
                    self.retries = 0;
                    return Some(self.make_term_block());
                }
            }
            YmodemState::SendingTermBlock | YmodemState::WaitingForTermAck => {
                if byte == ACK {
                    self.state = YmodemState::Finished;
                    self.emit_progress("Finished");
                } else if byte == NAK {
                    if self.retries < self.max_retries {
                        self.retries += 1;
                        return Some(self.make_term_block());
                    } else {
                        self.fail("Too many NAKs sending term block");
                    }
                }
            }
            _ => {}
        }
        None
    }

    pub fn handle_data(&mut self, data: &[u8]) -> Option<Vec<u8>> {
        let mut response = None;
        for &byte in data {
            if let Some(r) = self.handle_byte(byte) {
                response = Some(r);
            }
        }
        response
    }

    fn fail(&mut self, msg: &str) {
        self.state = YmodemState::Failed;
        self.error_msg = Some(msg.to_string());
        self.emit_progress(msg);
    }

    fn emit_progress(&self, state_name: &str) {
        let elapsed = self.start_time.elapsed().as_secs_f64();
        let speed = if elapsed > 0.0 {
            (self.offset as f64) / 1024.0 / elapsed
        } else {
            0.0
        };

        if let Some(ref tx) = self.progress_tx {
            let _ = tx.send(YmodemProgress {
                bytes_sent: self.offset,
                total_bytes: self.file_size,
                speed,
                filename: self.file_name.clone(),
                state: state_name.to_string(),
            });
        }
    }

    fn make_header_block(&self) -> Vec<u8> {
        let mut block = vec![0u8; 128];
        let name_bytes = self.file_name.as_bytes();
        let size_str = self.file_size.to_string();
        let size_bytes = size_str.as_bytes();

        let mut idx = 0;
        for &b in name_bytes {
            if idx < 120 {
                block[idx] = b;
                idx += 1;
            }
        }
        block[idx] = 0;
        idx += 1;

        for &b in size_bytes {
            if idx < 127 {
                block[idx] = b;
                idx += 1;
            }
        }
        block[idx] = 0;

        let mut packet = Vec::with_capacity(3 + 128 + 2);
        packet.push(SOH);
        packet.push(0x00);
        packet.push(0xFF);
        packet.extend_from_slice(&block);

        let crc = crc16(&block);
        packet.push((crc >> 8) as u8);
        packet.push((crc & 0xFF) as u8);
        packet
    }

    fn make_data_block(&self) -> Vec<u8> {
        let mut block = vec![0u8; 1024];
        let remaining = self.file_size - self.offset;
        let to_copy = std::cmp::min(1024, remaining);
        block[..to_copy].copy_from_slice(&self.data[self.offset..self.offset + to_copy]);

        let block_num = (self.current_block % 256) as u8;
        let block_num_inv = !block_num;

        let mut packet = Vec::with_capacity(3 + 1024 + 2);
        packet.push(STX);
        packet.push(block_num);
        packet.push(block_num_inv);
        packet.extend_from_slice(&block);

        let crc = crc16(&block);
        packet.push((crc >> 8) as u8);
        packet.push((crc & 0xFF) as u8);
        packet
    }

    fn make_term_block(&self) -> Vec<u8> {
        let block = vec![0u8; 128];
        let mut packet = Vec::with_capacity(3 + 128 + 2);
        packet.push(SOH);
        packet.push(0x00);
        packet.push(0xFF);
        packet.extend_from_slice(&block);

        let crc = crc16(&block);
        packet.push((crc >> 8) as u8);
        packet.push((crc & 0xFF) as u8);
        packet
    }
}

pub fn crc16(data: &[u8]) -> u16 {
    let mut crc = 0u16;
    for &byte in data {
        crc ^= (byte as u16) << 8;
        for _ in 0..8 {
            if (crc & 0x8000) != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}
