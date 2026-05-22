use crate::{Motor, Gpio};
use io_oi_core::{ArchivedVmScript, ArchivedVmStep};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmError {
    OutOfFuel,
    AssertionFailed { pin: u8, expected: u8, actual: u8 },
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
}
