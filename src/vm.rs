use crate::{Motor, Gpio, Adc};
use io_oi_core::{ArchivedVmScript, ArchivedVmStep};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmError {
    OutOfFuel,
    AssertionFailed { pin: u8, expected: u8, actual: u8 },
    VibrationHazard { hz: u32, amplitude: u16 },
    MultiBandSpectrumHazard,
    AcousticFailureDetected,
    EnvelopeViolation,
    InvalidStdOpCode,
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

    /// Run the archived VM script in a zero-copy manner.
    pub fn run<M: Motor, G: Gpio>(
        &mut self,
        script: &ArchivedVmScript,
        motor: &mut M,
        gpio: &G,
    ) -> Result<(), VmError> {
        for step in script.steps.iter() {
            if self.fuel == 0 {
                self.trap_reason = Some(VmError::OutOfFuel);
                return Err(VmError::OutOfFuel);
            }
            self.fuel -= 1;

            match step {
                ArchivedVmStep::SetPwm { channel: _, speed } => {
                    motor.set_speed(*speed);
                }
                ArchivedVmStep::Delay { ticks } => {
                    let cost = (*ticks).min(self.fuel as u32);
                    self.fuel -= cost;
                }
                ArchivedVmStep::AssertOrYield { pin, expected } => {
                    let actual = gpio.read_pin(*pin);
                    if actual != *expected {
                        let err = VmError::AssertionFailed {
                            pin: *pin,
                            expected: *expected,
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
    pub fn run_std<M: Motor, A: Adc>(
        &mut self,
        bytecode: &[u8],
        motor: &mut M,
        adc: &A,
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
            let param_a = u16::from_le_bytes([bytecode[offset + 2], bytecode[offset + 3]]);
            let param_b = u32::from_le_bytes([
                bytecode[offset + 4],
                bytecode[offset + 5],
                bytecode[offset + 6],
                bytecode[offset + 7],
            ]);

            crate::std_impl::StdExecutor::execute_step(opcode, pin, param_a, param_b, motor, adc)?;
            offset += 8;
        }
        Ok(())
    }
}
