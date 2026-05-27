#![no_std]
#![no_main]

extern crate alloc;

esp_bootloader_esp_idf::esp_app_desc!();

use esp_backtrace as _;
use esp_hal::{
    gpio::{Level, Output, OutputConfig},
    delay::Delay,
};
use tiny_io_oi::{
    TinyNode, OpCode, Gpio, Motor, Network, IoOiState, DigitalOutput, PwmOutput,
    HardwareRouter, Uart, GatewayBridge, NodeId,
};
use embedded_io::Write as _;


fn custom_getrandom(buf: &mut [u8]) -> Result<(), getrandom::Error> {
    for b in buf.iter_mut() {
        *b = 0;
    }
    Ok(())
}
getrandom::register_custom_getrandom!(custom_getrandom);

// Support getrandom v0.3 custom backend via C linkage
#[unsafe(no_mangle)]
unsafe extern "C" fn __getrandom_v3_custom(dest: *mut u8, len: usize) -> u32 {
    let slice = unsafe { core::slice::from_raw_parts_mut(dest, len) };
    for b in slice.iter_mut() {
        *b = 0;
    }
    0 // Return 0 for Success
}

#[derive(Default, Clone, Debug)]
struct EspState;
impl IoOiState for EspState {
    fn to_bytes(&self) -> &[u8] {
        &[]
    }
    fn apply_delta(&mut self, _delta_mask: u8) {}
}

struct EspGpio;
impl Gpio for EspGpio {
    fn read_pin(&self, _pin: u8) -> u8 {
        1
    }
}

impl tiny_io_oi::Adc for EspGpio {
    fn read_adc_buffer(&self, _pin: u8, buffer: &mut [i16]) {
        #[cfg(feature = "roadshow")]
        {
            if !TRIGGER_VIBRATION.load(core::sync::atomic::Ordering::Relaxed) {
                for b in buffer.iter_mut() { *b = 0; }
                return;
            }
        }

        // Landslide prelude: 20Hz low-frequency deep vibration waves
        let sample_rate = 1000.0f32;
        let freq = 20.0f32;
        for i in 0..buffer.len() {
            let t = i as f32 / sample_rate;
            let val = libm::sinf(2.0f32 * core::f32::consts::PI * freq * t) * 5000.0f32;
            buffer[i] = val as i16;
        }
    }
}

#[cfg(feature = "roadshow")]
static TRIGGER_VIBRATION: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

struct EspUart<'a> {
    uart: esp_hal::uart::Uart<'a, esp_hal::Blocking>,
}

impl<'a> Uart for EspUart<'a> {
    fn read(&mut self) -> Option<u8> {
        if self.uart.read_ready() {
            let mut buf = [0u8; 1];
            match self.uart.read(&mut buf) {
                Ok(1) => Some(buf[0]),
                _ => None,
            }
        } else {
            None
        }
    }

    fn write(&mut self, data: &[u8]) -> Result<(), &'static str> {
        match self.uart.write(data) {
            Ok(_) => Ok(()),
            Err(_) => Err("UART write failed"),
        }
    }
}

struct EspUsbSerialJtag<'a> {
    usb: esp_hal::usb_serial_jtag::UsbSerialJtag<'a, esp_hal::Blocking>,
}

impl<'a> Uart for EspUsbSerialJtag<'a> {
    fn read(&mut self) -> Option<u8> {
        match self.usb.read_byte() {
            Ok(b) => Some(b),
            _ => None,
        }
    }

    fn write(&mut self, data: &[u8]) -> Result<(), &'static str> {
        match self.usb.write_all(data) {
            Ok(_) => Ok(()),
            Err(_) => Err("USB JTAG write failed"),
        }
    }
}

struct EspNowNetwork<'a> {
    esp_now: esp_wifi::esp_now::EspNow<'a>,
}

impl<'a> Network for EspNowNetwork<'a> {
    fn broadcast(&mut self, opcode: OpCode, payload: &[u8]) -> Result<(), &'static str> {
        esp_println::println!("📤 ESP-NOW Broadcasting OpCode: {:?}, Payload: {:02X?}", opcode, payload);
        let mut msg = [0u8; 256];
        if payload.len() + 1 > msg.len() {
            return Err("Payload too long");
        }
        msg[0] = opcode as u8;
        msg[1..1 + payload.len()].copy_from_slice(payload);
        
        let broadcast_mac = [0xFF; 6];
        if !self.esp_now.peer_exists(&broadcast_mac) {
            let _ = self.esp_now.add_peer(esp_wifi::esp_now::PeerInfo {
                peer_address: broadcast_mac,
                lmk: None,
                channel: None,
                encrypt: false,
                interface: esp_wifi::esp_now::EspNowWifiInterface::Sta,
            });
        }
        
        match self.esp_now.send(&broadcast_mac, &msg[..1 + payload.len()]) {
            Ok(waiter) => {
                let _ = waiter.wait();
                esp_println::println!("✓ ESP-NOW Broadcast completed successfully!");
                Ok(())
            }
            Err(_) => {
                esp_println::println!("❌ ESP-NOW Broadcast failed!");
                Err("ESP-NOW send failed")
            }
        }
    }

    fn receive<'b>(&mut self, buffer: &'b mut [u8]) -> Option<(NodeId, OpCode, &'b [u8])> {
        if let Some(received) = self.esp_now.receive() {
            let src = received.info.src_address;
            let data = received.data();
            if !data.is_empty() {
                let opcode = OpCode::from(data[0]);
                let payload = &data[1..];
                esp_println::println!("📥 ESP-NOW Recv from {:02X?}, opcode: {:?}, payload_len: {}", src, opcode, payload.len());
                let len = payload.len().min(buffer.len());
                buffer[..len].copy_from_slice(&payload[..len]);
                return Some((src, opcode, &buffer[..len]));
            }
        }
        None
    }
}

