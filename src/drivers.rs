#[cfg(not(feature = "std"))]
use alloc::{vec::Vec, collections::VecDeque};
#[cfg(feature = "std")]
use std::{vec::Vec, collections::VecDeque};

use crate::{Network, Motor, OpCode, NodeId};

/// A mock network that stores messages in a queue.
pub struct MockNetwork {
    pub incoming: VecDeque<(NodeId, OpCode, Vec<u8>)>,
    pub sent: Vec<(OpCode, Vec<u8>)>,
}

impl MockNetwork {
    pub fn new() -> Self {
        Self {
            incoming: VecDeque::new(),
            sent: Vec::new(),
        }
    }

    pub fn simulate_receive(&mut self, sender: NodeId, opcode: OpCode, payload: &[u8]) {
        self.incoming.push_back((sender, opcode, payload.to_vec()));
    }
}

impl Network for MockNetwork {
    fn broadcast(&mut self, opcode: OpCode, payload: &[u8]) -> Result<(), &'static str> {
        self.sent.push((opcode, payload.to_vec()));
        Ok(())
    }

    fn receive<'a>(&mut self, buffer: &'a mut [u8]) -> Option<(NodeId, OpCode, &'a [u8])> {
        if let Some((id, opcode, payload)) = self.incoming.pop_front() {
            let len = payload.len().min(buffer.len());
            buffer[..len].copy_from_slice(&payload[..len]);
            Some((id, opcode, &buffer[..len]))
        } else {
            None
        }
    }
}

/// A mock motor that records speed changes.
pub struct MockMotor {
    pub current_speed: u8,
    pub history: Vec<u8>,
}

impl MockMotor {
    pub fn new() -> Self {
        Self {
            current_speed: 0,
            history: Vec::new(),
        }
    }
}

impl Motor for MockMotor {
    fn set_speed(&mut self, speed: u8) {
        self.current_speed = speed;
        self.history.push(speed);
        #[cfg(feature = "std")]
        println!("[MockMotor] Speed set to {}", speed);
    }

    fn stop(&mut self) {
        self.current_speed = 0;
        self.history.push(0);
        #[cfg(feature = "std")]
        println!("[MockMotor] Stopped");
    }
}

/// A simple mock state for testing bitmask sessions.
#[derive(Default, Clone, Debug)]
pub struct MockState {
    pub flags: u8,
}

impl crate::IoOiState for MockState {
    fn to_bytes(&self) -> &[u8] {
        core::slice::from_ref(&self.flags)
    }

    fn apply_delta(&mut self, delta_mask: u8) {
        self.flags ^= delta_mask; // XOR for simple toggle
        #[cfg(feature = "std")]
        println!("[MockState] Applied delta: {:#010b}, new state: {:#010b}", delta_mask, self.flags);
    }
}

/// A simple mock GPIO implementation for testing.
#[derive(Default, Clone, Debug)]
pub struct MockGpio {
    pub pins: [u8; 32],
}

impl MockGpio {
    pub fn new() -> Self {
        Self { pins: [0; 32] }
    }

    pub fn set_pin(&mut self, pin: u8, val: u8) {
        if (pin as usize) < self.pins.len() {
            self.pins[pin as usize] = val;
        }
    }
}

impl crate::Gpio for MockGpio {
    fn read_pin(&self, pin: u8) -> u8 {
        if (pin as usize) < self.pins.len() {
            self.pins[pin as usize]
        } else {
            0
        }
    }
}

impl crate::Adc for MockGpio {
    fn read_adc_buffer(&self, pin: u8, buffer: &mut [i16]) {
        let freq = match pin {
            3 => 125, // Mock nozzle scraping frequency (AssertVibration target)
            5 => 45,  // Mock structure resonance frequency (AvoidResonance target)
            _ => 10,
        };
        let amp = if (pin as usize) < self.pins.len() {
            self.pins[pin as usize] as f32
        } else {
            1.0f32
        };
        for i in 0..buffer.len() {
            let t = i as f32 / 1000.0f32;
            buffer[i] = (amp * 1.5f32 * libm::sinf(2.0f32 * core::f32::consts::PI * freq as f32 * t)) as i16;
        }
    }
}

/// A mock UART implementation for testing GatewayBridge.
pub struct MockUart {
    pub incoming: VecDeque<u8>,
    pub written: Vec<u8>,
}

impl MockUart {
    pub fn new() -> Self {
        Self {
            incoming: VecDeque::new(),
            written: Vec::new(),
        }
    }

    pub fn simulate_read(&mut self, data: &[u8]) {
        self.incoming.extend(data.iter().copied());
    }
}

impl crate::Uart for MockUart {
    fn read(&mut self) -> Option<u8> {
        self.incoming.pop_front()
    }

    fn write(&mut self, data: &[u8]) -> Result<(), &'static str> {
        self.written.extend_from_slice(data);
        Ok(())
    }
}
