#[cfg(not(feature = "std"))]
use alloc::vec::Vec;
#[cfg(feature = "std")]
use std::vec::Vec;

use crate::{Network, Motor, Gpio, Adc, IoOiState, OpCode, NodeId, to_core_id, from_core_id, VmError, MicroVm};
use io_oi_core::{NodeId as CoreNodeId, VmScript};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GossipAssertion {
    pub sender_mac: [u8; 6],
    pub timestamp_us: u64,
    pub hazard_score: u16,
}

pub struct GossipContext<'a, N: Network> {
    pub my_mac: [u8; 6],
    pub network: &'a mut N,
    pub recent_gossip: &'a [GossipAssertion],
    pub current_time_us: u64,
}

#[inline(always)]
pub fn get_time_us() -> u64 {
    #[cfg(feature = "ptp")]
    {
        crate::ptp::PTP_CLOCK.lock().get_time_us()
    }
    #[cfg(not(feature = "ptp"))]
    {
        0
    }
}

/// The Soldier node implementation
pub struct TinyNode<N: Network, M: Motor, S: IoOiState, G: Gpio + Adc, const CACHE_SIZE: usize> {
    pub id: NodeId,
    pub network: N,
    pub motor: M,
    pub state: S,
    pub gpio: G,
    pub scores: [Option<(CoreNodeId, u64)>; CACHE_SIZE],
    pub wal: Option<cdDB::StdWal>,
    pub safe_mode: bool,
    pub disqualified_leader: Option<NodeId>,
    pub last_leader_msg: Option<(OpCode, Vec<u8>)>,
    pub leader_active: bool,
    pub recent_gossip: heapless::Vec<GossipAssertion, 8>,
}

impl<N: Network, M: Motor, S: IoOiState, G: Gpio + Adc, const CACHE_SIZE: usize> TinyNode<N, M, S, G, CACHE_SIZE> {
    pub fn new(id: NodeId, network: N, motor: M, state: S, gpio: G) -> Self {
        Self {
            id,
            network,
            motor,
            state,
            gpio,
            scores: [const { None }; CACHE_SIZE],
            wal: None,
            safe_mode: false,
            disqualified_leader: None,
            last_leader_msg: None,
            leader_active: false,
            recent_gossip: heapless::Vec::new(),
        }
    }

    pub fn with_wal(mut self, wal: cdDB::StdWal) -> Self {
        self.wal = Some(wal);
        self
    }

    pub fn enter_safe_mode(&mut self, reason: &str) {
        self.safe_mode = true;
        self.motor.stop();
        
        if let Some(wal) = &self.wal {
            #[cfg(not(feature = "std"))]
            use alloc::string::ToString;
            #[cfg(feature = "std")]
            use std::string::ToString;

            let mut attributes = cdDB::Attributes::new();
            attributes.insert("conflict_reason".to_string(), reason.to_string());
            let cmd = cdDB::WriteCommand::Insert {
                entity_id: 999, // Special ID for Jury conflict log
                attributes,
                attributes_int: cdDB::Attributes::new(),
                attributes_blob: cdDB::Attributes::new(),
            };
            let _ = cdDB::WalProvider::append(wal, &cmd);
        }
    }

