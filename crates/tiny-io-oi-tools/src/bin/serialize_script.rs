use tiny_io_oi::{VmScript, VmStep};

fn main() {
    // A blinking script: 3 cycles of On (500ms) and Off (500ms)
    let script = VmScript {
        steps: vec![
            VmStep::SetPwm { channel: 0, speed: 255 },
            VmStep::Delay { ticks: 50 },
            VmStep::SetPwm { channel: 0, speed: 0 },
            VmStep::Delay { ticks: 50 },
            VmStep::SetPwm { channel: 0, speed: 255 },
            VmStep::Delay { ticks: 50 },
            VmStep::SetPwm { channel: 0, speed: 0 },
            VmStep::Delay { ticks: 50 },
            VmStep::SetPwm { channel: 0, speed: 255 },
            VmStep::Delay { ticks: 50 },
            VmStep::SetPwm { channel: 0, speed: 0 },
            VmStep::Delay { ticks: 50 },
        ],
    };

    let serialized = rkyv::to_bytes::<_, 512>(&script).unwrap();
    
    // Print the raw bytes formatted as a hex string
    let hex_str: String = serialized.iter().map(|b| format!("{:02x}", b)).collect();
    println!("{}", hex_str);
}