struct EspMotor<'a> {
    led_pin: Output<'a>,
}

impl<'a> Motor for EspMotor<'a> {
    fn set_speed(&mut self, speed: u8) {
        if speed > 0 {
            self.led_pin.set_high();
        } else {
            self.led_pin.set_low();
        }
    }

    fn stop(&mut self) {
        self.led_pin.set_low();
    }
}

impl<'a> PwmOutput for EspMotor<'a> {
    fn set_duty(&mut self, duty: u8) {
        if duty > 0 {
            self.led_pin.set_high();
        } else {
            self.led_pin.set_low();
        }
    }
}

struct EspPin<'a> {
    pin: Output<'a>,
}

impl<'a> DigitalOutput for EspPin<'a> {
    fn set_level(&mut self, high: bool) {
        if high {
            self.pin.set_high();
        } else {
            self.pin.set_low();
        }
    }
}

#[esp_hal::main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default());
    let delay = Delay::new();
    delay.delay_millis(2000);

    esp_println::println!("DEBUG: ESP32 main starting after 2s USB reconnection delay...");
    esp_alloc::heap_allocator!(size: 131_072);

    // Initialize esp-wifi stack
    let systimer = esp_hal::timer::systimer::SystemTimer::new(peripherals.SYSTIMER);
    let timer = systimer.alarm0;

    let init = esp_wifi::init(
        timer,
        esp_hal::rng::Rng::new(peripherals.RNG),
    )
    .unwrap();

    let wifi = peripherals.WIFI;
    let (_controller, interfaces) = esp_wifi::wifi::new(&init, wifi).unwrap();
    let esp_now = interfaces.esp_now;
    let network = EspNowNetwork { esp_now };



    #[cfg(feature = "gateway")]
    {
        // Gateway Bridge role using native USB JTAG Serial
        let usb = esp_hal::usb_serial_jtag::UsbSerialJtag::new(
            peripherals.USB_DEVICE,
        );
        let esp_usb = EspUsbSerialJtag { usb };

        let mut bridge = GatewayBridge::new(network, esp_usb);

        // Blink 3 times on boot to signal Gateway mode
        let mut boot_led = Output::new(peripherals.GPIO8, Level::Low, OutputConfig::default());
        for _ in 0..3 {
            boot_led.set_high();
            delay.delay_millis(200);
            boot_led.set_low();
            delay.delay_millis(200);
        }

        esp_println::println!("⚡ Gateway Bridge started! Ready to route USB UART <-> ESP-NOW");

        loop {
            bridge.tick();
            delay.delay_millis(5);
        }
    }

    #[cfg(not(feature = "gateway"))]
    {
        // Soldier Node role (either explicit feature soldier, or fallback)
        let led = Output::new(peripherals.GPIO8, Level::Low, OutputConfig::default());
        
        #[cfg(not(feature = "roadshow"))]
        let extra_pin = Output::new(peripherals.GPIO9, Level::Low, OutputConfig::default());
        
        #[cfg(feature = "roadshow")]
        let mut boot_button = esp_hal::gpio::Input::new(peripherals.GPIO9, esp_hal::gpio::InputConfig::default().with_pull(esp_hal::gpio::Pull::Up));

        let motor = EspMotor { led_pin: led };
        let gpio = EspGpio;

        let mut router = HardwareRouter::<EspPin, EspMotor, 2>::new();
        
        #[cfg(not(feature = "roadshow"))]
        {
            let dig_pin = EspPin { pin: extra_pin };
            let _ = router.bind_digital(3, dig_pin);
        }

        // Soldier A node MAC address
        let my_mac = [0x02, 0x02, 0x02, 0x02, 0x02, 0x02];

        let mut node = TinyNode::<_, _, _, _, 2>::new(
            my_mac,
            network,
            motor,
            EspState,
            gpio,
        );

        // Blink 5 times on boot to signal Soldier mode
        for _ in 0..5 {
            node.motor.led_pin.set_high();
            delay.delay_millis(150);
            node.motor.led_pin.set_low();
            delay.delay_millis(150);
        }

        esp_println::println!("⚡ Soldier Node started! MAC: {:02X?}", my_mac);

        let mut tick_cnt = 0;
        loop {
            #[cfg(feature = "roadshow")]
            {
                if boot_button.is_low() {
                    let was_triggered = TRIGGER_VIBRATION.swap(true, core::sync::atomic::Ordering::Relaxed);
                    if !was_triggered {
                        esp_println::println!("🚨 [ROADSHOW] BOOT Button Pressed! Simulating 20Hz vibration...");
                    }
                }
            }
            node.tick();
            
            tick_cnt += 1;
            if tick_cnt % 200 == 0 {
                esp_println::println!("Node tick #{}, safe_mode: {}, leader: {:?}", tick_cnt, node.safe_mode, node.get_leader());
            }
            
            let dummy_payload = &[];
            let _ = router.apply_waveforms(dummy_payload);
            
            delay.delay_millis(10);
        }
    }
}