    pub fn recover_from_wal(&mut self) -> Result<(), &'static str> {
        if let Some(wal) = &self.wal {
            if let Ok(bytes) = cdDB::WalProvider::read_all(wal) {
                let mut pos = 0;
                while pos < bytes.len() {
                    if pos + 4 > bytes.len() {
                        break;
                    }
                    let mut len_buf = [0u8; 4];
                    len_buf.copy_from_slice(&bytes[pos..pos + 4]);
                    let len = u32::from_le_bytes(len_buf) as usize;
                    pos += 4;
                    if pos + len > bytes.len() {
                        break;
                    }
                    let cmd_bytes = &bytes[pos..pos + len];
                    pos += len;
                    if let Some(cmd) = cdDB::WriteCommand::decode(cmd_bytes) {
                        match cmd {
                            cdDB::WriteCommand::Insert { attributes_int, .. } => {
                                if let Some(&delta) = attributes_int.inner().get("delta") {
                                    self.state.apply_delta(delta as u8);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Process incoming messages and perform actions.
    pub fn tick(&mut self) {
        self.decay_scores();

        // Expire assertions from the cache (5-second validity window)
        let current_ptp_time = get_time_us();
        self.recent_gossip.retain(|assertion| {
            current_ptp_time.saturating_sub(assertion.timestamp_us) <= 5_000_000
        });

        // If we previously had an active leader, and now get_leader() is None,
        // that means the leader's heartbeat decayed to zero (went offline).
        if self.leader_active && self.get_leader().is_none() {
            self.leader_active = false;
            self.enter_safe_mode("heartbeat_decay");
        }

        let mut buffer = [0u8; 256]; // ESP-NOW payload fits inside 256 bytes
        if let Some((sender_id, opcode, payload)) = self.network.receive(&mut buffer) {
            // Ignore anything from the disqualified leader
            if let Some(dq) = self.disqualified_leader {
                if dq == sender_id {
                    return;
                }
            }

            let core_sender_id = to_core_id(sender_id);

            // Double sign conflict detection
            let is_leader = self.get_leader() == Some(sender_id);
            if is_leader {
                match opcode {
                    OpCode::StateUpdate | OpCode::VmScriptDispatch | OpCode::TaskDispatch => {
                        if let Some((prev_op, prev_payload)) = &self.last_leader_msg {
                            if *prev_op == opcode && prev_payload != payload {
                                // Double sign conflict detected!
                                self.disqualified_leader = Some(sender_id);
                                self.enter_safe_mode("double_sign");
                                return;
                            }
                        }
                        self.last_leader_msg = Some((opcode, payload.to_vec()));
                    }
                    _ => {}
                }
            }

            match opcode {
                OpCode::Heartbeat => {
                    self.update_score(core_sender_id, 100);
                    // Healing out of safe mode when a new leader is recognized
                    if self.safe_mode {
                        if let Some(leader_id) = self.get_leader() {
                            if self.disqualified_leader != Some(leader_id) {
                                self.safe_mode = false;
                            }
                        }
                    }
                }
                OpCode::SpatialGossip => {
                    if payload.len() >= 16 {
                        let mut mac = [0u8; 6];
                        mac.copy_from_slice(&payload[0..6]);
                        
                        let timestamp_us = u64::from_le_bytes([
                            payload[6], payload[7], payload[8], payload[9],
                            payload[10], payload[11], payload[12], payload[13]
                        ]);
                        
                        let hazard_score = u16::from_le_bytes([payload[14], payload[15]]);
                        
                        // Push to recent_gossip if it doesn't already exist or updates an existing entry
                        let mut exists = false;
                        for entry in self.recent_gossip.iter_mut() {
                            if entry.sender_mac == mac {
                                entry.timestamp_us = timestamp_us;
                                entry.hazard_score = hazard_score;
                                exists = true;
                                break;
                            }
                        }
                        if !exists {
                            let assertion = GossipAssertion {
                                sender_mac: mac,
                                timestamp_us,
                                hazard_score,
                            };
                            let _ = self.recent_gossip.push(assertion);
                        }
                    }
                }
                OpCode::TaskDispatch => {
                    if !self.safe_mode {
                        self.motor.set_speed(255);
                    }
                }
                OpCode::StateUpdate => {
                    if !self.safe_mode {
                        if let Some(&delta) = payload.first() {
                            if let Some(wal) = &self.wal {
                                #[cfg(not(feature = "std"))]
                                use alloc::string::ToString;
                                #[cfg(feature = "std")]
                                use std::string::ToString;

                                let mut attributes_int = cdDB::Attributes::new();
                                attributes_int.insert("delta".to_string(), delta as u32);
                                let cmd = cdDB::WriteCommand::Insert {
                                    entity_id: 1,
                                    attributes: cdDB::Attributes::new(),
                                    attributes_int,
                                    attributes_blob: cdDB::Attributes::new(),
                                };
                                let _ = cdDB::WalProvider::append(wal, &cmd);
                            }
                            self.state.apply_delta(delta);
                        }
                    }
                }
                OpCode::VmScriptDispatch => {
                    if !self.safe_mode {
                        // Phase 2: 安全解析 & 帶燃料限制 of VM 執行
                        if let Ok(archived_script) = rkyv::check_archived_root::<VmScript>(payload) {
                            let mut vm = MicroVm::new(100); // 100 fuel budget
                            if let Err(e) = vm.run(archived_script, &mut self.motor, &self.gpio) {
                                // Phase 3: Trap & Forward-Back (反向異常回報)
                                self.motor.stop();
                                let mut err_buf = [0u8; 5];
                                err_buf[0] = 0xFF; // Exception identifier
                                match e {
                                    VmError::OutOfFuel => {
                                        err_buf[1] = 0x01; // Out of Fuel code
                                    }
                                    VmError::AssertionFailed { pin, expected, actual } => {
                                        err_buf[1] = 0x02; // Assertion Failed code
                                        err_buf[2] = pin;
                                        err_buf[3] = expected;
                                        err_buf[4] = actual;
                                    }
                                    _ => {
                                        err_buf[1] = 0x03; // Standard Library Vibration/Acoustic exception code
                                    }
                                }
                                let _ = self.network.broadcast(OpCode::Exception, &err_buf);
                            }
                        } else {
                            // FALLBACK: Execute as standard library 8-byte bytecode
                            let mut vm = MicroVm::new(100);
                            let gossip_ctx = GossipContext {
                                my_mac: self.id,
                                network: &mut self.network,
                                recent_gossip: &self.recent_gossip,
                                current_time_us: get_time_us(),
                            };
                            if let Err(e) = vm.run_std(payload, &mut self.motor, &self.gpio, Some(gossip_ctx)) {
                                self.motor.stop();
                                let mut err_buf = [0u8; 5];
                                err_buf[0] = 0xFF; // Exception identifier
                                match e {
                                    VmError::OutOfFuel => {
                                        err_buf[1] = 0x01;
                                    }
                                    VmError::UnauthorizedAccess => {
                                        err_buf[1] = 0x05;
                                    }
                                    VmError::VibrationHazard { .. } => {
                                        err_buf[1] = 0x02;
                                    }
                                    VmError::MultiBandSpectrumHazard => {
                                        err_buf[1] = 0x03;
                                    }
                                    VmError::AcousticFailureDetected => {
                                        err_buf[1] = 0x04;
                                    }
                                    _ => {
                                        err_buf[1] = 0x99;
                                    }
                                }
                                let _ = self.network.broadcast(OpCode::Exception, &err_buf);
                            }
                        }
                    }
                }
                OpCode::StdBytecodeDispatch => {
                    if !self.safe_mode {
                        let mut vm = MicroVm::new(100);
                        let gossip_ctx = GossipContext {
                            my_mac: self.id,
                            network: &mut self.network,
                            recent_gossip: &self.recent_gossip,
                            current_time_us: get_time_us(),
                        };
                        if let Err(e) = vm.run_std(payload, &mut self.motor, &self.gpio, Some(gossip_ctx)) {
                            self.motor.stop();
                            let mut err_buf = [0u8; 5];
                            err_buf[0] = 0xFF; // Exception identifier
                            match e {
                                VmError::OutOfFuel => {
                                    err_buf[1] = 0x01;
                                }
                                VmError::UnauthorizedAccess => {
                                    err_buf[1] = 0x05;
                                }
                                VmError::VibrationHazard { .. } => {
                                    err_buf[1] = 0x02;
                                }
                                VmError::MultiBandSpectrumHazard => {
                                    err_buf[1] = 0x03;
                                }
                                VmError::AcousticFailureDetected => {
                                    err_buf[1] = 0x04;
                                }
                                _ => {
                                    err_buf[1] = 0x99;
                                }
                            }
                            let _ = self.network.broadcast(OpCode::Exception, &err_buf);
                        }
                    }
                }
                _ => {}
            }
        }

        // When a leader is successfully retrieved, mark leader_active as true
        if self.get_leader().is_some() {
            self.leader_active = true;
        }
    }

    fn decay_scores(&mut self) {
        for entry in self.scores.iter_mut() {
            if let Some((_, score)) = entry {
                *score = score.saturating_sub(1);
            }
        }
        for entry in self.scores.iter_mut() {
            if let Some((_, 0)) = entry {
                *entry = None;
            }
        }
    }

    pub fn check_and_heal(&mut self) {
        if self.get_leader().is_none() {
            let _ = self.network.broadcast(OpCode::Exception, &[0xFE]);
        }
    }

    pub fn update_score(&mut self, node_id: CoreNodeId, score: u64) {
        // Look for existing entry
        for entry in self.scores.iter_mut() {
            if let Some((existing_id, existing_score)) = entry {
                if *existing_id == node_id {
                    *existing_score = score;
                    return;
                }
            }
        }
        // Find empty slot
        for entry in self.scores.iter_mut() {
            if entry.is_none() {
                *entry = Some((node_id, score));
                return;
            }
        }
    }

    pub fn get_leader(&self) -> Option<NodeId> {
        self.scores.iter()
            .flatten()
            .max_by_key(|(_, score)| *score)
            .map(|(id, _)| from_core_id(*id))
    }
}
