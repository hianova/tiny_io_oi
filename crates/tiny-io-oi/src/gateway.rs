#[cfg(not(feature = "std"))]
use alloc::vec::Vec;
#[cfg(feature = "std")]
use std::vec::Vec;

use crate::{Network, Uart, OpCode};
use io_oi_core::GatewayFrame;

/// A physical bridge between USB Serial and ESP-NOW.
pub struct GatewayBridge<N: Network, U: Uart> {
    pub network: N,
    pub uart: U,
    pub rx_buf: Vec<u8>,
}

impl<N: Network, U: Uart> GatewayBridge<N, U> {
    pub fn new(network: N, uart: U) -> Self {
        Self {
            network,
            uart,
            rx_buf: Vec::new(),
        }
    }

    /// Process UART input and ESP-NOW messages
    pub fn tick(&mut self) {
        // 1. Read UART bytes
        while let Some(b) = self.uart.read() {
            self.rx_buf.push(b);

            // Framing parser: look for [0xDE, 0xAD] magic prefix + 2 bytes len
            if self.rx_buf.len() >= 4 {
                if self.rx_buf[0] == 0xDE && self.rx_buf[1] == 0xAD {
                    let len = u16::from_be_bytes([self.rx_buf[2], self.rx_buf[3]]) as usize;
                    if self.rx_buf.len() >= 4 + len {
                        let frame_bytes = &self.rx_buf[4..4 + len];
                        if let Ok(archived) = rkyv::check_archived_root::<GatewayFrame>(frame_bytes) {
                            // Forward over ESP-NOW
                            if !archived.payload.is_empty() {
                                let opcode = OpCode::from(archived.payload[0]);
                                let _ = self.network.broadcast(opcode, &archived.payload[1..]);
                            }
                        }
                        self.rx_buf.drain(..4 + len);
                    }
                } else {
                    // Drop first byte and look again
                    self.rx_buf.remove(0);
                }
            }
        }

        // 2. Read ESP-NOW messages and forward to UART
        let mut esp_buf = [0u8; 256];
        if let Some((sender, opcode, payload)) = self.network.receive(&mut esp_buf) {
            let mut packed = Vec::new();
            packed.push(opcode as u8);
            packed.extend_from_slice(payload);

            let frame = GatewayFrame {
                mac_addr: sender,
                payload: packed,
            };

            if let Ok(serialized) = rkyv::to_bytes::<_, 256>(&frame) {
                let len = serialized.len() as u16;
                let mut header = [0u8; 4];
                header[0] = 0xDE;
                header[1] = 0xAD;
                header[2..4].copy_from_slice(&len.to_be_bytes());

                let _ = self.uart.write(&header);
                let _ = self.uart.write(&serialized);
            }
        }
    }
}
