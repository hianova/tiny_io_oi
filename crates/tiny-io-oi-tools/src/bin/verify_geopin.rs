use std::time::Duration;
use std::io::Read;
use tiny_io_oi::{GatewayFrame, OpCode};

fn send_frame(port: &mut dyn serialport::SerialPort, mac_addr: [u8; 6], payload: Vec<u8>) -> Result<(), Box<dyn std::error::Error>> {
    let frame = GatewayFrame {
        mac_addr,
        payload,
    };
    let serialized = rkyv::to_bytes::<_, 256>(&frame).unwrap();
    let len = serialized.len() as u16;
    
    let mut serial_packet = vec![0xDE, 0xAD];
    serial_packet.extend_from_slice(&len.to_be_bytes());
    serial_packet.extend_from_slice(&serialized);
    
    port.write_all(&serial_packet)?;
    port.flush()?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("⚡ Starting E2E Landslide Geopin (智慧地釘) Verification...");

    // 1. Open the Gateway serial port `/dev/cu.usbmodem11401`
    let port_path = "/dev/cu.usbmodem11401";
    println!("🔌 Opening Gateway serial port: {}", port_path);
    let mut port = serialport::new(port_path, 115_200)
        .timeout(Duration::from_millis(100))
        .open()?;

    // Let the Gateway reboot/initialize
    println!("⏳ Waiting 2s for Gateway to boot up...");
    std::thread::sleep(Duration::from_secs(2));

    // 2. Heal the Soldier node by transmitting a leader Heartbeat frame first
    // OpCode: Heartbeat (0x04)
    println!("📤 Transmitting Heartbeat (0x04) to heal Soldier node safe mode...");
    send_frame(&mut *port, [0xFF; 6], vec![0x04])?;

    // Wait 100ms for state update propagation
    std::thread::sleep(Duration::from_millis(100));

    // 3. Build the standard library MultiBandAssert bytecode
    // OpCode: 0x83 (MultiBandAssert)
    // Pin: 3
    // Param A: (LogicOp.OR << 8) | Band.LOW = 0x0101
    // Param B: (LowMidQ15 << 16) | HighQ15
    // lowMidThreshold = 0.02 -> 0.02 * 32768 = 655 = 0x028F
    // highThreshold = 0.9 -> 0.9 * 32768 = 29491 = 0x7333
    // So Param B = 0x028F7333
    // Bytes: 83 03 01 01 33 73 8F 02
    let std_bytecode = vec![0x83, 0x03, 0x01, 0x01, 0x33, 0x73, 0x8F, 0x02];

    // Wrap standard library bytecode inside VmScriptDispatch (0x40)
    let mut payload = vec![0x40]; // VmScriptDispatch (which triggers our dual-format fallback on the Node!)
    payload.extend_from_slice(&std_bytecode);

    println!("📤 Transmitting 8-byte MultiBandAssert bytecode over Gateway serial...");
    send_frame(&mut *port, [0xFF; 6], payload)?;

    // 4. Listen for returned exception frame
    println!("📥 Listening for landslide pre-alert exception reports from the Soldier node...");
    let mut rx_buf = Vec::new();
    let start = std::time::Instant::now();
    let mut exception_found = false;

    while start.elapsed() < Duration::from_secs(6) {
        let mut byte = [0u8; 1];
        if port.read_exact(&mut byte).is_ok() {
            rx_buf.push(byte[0]);

            if rx_buf.len() >= 4 {
                if rx_buf[0] == 0xDE && rx_buf[1] == 0xAD {
                    let packet_len = u16::from_be_bytes([rx_buf[2], rx_buf[3]]) as usize;
                    if rx_buf.len() >= 4 + packet_len {
                        let frame_bytes = &rx_buf[4..4 + packet_len];
                        if let Ok(archived) = rkyv::check_archived_root::<GatewayFrame>(frame_bytes) {
                            let src = archived.mac_addr;
                            let inner_payload = &archived.payload;
                            if !inner_payload.is_empty() {
                                let opcode = OpCode::from(inner_payload[0]);
                                if opcode == OpCode::Exception {
                                    println!("\n🍀 Landslide Pre-Alert EXCEPTION RECEIVED!");
                                    println!("- From Soldier MAC: {:02X?}", src);
                                    println!("- Exception Payload: {:02X?}", inner_payload);
                                    if inner_payload.len() >= 2 && inner_payload[1] == 0x03 {
                                        println!("🎉 SUCCESS: Landslide MultiBandSpectrumHazard detected and verified!");
                                    }
                                    exception_found = true;
                                    break;
                                }
                            }
                        }
                        rx_buf.drain(..4 + packet_len);
                    }
                } else {
                    rx_buf.remove(0);
                }
            }
        }
    }

    if !exception_found {
        println!("\n❌ TIMEOUT: Did not receive exception frame from Soldier node. Check connections.");
    }

    Ok(())
}
