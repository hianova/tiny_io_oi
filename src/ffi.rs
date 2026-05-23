#![cfg(feature = "std")]

use crate::{VmScript, VmStep, Motor};
use rustfft::{FftPlanner, num_complex::Complex};

/// A simple Rust-side builder to accumulate VmSteps before serializing them for FFI consumption.
pub struct ScriptBuilder {
    steps: Vec<VmStep>,
}

#[unsafe(no_mangle)]
pub extern "C" fn create_script_builder() -> *mut ScriptBuilder {
    let builder = Box::new(ScriptBuilder { steps: Vec::new() });
    Box::into_raw(builder)
}

#[unsafe(no_mangle)]
pub extern "C" fn script_builder_add_pwm(builder: *mut ScriptBuilder, channel: u8, speed: u8) {
    if !builder.is_null() {
        let builder = unsafe { &mut *builder };
        builder.steps.push(VmStep::SetPwm { channel, speed });
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn script_builder_add_delay(builder: *mut ScriptBuilder, ticks: u32) {
    if !builder.is_null() {
        let builder = unsafe { &mut *builder };
        builder.steps.push(VmStep::Delay { ticks });
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn script_builder_add_assert(builder: *mut ScriptBuilder, pin: u8, expected: u8) {
    if !builder.is_null() {
        let builder = unsafe { &mut *builder };
        builder.steps.push(VmStep::AssertOrYield { pin, expected });
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn script_builder_serialize(
    builder: *mut ScriptBuilder,
    out_len: *mut u32,
) -> *mut u8 {
    if builder.is_null() || out_len.is_null() {
        return std::ptr::null_mut();
    }
    let builder = unsafe { &*builder };
    let script = VmScript {
        steps: builder.steps.clone(),
    };

    match rkyv::to_bytes::<_, 512>(&script) {
        Ok(serialized) => {
            let mut serialized = serialized.into_vec();
            let len = serialized.len() as u32;
            unsafe { *out_len = len };
            let ptr = serialized.as_mut_ptr();
            std::mem::forget(serialized); // transfer ownership to the caller
            ptr
        }
        Err(_) => std::ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn script_builder_free(builder: *mut ScriptBuilder) {
    if !builder.is_null() {
        unsafe {
            let _ = Box::from_raw(builder);
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn free_serialized_bytes(ptr: *mut u8, len: u32) {
    if !ptr.is_null() {
        unsafe {
            let _ = Vec::from_raw_parts(ptr, len as usize, len as usize);
        }
    }
}

/// A zero-allocation FFI FFT function utilizing the high-speed rustfft library.
///
/// It processes input physical vibration/IMU signal buffer, calculates magnitude spectrum,
/// populates the output buffer, and returns the peak resonance frequency.
#[unsafe(no_mangle)]
pub extern "C" fn rust_fft(
    input_ptr: *const f32,
    len: u32,
    sample_rate: f32,
    output_magnitude_ptr: *mut f32,
) -> f32 {
    if input_ptr.is_null() || len == 0 || output_magnitude_ptr.is_null() {
        return 0.0;
    }

    let n = len as usize;
    let input = unsafe { std::slice::from_raw_parts(input_ptr, n) };

    // Convert input to Complex representation
    let mut buffer: Vec<Complex<f32>> = input.iter().map(|&x| Complex::new(x, 0.0)).collect();

    // Plan and process the forward FFT
    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(n);
    fft.process(&mut buffer);

    let half_n = n / 2;
    let output_mag = unsafe { std::slice::from_raw_parts_mut(output_magnitude_ptr, half_n) };

    let mut max_mag = -1.0f32;
    let mut peak_index = 0usize;

    for i in 0..half_n {
        let mag = buffer[i].norm(); // Compute norm/magnitude
        output_mag[i] = mag;
        // Exclude the DC offset component (index 0) from the peak finder
        if i > 0 && mag > max_mag {
            max_mag = mag;
            peak_index = i;
        }
    }

    let freq_resolution = sample_rate / (n as f32);
    let peak_freq = (peak_index as f32) * freq_resolution;

    peak_freq
}

/// Dynamic host FFI executor to run and verify semantic standard library bytecodes
/// inside the Rust MicroVM, testing motor reactions to physical ADCs/vibrations.
#[unsafe(no_mangle)]
pub extern "C" fn run_standard_bytecode(
    bytecode_ptr: *const u8,
    bytecode_len: u32,
    fuel: u32,
    initial_motor_speed: u8,
    pin_sensor_val: u8,
    out_final_motor_speed: *mut u8,
) -> i32 {
    if bytecode_ptr.is_null() || bytecode_len == 0 || out_final_motor_speed.is_null() {
        return -1;
    }

    let bytecode = unsafe { std::slice::from_raw_parts(bytecode_ptr, bytecode_len as usize) };
    let mut vm = crate::vm::MicroVm::new(fuel);
    let mut motor = crate::drivers::MockMotor::new();
    motor.set_speed(initial_motor_speed);

    let mut gpio = crate::drivers::MockGpio::new();
    gpio.set_pin(3, pin_sensor_val);
    gpio.set_pin(5, pin_sensor_val);

    match vm.run_std(bytecode, &mut motor, &gpio) {
        Ok(_) => {
            unsafe { *out_final_motor_speed = motor.current_speed };
            0 // Success
        }
        Err(e) => {
            unsafe { *out_final_motor_speed = motor.current_speed };
            match e {
                crate::vm::VmError::OutOfFuel => 1,
                crate::vm::VmError::VibrationHazard { .. } => 2,
                crate::vm::VmError::MultiBandSpectrumHazard => 3,
                crate::vm::VmError::AcousticFailureDetected => 4,
                _ => 99,
            }
        }
    }
}

#[cfg(feature = "verifier")]
#[unsafe(no_mangle)]
pub extern "C" fn rust_verify_std_bytecode(
    bytecode_ptr: *const u8,
    bytecode_len: u32,
    max_current_limit: u32,
    out_safe: *mut bool,
    out_report_len: *mut u32,
) -> *mut u8 {
    if bytecode_ptr.is_null() || bytecode_len == 0 || out_report_len.is_null() || out_safe.is_null() {
        return std::ptr::null_mut();
    }

    let bytecode = unsafe { std::slice::from_raw_parts(bytecode_ptr, bytecode_len as usize) };
    let result = crate::verifier::StaticVerifier::verify_std_bytecode(bytecode, max_current_limit);

    unsafe { *out_safe = result.safe };

    let mut report_bytes = result.report.into_bytes();
    let len = report_bytes.len() as u32;
    unsafe { *out_report_len = len };

    let ptr = report_bytes.as_mut_ptr();
    std::mem::forget(report_bytes); // transfer ownership to the caller
    ptr
}

#[cfg(feature = "verifier")]
#[unsafe(no_mangle)]
pub extern "C" fn rust_verify_vm_script_bytecode(
    bytecode_ptr: *const u8,
    bytecode_len: u32,
    max_current_limit: u32,
    out_safe: *mut bool,
    out_report_len: *mut u32,
) -> *mut u8 {
    if bytecode_ptr.is_null() || bytecode_len == 0 || out_report_len.is_null() || out_safe.is_null() {
        return std::ptr::null_mut();
    }

    let bytecode = unsafe { std::slice::from_raw_parts(bytecode_ptr, bytecode_len as usize) };
    
    // Check archived root VmScript
    if let Ok(archived) = rkyv::check_archived_root::<crate::VmScript>(bytecode) {
        let result = crate::verifier::StaticVerifier::verify_vm_script(archived, max_current_limit);
        unsafe { *out_safe = result.safe };

        let mut report_bytes = result.report.into_bytes();
        let len = report_bytes.len() as u32;
        unsafe { *out_report_len = len };

        let ptr = report_bytes.as_mut_ptr();
        std::mem::forget(report_bytes); // transfer ownership to the caller
        ptr
    } else {
        unsafe { 
            *out_safe = false;
            *out_report_len = 0;
        }
        std::ptr::null_mut()
    }
}
