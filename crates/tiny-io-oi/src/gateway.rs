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
    pub decoder: io_oi_core::embedded::FramingDecoder<0xDE, 0xAD, 256>,
}

impl<N: Network, U: Uart> GatewayBridge<N, U> {
    pub fn new(network: N, uart: U) -> Self {
        Self {
            network,
            uart,
            decoder: io_oi_core::embedded::FramingDecoder::new(),
        }
    }

    /// Process UART input and ESP-NOW messages
    pub fn tick(&mut self) {
        // 1. Read UART bytes
        while let Some(b) = self.uart.read() {
            if let Some(frame_bytes) = self.decoder.feed_byte(b) {
                if let Ok(archived) = rkyv::check_archived_root::<GatewayFrame>(frame_bytes) {
                    // Forward over ESP-NOW
                    if !archived.payload.is_empty() {
                        let opcode = OpCode::from(archived.payload[0]);
                        let _ = self.network.broadcast(opcode, &archived.payload[1..]);
                    }
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
