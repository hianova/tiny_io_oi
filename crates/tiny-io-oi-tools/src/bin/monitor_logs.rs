use std::time::Duration;
use std::io::Read;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔌 Opening Soldier Native JTAG port...");

    let mut port = serialport::new("/dev/cu.usbmodem1401", 115_200)
        .timeout(Duration::from_millis(10))
        .open()?;

    println!("👂 Monitoring Soldier logs (will read for 12 seconds)...");
    let start = std::time::Instant::now();
    let mut buf = [0u8; 1024];

    while start.elapsed() < Duration::from_secs(12) {
        if let Ok(n) = port.read(&mut buf) {
            if n > 0 {
                let text = String::from_utf8_lossy(&buf[..n]);
                print!("{}", text);
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }

    println!("\n✓ Completed log monitoring.");
    Ok(())
}
