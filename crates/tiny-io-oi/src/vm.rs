use crate::{Motor, Gpio, Adc};
use io_oi_core::embedded::VmScriptViewer;
use io_oi_core::VmStep;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmError {
    OutOfFuel,
    AssertionFailed { pin: u8, expected: u8, actual: u8 },
    VibrationHazard { hz: u32, amplitude: u16 },
    MultiBandSpectrumHazard,
    AcousticFailureDetected,
    EnvelopeViolation,
    InvalidStdOpCode,
    UnauthorizedAccess,
    SensorFusionHazard,
    PidHazard,
    SpatialRangingHazard,
    BufferOverflow,
}

pub struct MicroVm {
    pub fuel: u32,
    pub trap_reason: Option<VmError>,
}

impl MicroVm {
    pub fn new(fuel: u32) -> Self {
        Self {
            fuel,
            trap_reason: None,
        }
    }

    /// Run the archived VM script in a zero-copy manner using VmScriptViewer.
    pub fn run<M: Motor, G: Gpio>(
        &mut self,
        script_bytes: &[u8],
        motor: &mut M,
        gpio: &G,
    ) -> Result<(), VmError> {
        let viewer = VmScriptViewer::new(script_bytes);
        let count = viewer.step_count() as usize;

        for i in 0..count {
            if self.fuel == 0 {
                self.trap_reason = Some(VmError::OutOfFuel);
                return Err(VmError::OutOfFuel);
            }
            self.fuel -= 1;

            let step = viewer.get_step(i).ok_or(VmError::BufferOverflow)?;

            match step {
                VmStep::SetPwm { channel, speed } => {
                    if channel >= 8 {
                        self.trap_reason = Some(VmError::UnauthorizedAccess);
                        return Err(VmError::UnauthorizedAccess);
                    }
                    motor.set_speed(speed);
                }
                VmStep::Delay { ticks } => {
                    let cost = ticks.min(self.fuel as u32);
                    self.fuel -= cost;
                }
                VmStep::AssertOrYield { pin, expected } => {
                    if pin >= 32 {
                        self.trap_reason = Some(VmError::UnauthorizedAccess);
                        return Err(VmError::UnauthorizedAccess);
                    }
                    let actual = gpio.read_pin(pin);
                    if actual != expected {
                        let err = VmError::AssertionFailed {
                            pin,
                            expected,
                            actual,
                        };
                        self.trap_reason = Some(err);
                        return Err(err);
                    }
                }
            }
        }
        Ok(())
    }

    /// Execute semantic standard library bytecode script.
    pub fn run_std<M: Motor, A: Adc, N: crate::Network>(
        &mut self,
        bytecode: &[u8],
        motor: &mut M,
        adc: &A,
        mut gossip_ctx: Option<crate::node::GossipContext<'_, N>>,
    ) -> Result<(), VmError> {
        let mut offset = 0;
        while offset + 8 <= bytecode.len() {
            if self.fuel == 0 {
                self.trap_reason = Some(VmError::OutOfFuel);
                return Err(VmError::OutOfFuel);
            }
            self.fuel -= 1;

            let opcode = bytecode[offset];
            let pin = bytecode[offset + 1];
            if pin >= 32 {
                self.trap_reason = Some(VmError::UnauthorizedAccess);
                return Err(VmError::UnauthorizedAccess);
            }
            let param_a = u16::from_le_bytes([bytecode[offset + 2], bytecode[offset + 3]]);
            let param_b = u32::from_le_bytes([
                bytecode[offset + 4],
                bytecode[offset + 5],
                bytecode[offset + 6],
                bytecode[offset + 7],
            ]);

            crate::std_impl::StdExecutor::execute_step(opcode, pin, param_a, param_b, motor, adc, &mut gossip_ctx)?;
            offset += 8;
        }
        Ok(())
    }
}
