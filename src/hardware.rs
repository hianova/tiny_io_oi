#[cfg(not(feature = "std"))]
use alloc::vec::Vec;
#[cfg(feature = "std")]
use std::vec::Vec;

/// 嚴謹的硬體狀態約束
pub trait IoOiState: Default + Clone + core::fmt::Debug {
    /// 必須能序列化成位元組陣列 (支援你的連續記憶體與網路傳輸)
    fn to_bytes(&self) -> &[u8];
    /// 必須能從位元遮罩快速更新狀態
    fn apply_delta(&mut self, delta_mask: u8);
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[archive_attr(derive(rkyv::CheckBytes, Debug, PartialEq))]
pub enum Waveform {
    DigitalOut { state: u8 },
    Pwm8Bit { duty_cycle: u8 },
    AnalogOut { voltage_level: u16 },
    ServoAngle { angle: u8 },
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[archive_attr(derive(rkyv::CheckBytes, Debug, PartialEq))]
pub enum PhysicalStatus {
    Pending = 0x01,
    FailedRecoverable = 0x02,
    FailedFatal = 0x03,
    Success = 0x04,
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[archive_attr(derive(rkyv::CheckBytes))]
pub struct WaveformCmd {
    pub channel: u8,
    pub waveform: Waveform,
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone, PartialEq, Eq)]
#[archive_attr(derive(rkyv::CheckBytes))]
pub struct WaveformMatrix {
    pub commands: Vec<WaveformCmd>,
}

pub trait DigitalOutput {
    fn set_level(&mut self, high: bool);
}

pub trait PwmOutput {
    fn set_duty(&mut self, duty: u8);
}

pub struct HardwareRouter<D, P, const C: usize> {
    pub digital_pins: [Option<(u8, D)>; C],
    pub pwm_channels: [Option<(u8, P)>; C],
}

impl<D, P, const C: usize> HardwareRouter<D, P, C> {
    pub const fn new() -> Self {
        Self {
            digital_pins: [const { None }; C],
            pwm_channels: [const { None }; C],
        }
    }
}

impl<D: DigitalOutput, P: PwmOutput, const C: usize> HardwareRouter<D, P, C> {
    pub fn bind_digital(&mut self, channel: u8, pin: D) -> Result<(), &'static str> {
        for entry in self.digital_pins.iter_mut() {
            if entry.is_none() {
                *entry = Some((channel, pin));
                return Ok(());
            }
        }
        Err("Digital routing table full")
    }

    pub fn bind_pwm(&mut self, channel: u8, pwm: P) -> Result<(), &'static str> {
        for entry in self.pwm_channels.iter_mut() {
            if entry.is_none() {
                *entry = Some((channel, pwm));
                return Ok(());
            }
        }
        Err("PWM routing table full")
    }

    pub fn apply_waveforms(&mut self, payload: &[u8]) -> Result<(), &'static str> {
        if let Ok(archived) = rkyv::check_archived_root::<WaveformMatrix>(payload) {
            for cmd in archived.commands.iter() {
                let channel = cmd.channel;
                match &cmd.waveform {
                    ArchivedWaveform::DigitalOut { state } => {
                        for entry in self.digital_pins.iter_mut() {
                            if let Some((ch, pin)) = entry {
                                if *ch == channel {
                                    pin.set_level(*state > 0);
                                    break;
                                }
                            }
                        }
                    }
                    ArchivedWaveform::Pwm8Bit { duty_cycle } => {
                        for entry in self.pwm_channels.iter_mut() {
                            if let Some((ch, pwm)) = entry {
                                if *ch == channel {
                                    pwm.set_duty(*duty_cycle);
                                    break;
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(())
        } else {
            Err("Invalid WaveformMatrix payload")
        }
    }
}
