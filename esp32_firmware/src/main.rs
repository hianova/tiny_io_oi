#![no_std]
#![no_main]

extern crate alloc;

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


use esp_backtrace as _;
use esp_hal::{
    gpio::{Level, Output},
    delay::Delay,
};
use tiny_io_oi::{TinyNode, OpCode, Gpio, Motor, Network, IoOiState, DigitalOutput, PwmOutput, HardwareRouter};

esp_bootloader_esp_idf::esp_app_desc!();

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

struct EspGpio;
impl Gpio for EspGpio {
    fn read_pin(&self, _pin: u8) -> u8 {
        1
    }
}

struct EspNetwork {
    received_script: bool,
}

impl Network for EspNetwork {
    fn broadcast(&mut self, _opcode: OpCode, _payload: &[u8]) -> Result<(), &'static str> {
        Ok(())
    }

    fn receive<'b>(&mut self, buffer: &'b mut [u8]) -> Option<(tiny_io_oi::NodeId, OpCode, &'b [u8])> {
        if !self.received_script {
            self.received_script = true;
            let script = tiny_io_oi::VmScript {
                steps: alloc::vec![
                    tiny_io_oi::VmStep::SetPwm { channel: 0, speed: 255 },
                    tiny_io_oi::VmStep::AssertOrYield { pin: 1, expected: 1 },
                ],
            };
            if let Ok(serialized) = rkyv::to_bytes::<_, 256>(&script) {
                let len = serialized.len().min(buffer.len());
                buffer[..len].copy_from_slice(&serialized[..len]);
                return Some(([1; 6], OpCode::VmScriptDispatch, &buffer[..len]));
            }
        }
        None
    }
}

#[derive(Default, Clone, Debug)]
struct EspState;
impl IoOiState for EspState {
    fn to_bytes(&self) -> &[u8] {
        &[]
    }
    fn apply_delta(&mut self, _delta_mask: u8) {}
}

#[esp_hal::entry]
fn main() -> ! {

    // heap 分配器應在函數內部被調用擴展！
    esp_alloc::heap_allocator!(8192);

    let peripherals = esp_hal::init(esp_hal::Config::default());

    // 直接使用 peripherals.GPIO8 初始化內建 LED 的 Output 引腳
    let led = Output::new(peripherals.GPIO8, Level::Low);
    // 使用 peripherals.GPIO9 初始化額外的 GPIO Output 引腳用以展示 HardwareRouter
    let extra_pin = Output::new(peripherals.GPIO9, Level::Low);

    let motor = EspMotor { led_pin: led };
    let gpio = EspGpio;
    let network = EspNetwork { received_script: false };

    let mut router = HardwareRouter::<EspPin, EspMotor, 2>::new();
    let dig_pin = EspPin { pin: extra_pin };

    // 進行 HardwareRouter 的靜態零分配綁定
    let _ = router.bind_digital(3, dig_pin);

    let mut node = TinyNode::<_, _, _, _, 2>::new(
        [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF],
        network,
        motor,
        EspState,
        gpio,
    );

    let delay = Delay::new();

    // Blink 測試 (亮滅 3 次驗證硬體)
    for _ in 0..3 {
        node.motor.led_pin.set_high();
        delay.delay_millis(200);
        node.motor.led_pin.set_low();
        delay.delay_millis(200);
    }

    // 進入 Tiny Swarm 零拷貝大腦與物理反射的運行主迴圈
    loop {
        node.tick();
        
        // 演示：透過 HardwareRouter 套用電氣波形訊號
        let dummy_payload = &[];
        let _ = router.apply_waveforms(dummy_payload);
        
        delay.delay_millis(500);
    }
}
