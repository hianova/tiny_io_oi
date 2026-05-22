#![no_std]
#![no_main]

extern crate alloc;

use esp_backtrace as _;
use esp_hal::{
    gpio::{Level, Output, OutputConfig},
    delay::Delay,
};
use tiny_io_oi::{
    TinyNode, OpCode, Gpio, Motor, Network, IoOiState, DigitalOutput, PwmOutput,
    HardwareRouter, Uart, GatewayBridge, NodeId,
};


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

struct EspNowNetwork<'a> {
    esp_now: esp_wifi::esp_now::EspNow<'a>,
}

impl<'a> Network for EspNowNetwork<'a> {
    fn broadcast(&mut self, opcode: OpCode, payload: &[u8]) -> Result<(), &'static str> {
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
                Ok(())
            }
            Err(_) => Err("ESP-NOW send failed"),
        }
    }

    fn receive<'b>(&mut self, buffer: &'b mut [u8]) -> Option<(NodeId, OpCode, &'b [u8])> {
        if let Some(received) = self.esp_now.receive() {
            let src = received.info.src_address;
            let data = received.data();
            if !data.is_empty() {
                let opcode = OpCode::from(data[0]);
                let payload = &data[1..];
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
    esp_alloc::heap_allocator!(size: 8192);

    let peripherals = esp_hal::init(esp_hal::Config::default());
    let _clocks = esp_hal::clock::Clocks::get();

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

    let delay = Delay::new();

    #[cfg(feature = "gateway")]
    {
        // Gateway Bridge role
        let uart = esp_hal::uart::Uart::new(
            peripherals.UART0,
            esp_hal::uart::Config::default(),
        )
        .unwrap();
        let esp_uart = EspUart { uart };

        let mut bridge = GatewayBridge::new(network, esp_uart);

        // Blink 3 times on boot to signal Gateway mode
        let mut boot_led = Output::new(peripherals.GPIO8, Level::Low, OutputConfig::default());
        for _ in 0..3 {
            boot_led.set_high();
            delay.delay_millis(200);
            boot_led.set_low();
            delay.delay_millis(200);
        }

        loop {
            bridge.tick();
            delay.delay_millis(5);
        }
    }

    #[cfg(not(feature = "gateway"))]
    {
        // Soldier Node role (either explicit feature soldier, or fallback)
        let led = Output::new(peripherals.GPIO8, Level::Low, OutputConfig::default());
        let extra_pin = Output::new(peripherals.GPIO9, Level::Low, OutputConfig::default());

        let motor = EspMotor { led_pin: led };
        let gpio = EspGpio;

        let mut router = HardwareRouter::<EspPin, EspMotor, 2>::new();
        let dig_pin = EspPin { pin: extra_pin };

        let _ = router.bind_digital(3, dig_pin);

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

        loop {
            node.tick();
            
            let dummy_payload = &[];
            let _ = router.apply_waveforms(dummy_payload);
            
            delay.delay_millis(10);
        }
    }
}
