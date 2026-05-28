#![cfg_attr(not(feature = "std"), no_std)]

pub mod drivers;
pub mod memory;
pub mod hardware;
pub mod vm;
pub mod node;
pub mod gateway;
pub mod std_impl;
pub mod unsafe_core;
#[cfg(feature = "ptp")]
pub mod ptp;

#[cfg(feature = "verifier")]
pub mod verifier;

pub use tiny_io_oi_macros::io_oi_node;

extern crate alloc;

/// NodeId is a 6-byte MAC address, commonly used in ESP-NOW.
pub type NodeId = [u8; 6];

use io_oi_core::NodeId as CoreNodeId;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpCode {
    Heartbeat = 0x04,
    SpatialGossip = 0x05,
    TaskDispatch = 0x20,
    TaskAchieved = 0x21,
    TaskFailed = 0x22,
    StateUpdate = 0x30, // New opcode for bitmask session
    VmScriptDispatch = 0x40, // New: for distributing zero-copy VM bytecode
    StdBytecodeDispatch = 0x41, // New: for distributing standard library bytecode
    Exception = 0xFF,        // New: for Exception / Trap reporting
}

impl From<u8> for OpCode {
    fn from(value: u8) -> Self {
        match value {
            0x04 => OpCode::Heartbeat,
            0x05 => OpCode::SpatialGossip,
            0x20 => OpCode::TaskDispatch,
            0x21 => OpCode::TaskAchieved,
            0x22 => OpCode::TaskFailed,
            0x30 => OpCode::StateUpdate,
            0x40 => OpCode::VmScriptDispatch,
            0x41 => OpCode::StdBytecodeDispatch,
            0xFF => OpCode::Exception,
            _ => OpCode::Heartbeat,
        }
    }
}

/// Helper to convert between tiny NodeId and core NodeId.
pub fn to_core_id(id: NodeId) -> CoreNodeId {
    let mut core_id = [0u8; 32];
    core_id[0..6].copy_from_slice(&id);
    core_id
}

/// Helper to convert back to tiny NodeId.
pub fn from_core_id(id: CoreNodeId) -> NodeId {
    let mut tiny_id = [0u8; 6];
    tiny_id.copy_from_slice(&id[0..6]);
    tiny_id
}

/// Hardware abstraction for ESP-NOW or similar network layers.
pub trait Network {
    fn broadcast(&mut self, opcode: OpCode, payload: &[u8]) -> Result<(), &'static str>;
    /// Receives a message and writes the payload into the provided buffer.
    /// Returns the sender ID, opcode, and the slice of the buffer containing the payload.
    fn receive<'a>(&mut self, buffer: &'a mut [u8]) -> Option<(NodeId, OpCode, &'a [u8])>;
}

/// Hardware abstraction for UART (USB Serial) communication.
pub trait Uart {
    fn read(&mut self) -> Option<u8>;
    fn write(&mut self, data: &[u8]) -> Result<(), &'static str>;
}

/// Hardware abstraction for Motor control (PWM/GPIO).
pub trait Motor {
    fn set_speed(&mut self, speed: u8);
    fn stop(&mut self);
    fn fade_to(&mut self, channel: u8, target_duty: u8, fade_ms: u16) {
        let _ = (channel, fade_ms);
        self.set_speed(target_duty);
    }
}

/// Hardware abstraction for GPIO inputs.
pub trait Gpio {
    fn read_pin(&self, pin: u8) -> u8;
}

pub trait Adc {
    fn read_adc_buffer(&self, pin: u8, buffer: &mut [i16]);
}

pub use io_oi_core::{VmStep, VmScript, ArchivedVmScript, ArchivedVmStep, GatewayFrame, ArchivedGatewayFrame};

pub use hardware::{IoOiState, PhysicalStatus, Waveform, WaveformCmd, WaveformMatrix, DigitalOutput, PwmOutput, HardwareRouter};
pub use vm::{VmError, MicroVm};
pub use node::TinyNode;
pub use gateway::GatewayBridge;

pub mod sync {
    #[cfg(all(feature = "std", not(feature = "tiny-node")))]
    pub use std::sync::Arc;

    #[cfg(feature = "tiny-node")]
    pub use crate::memory::TinyArc as Arc;
}

/// 全域的靜態 Arena，消滅記憶體碎片化
#[cfg(all(feature = "tiny-node", not(feature = "loom")))]
pub static GLOBAL_ARENA: memory::Arena<[u8; 32], 256> = memory::Arena::new();

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drivers::{MockNetwork, MockMotor, MockState, MockGpio, MockUart};
    use hardware::ArchivedPhysicalStatus;

    #[cfg(not(feature = "std"))]
    use alloc::vec;

    #[test]
    fn test_arena_scorecard_leader() {
        let mut node = TinyNode::<_, _, MockState, MockGpio, 2>::new(
            [0; 6],
            MockNetwork::new(),
            MockMotor::new(),
            MockState::default(),
            MockGpio::new(),
        );
        node.update_score(to_core_id([1; 6]), 50);
        node.update_score(to_core_id([2; 6]), 100);
        assert_eq!(node.get_leader(), Some([2; 6]));
    }

    #[test]
    fn test_bitmask_state_update() {
        let network = MockNetwork::new();
        let mut node = TinyNode::<_, _, MockState, MockGpio, 2>::new(
            [0; 6],
            network,
            MockMotor::new(),
            MockState::default(),
            MockGpio::new(),
        );
        
        let payload = [0b0000_0001];
        node.network.simulate_receive([1; 6], OpCode::StateUpdate, &payload);
        node.tick();
        
        assert_eq!(node.state.flags, 0b0000_0001);
    }

    #[test]
    fn test_zero_copy_vm_execution() {
        let script = VmScript {
            steps: vec![
                VmStep::SetPwm { channel: 0, speed: 120 },
                VmStep::AssertOrYield { pin: 5, expected: 1 },
            ],
        };
        let serialized = rkyv::to_bytes::<_, 256>(&script).unwrap();
        
        let mut motor = MockMotor::new();
        let mut gpio = MockGpio::new();
        gpio.set_pin(5, 1);

        let mut vm = MicroVm::new(10);
        
        assert!(vm.run(&serialized, &mut motor, &gpio).is_ok());
        assert_eq!(motor.current_speed, 120);
        assert_eq!(vm.fuel, 8);
    }

    #[test]
    fn test_vm_out_of_fuel() {
        let script = VmScript {
            steps: vec![
                VmStep::SetPwm { channel: 0, speed: 120 },
                VmStep::SetPwm { channel: 0, speed: 130 },
            ],
        };
        let serialized = rkyv::to_bytes::<_, 256>(&script).unwrap();
        let mut motor = MockMotor::new();
        let gpio = MockGpio::new();
        let mut vm = MicroVm::new(1);
        
        let res = vm.run(&serialized, &mut motor, &gpio);
        assert_eq!(res, Err(VmError::OutOfFuel));
    }

    #[test]
    fn test_vm_assertion_failed_and_trap() {
        let script = VmScript {
            steps: vec![
                VmStep::SetPwm { channel: 0, speed: 200 },
                VmStep::AssertOrYield { pin: 2, expected: 1 },
            ],
        };
        let serialized = rkyv::to_bytes::<_, 256>(&script).unwrap();
        
        let network = MockNetwork::new();
        let motor = MockMotor::new();
        let mut gpio = MockGpio::new();
        gpio.set_pin(2, 0);
        
        let mut node = TinyNode::<_, _, MockState, MockGpio, 2>::new(
            [0; 6],
            network,
            motor,
            MockState::default(),
            gpio,
        );

        node.network.simulate_receive([1; 6], OpCode::VmScriptDispatch, &serialized);
        node.tick();

        assert_eq!(node.motor.current_speed, 0);
        let sent = &node.network.sent;
        assert_eq!(sent.len(), 1);
        let (op, payload) = &sent[0];
        assert_eq!(*op, OpCode::Exception);
        assert_eq!(payload, &vec![0xFF, 0x02, 2, 1, 0]);
    }

    #[test]
    fn test_gateway_bridge_flow() {
        let network = MockNetwork::new();
        let uart = MockUart::new();
        let mut bridge = GatewayBridge::new(network, uart);

        // 1. Simulate UART -> ESP-NOW
        let mac = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66];
        let payload = vec![OpCode::TaskDispatch as u8, 0x01, 0x02];
        let frame = GatewayFrame {
            mac_addr: mac,
            payload: payload.clone(),
        };
        let serialized = rkyv::to_bytes::<_, 256>(&frame).unwrap();
        let len = serialized.len() as u16;
        let mut data = vec![0xDE, 0xAD];
        data.extend_from_slice(&len.to_be_bytes());
        data.extend_from_slice(&serialized);

        bridge.uart.simulate_read(&data);
        bridge.tick();

        // Should broadcast task dispatch over ESP-NOW
        assert_eq!(bridge.network.sent.len(), 1);
        let (op, p) = &bridge.network.sent[0];
        assert_eq!(*op, OpCode::TaskDispatch);
        assert_eq!(p, &vec![0x01, 0x02]);

        // 2. Simulate ESP-NOW -> UART
        let sender = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];
        bridge.network.simulate_receive(sender, OpCode::Exception, &[0xFF, 0x02]);
        bridge.tick();

        // UART should receive the framed serialized GatewayFrame
        assert!(bridge.uart.written.len() >= 4);
        assert_eq!(bridge.uart.written[0], 0xDE);
        assert_eq!(bridge.uart.written[1], 0xAD);
        let uart_len = u16::from_be_bytes([bridge.uart.written[2], bridge.uart.written[3]]) as usize;
        assert_eq!(bridge.uart.written.len(), 4 + uart_len);

        let frame_bytes = &bridge.uart.written[4..];
        let archived = rkyv::check_archived_root::<GatewayFrame>(frame_bytes).unwrap();
        assert_eq!(archived.mac_addr, sender);
        assert_eq!(archived.payload, vec![OpCode::Exception as u8, 0xFF, 0x02]);
    }

    #[io_oi_node]
    struct MacroSoldier {
        #[bind(channel = 0, strategy = "PWM")]
        pub motor: MockMotor,

        #[bind(channel = 5, strategy = "GPIO")]
        pub sensor: MockGpio,
    }

    #[test]
    fn test_macro_compile_and_routing() {
        let script = VmScript {
            steps: vec![
                VmStep::SetPwm { channel: 0, speed: 180 },
                VmStep::AssertOrYield { pin: 5, expected: 1 },
            ],
        };
        let serialized = rkyv::to_bytes::<_, 256>(&script).unwrap();
        let mut soldier = MacroSoldier {
            motor: MockMotor::new(),
            sensor: MockGpio::new(),
        };
        soldier.sensor.set_pin(5, 1);

        let mut fuel = 20;
        let res = soldier.run_vm_script(&serialized, &mut fuel);
        assert!(res.is_ok());
        assert_eq!(soldier.motor.current_speed, 180);
        assert_eq!(fuel, 18);
    }

    struct MockPin {
        pub high: bool,
    }
    impl DigitalOutput for MockPin {
        fn set_level(&mut self, high: bool) {
            self.high = high;
        }
    }

    struct MockPwm {
        pub duty: u8,
    }
    impl PwmOutput for MockPwm {
        fn set_duty(&mut self, duty: u8) {
            self.duty = duty;
        }
    }

    #[test]
    fn test_hardware_router() {
        let mut router = HardwareRouter::<MockPin, MockPwm, 2>::new();
        let pin = MockPin { high: false };
        let pwm = MockPwm { duty: 0 };
        
        router.bind_digital(3, pin).unwrap();
        router.bind_pwm(5, pwm).unwrap();
        
        let matrix = WaveformMatrix {
            commands: vec![
                WaveformCmd { channel: 3, waveform: Waveform::DigitalOut { state: 1 } },
                WaveformCmd { channel: 5, waveform: Waveform::Pwm8Bit { duty_cycle: 128 } },
            ],
        };
        
        let serialized = rkyv::to_bytes::<_, 256>(&matrix).unwrap();
        router.apply_waveforms(&serialized).unwrap();
        
        assert!(router.digital_pins[0].as_ref().unwrap().1.high);
        assert_eq!(router.pwm_channels[0].as_ref().unwrap().1.duty, 128);
    }

    #[test]
    fn test_client_side_failover() {
        let mut node = TinyNode::<_, _, MockState, MockGpio, 2>::new(
            [0; 6],
            MockNetwork::new(),
            MockMotor::new(),
            MockState::default(),
            MockGpio::new(),
        );
        
        let leader = [1; 6];
        node.network.simulate_receive(leader, OpCode::Heartbeat, &[]);
        node.tick();
        assert_eq!(node.get_leader(), Some(leader));
        
        // 模擬 100 次 tick，期間沒有收到 Heartbeat
        for _ in 0..100 {
            node.tick();
        }
        
        // Leader 應該由於 Heartbeat 衰減到 0 而被移除
        assert_eq!(node.get_leader(), None);
        
        // 進行自癒
        node.check_and_heal();
        
        // 網路應該廣播了自癒 Exception 訊息 0xFE
        let sent = &node.network.sent;
        assert_eq!(sent.len(), 1);
        let (op, payload) = &sent[0];
        assert_eq!(*op, OpCode::Exception);
        assert_eq!(payload, &vec![0xFE]);
    }

    #[test]
    fn test_physical_status() {
        let status = PhysicalStatus::Pending;
        let serialized = rkyv::to_bytes::<_, 32>(&status).unwrap();
        let archived = rkyv::check_archived_root::<PhysicalStatus>(&serialized).unwrap();
        assert_eq!(*archived, ArchivedPhysicalStatus::Pending);
    }

    #[test]
    fn test_state_recovery_from_wal() {
        use crate::memory::FlashFileSystem;
        use alloc::sync::Arc;
        use alloc::string::ToString;

        let fs = Arc::new(FlashFileSystem::new());
        let wal = cdDB::StdWal::new("test_wal.log".to_string(), fs.clone(), Default::default());

        let mut node = TinyNode::<_, _, MockState, MockGpio, 2>::new(
            [0; 6],
            MockNetwork::new(),
            MockMotor::new(),
            MockState::default(),
            MockGpio::new(),
        ).with_wal(wal);

        // 模擬收到 StateUpdate 封包
        let payload = [0b0000_0101];
        node.network.simulate_receive([1; 6], OpCode::StateUpdate, &payload);
        node.tick();

        // 確保狀態已被更新
        assert_eq!(node.state.flags, 5);

        // 模擬斷電重啟：建立一個新的節點，並共享同一個虛擬 Flash 檔案系統
        let wal_restart = cdDB::StdWal::new("test_wal.log".to_string(), fs, Default::default());
        let mut node_restart = TinyNode::<_, _, MockState, MockGpio, 2>::new(
            [0; 6],
            MockNetwork::new(),
            MockMotor::new(),
            MockState::default(), // 初始狀態為 0
            MockGpio::new(),
        ).with_wal(wal_restart);

        // 確保重啟後初始狀態為 0
        assert_eq!(node_restart.state.flags, 0);

        // 執行 WAL 恢復
        node_restart.recover_from_wal().unwrap();

        // 驗證狀態已成功從 WAL 恢復
        assert_eq!(node_restart.state.flags, 5);
    }

    #[test]
    #[cfg(not(feature = "loom"))]
    fn test_no_std_memory_leak_and_thread_drop() {
        use crate::memory::Arena;
        use std::thread;
        use std::sync::Barrier;

        // 1. Create a small Arena for testing reuse and limits inside Arc
        let arena_shared = std::sync::Arc::new(Arena::<[u8; 4], 4>::new());

        // 2. Allocate up to capacity
        let arc0 = arena_shared.alloc([0; 4]).expect("alloc 0");
        let arc1 = arena_shared.alloc([1; 4]).expect("alloc 1");
        let arc2 = arena_shared.alloc([2; 4]).expect("alloc 2");
        let arc3 = arena_shared.alloc([3; 4]).expect("alloc 3");

        // Arena is full now
        assert!(arena_shared.alloc([4; 4]).is_none());

        // 3. Drop one and verify slot is reused (No Memory Leak)
        drop(arc2);
        let arc_reused = arena_shared.alloc([5; 4]).expect("should reuse slot 2");
        assert_eq!(*arena_shared.get(&arc_reused).unwrap(), [5; 4]);

        // 4. Thread-safe concurrent access and drop test
        let barrier = std::sync::Arc::new(Barrier::new(4));
        let mut handles = vec![];

        // We have arc0, arc1, arc3, arc_reused outstanding.
        // Let's drop them to clear the arena.
        drop(arc0);
        drop(arc1);
        drop(arc3);
        drop(arc_reused);

        for i in 0..4 {
            let arena_clone = arena_shared.clone();
            let barrier_clone = barrier.clone();
            handles.push(thread::spawn(move || {
                barrier_clone.wait();
                // Alloc and clone in thread
                let val = [i as u8; 4];
                let arc = arena_clone.alloc(val).expect("thread alloc");
                let arc_clone = arc.clone();
                assert_eq!(*arena_clone.get(&arc_clone).unwrap(), val);
                drop(arc);
                // Hold arc_clone for a bit
                thread::yield_now();
                drop(arc_clone);
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        // After all threads exit, all slots must be fully released (ref_count == 0)
        // If we can allocate 4 new slots, then no leaks occurred!
        let a0 = arena_shared.alloc([10; 4]).expect("clean alloc 0");
        let a1 = arena_shared.alloc([11; 4]).expect("clean alloc 1");
        let a2 = arena_shared.alloc([12; 4]).expect("clean alloc 2");
        let a3 = arena_shared.alloc([13; 4]).expect("clean alloc 3");

        assert_eq!(*arena_shared.get(&a0).unwrap(), [10; 4]);
        assert_eq!(*arena_shared.get(&a1).unwrap(), [11; 4]);
        assert_eq!(*arena_shared.get(&a2).unwrap(), [12; 4]);
        assert_eq!(*arena_shared.get(&a3).unwrap(), [13; 4]);
    }

    #[io_oi_node]
    struct MacroDualMotor {
        #[bind(channel = 0, strategy = "PWM")]
        pub left: MockMotor,

        #[bind(channel = 1, strategy = "PWM")]
        pub right: MockMotor,
    }

    #[test]
    fn test_multi_pwm_routing() {
        let script = VmScript {
            steps: vec![
                VmStep::SetPwm { channel: 0, speed: 100 },
                VmStep::SetPwm { channel: 1, speed: 200 },
            ],
        };
        let serialized = rkyv::to_bytes::<_, 256>(&script).unwrap();
        let mut dual = MacroDualMotor {
            left: MockMotor::new(),
            right: MockMotor::new(),
        };

        let mut fuel = 10;
        assert!(dual.run_vm_script(&serialized, &mut fuel).is_ok());
        assert_eq!(dual.left.current_speed, 100);
        assert_eq!(dual.right.current_speed, 200);
    }

    #[io_oi_node]
    struct MacroSafeShutdown {
        #[bind(channel = 0, strategy = "PWM")]
        pub left: MockMotor,

        #[bind(channel = 1, strategy = "PWM")]
        pub right: MockMotor,

        #[bind(channel = 5, strategy = "GPIO")]
        pub sensor: MockGpio,
    }

    #[test]
    fn test_assert_or_yield_safe_shutdown() {
        let script = VmScript {
            steps: vec![
                VmStep::SetPwm { channel: 0, speed: 100 },
                VmStep::SetPwm { channel: 1, speed: 200 },
                VmStep::AssertOrYield { pin: 5, expected: 1 },
            ],
        };
        let serialized = rkyv::to_bytes::<_, 256>(&script).unwrap();
        let mut robot = MacroSafeShutdown {
            left: MockMotor::new(),
            right: MockMotor::new(),
            sensor: MockGpio::new(),
        };
        robot.sensor.set_pin(5, 0);

        let mut fuel = 10;
        let res = robot.run_vm_script(&serialized, &mut fuel);
        assert!(res.is_err());
        assert_eq!(robot.left.current_speed, 0);
        assert_eq!(robot.right.current_speed, 0);
    }

    #[test]
    fn test_failover_safe_mode_and_double_sign() {
        let network = MockNetwork::new();
        let mut node = TinyNode::<_, _, MockState, MockGpio, 2>::new(
            [0; 6],
            network,
            MockMotor::new(),
            MockState::default(),
            MockGpio::new(),
        );

        let leader_a = [1; 6];
        let leader_b = [2; 6];

        // Ensure we don't start in safe mode on boot
        node.tick();
        assert!(!node.safe_mode);

        // Receive heartbeat from leader_a
        node.network.simulate_receive(leader_a, OpCode::Heartbeat, &[]);
        node.tick();
        assert!(!node.safe_mode);
        assert_eq!(node.get_leader(), Some(leader_a));

        // Simulate heartbeat decay to zero
        for _ in 0..100 {
            node.tick();
        }
        assert_eq!(node.get_leader(), None);
        assert!(node.safe_mode); // Heartbeat decayed to zero, enters safe mode!

        // Heal when leader_b broadcasts heartbeat
        node.network.simulate_receive(leader_b, OpCode::Heartbeat, &[]);
        node.tick();
        assert!(!node.safe_mode);
        assert_eq!(node.get_leader(), Some(leader_b));

        // Now test double sign from leader_b (since leader_b is the current leader)
        node.network.simulate_receive(leader_b, OpCode::StateUpdate, &[10]);
        node.tick();
        assert!(!node.safe_mode);

        node.network.simulate_receive(leader_b, OpCode::StateUpdate, &[20]);
        node.tick();
        assert!(node.safe_mode);
        assert_eq!(node.disqualified_leader, Some(leader_b));

        node.network.simulate_receive(leader_b, OpCode::TaskDispatch, &[]);
        node.tick();
        assert_eq!(node.motor.current_speed, 0);

        // Heal when a new leader_c broadcasts
        let leader_c = [3; 6];
        node.network.simulate_receive(leader_c, OpCode::Heartbeat, &[]);
        node.tick();
        
        for _ in 0..5 {
            node.tick();
        }
        
        assert_eq!(node.get_leader(), Some(leader_c));
        assert!(!node.safe_mode);
    }

    #[cfg(feature = "std")]
    #[test]
    fn test_std_library_execution() {
        use crate::drivers::{MockMotor, MockGpio};
        use crate::vm::MicroVm;

        let mut motor = MockMotor::new();
        let mut gpio = MockGpio::new();

        // Opcode 0x80, Pin 3, param_a (Q15 threshold 1.0 = 32767), param_b (max_hz = 100)
        let bytecode = [
            0x80, 3, 0xFF, 0x7F, 100, 0, 0, 0, // 8-bytes
        ];

        let mut vm = MicroVm::new(100);
        // Set mock pin value so ADC amplitude is low (below threshold) -> should succeed
        gpio.set_pin(3, 1);
        let res = vm.run_std::<_, _, crate::drivers::MockNetwork>(&bytecode, &mut motor, &gpio, None);
        assert!(res.is_ok());

        // Set mock pin value high so ADC amplitude is high -> should trigger VibrationHazard
        gpio.set_pin(3, 100);
        let res = vm.run_std::<_, _, crate::drivers::MockNetwork>(&bytecode, &mut motor, &gpio, None);
        assert!(res.is_err());
        assert_eq!(motor.current_speed, 0); // verified safe shutdown!

        // 2. Test AvoidResonance (Pin 5: resonance)
        // Opcode 0x81, Pin 5, param_a (resonance_hz = 45), param_b (tolerance = 5, motor_chan = 1) -> packed: (5 << 24) | (1 << 16) = 0x05010000
        let tolerance = 5u32;
        let chan = 1u32;
        let param_b = (tolerance << 24) | (chan << 16);
        let param_b_bytes = param_b.to_le_bytes();

        let bytecode_2 = [
            0x81, 5, 45, 0, param_b_bytes[0], param_b_bytes[1], param_b_bytes[2], param_b_bytes[3]
        ];

        let mut vm2 = MicroVm::new(100);
        let mut motor2 = MockMotor::new();
        motor2.set_speed(50);
        gpio.set_pin(5, 5); // some vibration
        let res2 = vm2.run_std::<_, _, crate::drivers::MockNetwork>(&bytecode_2, &mut motor2, &gpio, None);
        assert!(res2.is_ok());
        // Since frequency (45Hz) falls inside tolerance, speed should shift/increase by +10% to 110!
        assert_eq!(motor2.current_speed, 110);
    }

    #[cfg(all(feature = "ptp", feature = "std"))]
    #[test]
    fn test_ptp_clock_and_delay_until() {
        use crate::drivers::MockMotor;
        use crate::vm::MicroVm;
        use crate::ptp::{set_local_hardware_time, PTP_CLOCK};

        set_local_hardware_time(1000);
        PTP_CLOCK.lock().set_offset(100); // 1100 synchronized time

        let target_time_us = 1150u64; // in future
        let param_a = (target_time_us >> 32) as u16;
        let param_b = (target_time_us & 0xFFFFFFFF) as u32;
        let param_b_bytes = param_b.to_le_bytes();

        let bytecode = [
            0x86, 0, (param_a & 0xFF) as u8, (param_a >> 8) as u8,
            param_b_bytes[0], param_b_bytes[1], param_b_bytes[2], param_b_bytes[3]
        ];

        // Spawn a thread to advance local time after 10ms
        let handle = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(10));
            set_local_hardware_time(1100); // synchronizes to 1200, passing target 1150
        });

        let mut vm = MicroVm::new(100);
        let mut motor = MockMotor::new();
        let gpio = crate::drivers::MockGpio::new();
        
        let res = vm.run_std::<_, _, crate::drivers::MockNetwork>(&bytecode, &mut motor, &gpio, None);
        assert!(res.is_ok());
        handle.join().unwrap();
    }

    #[cfg(feature = "std")]
    #[test]
    fn test_spatial_consensus_gossip() {
        use crate::drivers::{MockMotor, MockGpio, MockNetwork, MockState};
        use crate::vm::MicroVm;
        use crate::node::{TinyNode, GossipContext, get_time_us};

        let mut node = TinyNode::<_, _, MockState, MockGpio, 2>::new(
            [0; 6], // my MAC address
            MockNetwork::new(),
            MockMotor::new(),
            MockState::default(),
            MockGpio::new(),
        );

        // Opcode 0x87, Pin 3, param_a (Q15 energy threshold = 500)
        // param_b: K_neighbors = 2, TimeWindow_ms = 100, HighThreshold = 300
        // packed param_b = (2 << 24) | (100 << 16) | 300 = 0x0264012C
        let param_b_val = (2u32 << 24) | (100u32 << 16) | 300u32;
        let param_b_bytes = param_b_val.to_le_bytes();

        let bytecode = [
            0x87, 3, 0xF4, 0x01, param_b_bytes[0], param_b_bytes[1], param_b_bytes[2], param_b_bytes[3]
        ];

        // 1. Initial State: No neighbors confirming, vibration is high (amplitude 1000 so FFT energy exceeds 500)
        node.gpio.set_pin(3, 100); // high vibration
        node.motor.set_speed(100);

        let mut vm = MicroVm::new(100);
        let gossip_ctx = GossipContext {
            my_mac: node.id,
            network: &mut node.network,
            recent_gossip: &node.recent_gossip,
            current_time_us: get_time_us(),
        };

        // Should NOT trigger the hazard because no neighbors have asserted yet
        let res = vm.run_std(&bytecode, &mut node.motor, &node.gpio, Some(gossip_ctx));
        assert!(res.is_ok());
        assert_eq!(node.motor.current_speed, 100); // motor still runs

        // 2. Neighbor 1 asserts (MAC: [1; 6], hazard score 400)
        let n1_mac = [1; 6];
        let mut n1_payload = [0u8; 16];
        n1_payload[0..6].copy_from_slice(&n1_mac);
        let time_bytes = get_time_us().to_le_bytes();
        n1_payload[6..14].copy_from_slice(&time_bytes);
        n1_payload[14..16].copy_from_slice(&400u16.to_le_bytes());

        node.network.simulate_receive(n1_mac, OpCode::SpatialGossip, &n1_payload);
        node.tick(); // processes received frame and populates recent_gossip cache

        let mut vm = MicroVm::new(100);
        let gossip_ctx = GossipContext {
            my_mac: node.id,
            network: &mut node.network,
            recent_gossip: &node.recent_gossip,
            current_time_us: get_time_us(),
        };

        // Still should NOT trigger because we need K = 2 neighbors and only 1 neighbor is confirming
        let res = vm.run_std(&bytecode, &mut node.motor, &node.gpio, Some(gossip_ctx));
        assert!(res.is_ok());
        assert_eq!(node.motor.current_speed, 100);

        // 3. Neighbor 2 asserts (MAC: [2; 6], hazard score 500)
        let n2_mac = [2; 6];
        let mut n2_payload = [0u8; 16];
        n2_payload[0..6].copy_from_slice(&n2_mac);
        n2_payload[6..14].copy_from_slice(&time_bytes);
        n2_payload[14..16].copy_from_slice(&500u16.to_le_bytes());

        node.network.simulate_receive(n2_mac, OpCode::SpatialGossip, &n2_payload);
        node.tick();

        let mut vm = MicroVm::new(100);
        let gossip_ctx = GossipContext {
            my_mac: node.id,
            network: &mut node.network,
            recent_gossip: &node.recent_gossip,
            current_time_us: get_time_us(),
        };

        // NOW spatial consensus is achieved (2 neighbors confirming)!
        // Should trigger pre-alert MultiBandSpectrumHazard and execute safe shutdown (stop motor)
        let res = vm.run_std(&bytecode, &mut node.motor, &node.gpio, Some(gossip_ctx));
        assert_eq!(res, Err(VmError::MultiBandSpectrumHazard));
        assert_eq!(node.motor.current_speed, 0); // safe shutdown!
    }
}
