use crate::{Adc, VmError, Motor};

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StdOpCode {
    AssertVibration = 0x80,
    AvoidResonance  = 0x81,
    AcousticCheck   = 0x82,
    MultiBandAssert = 0x83,
    SpectrumAdaptive = 0x84,
    EnvelopeCheck   = 0x85,
}

/// A structure to hold static/stack complex values during FFT computation.
#[derive(Clone, Copy, Default)]
struct Complex32 {
    re: f32,
    im: f32,
}

impl Complex32 {
    #[inline(always)]
    fn new(re: f32, im: f32) -> Self {
        Self { re, im }
    }

    #[inline(always)]
    fn add(self, other: Self) -> Self {
        Self::new(self.re + other.re, self.im + other.im)
    }

    #[inline(always)]
    fn sub(self, other: Self) -> Self {
        Self::new(self.re - other.re, self.im - other.im)
    }

    #[inline(always)]
    fn mul(self, other: Self) -> Self {
        Self::new(
            self.re * other.re - self.im * other.im,
            self.re * other.im + self.im * other.re,
        )
    }

    #[inline(always)]
    fn norm(self) -> f32 {
        libm::sqrtf(self.re * self.re + self.im * self.im)
    }
}

/// A zero-allocation, heapless Cooley-Tukey Radix-2 FFT implementation on stack memory.
/// Supports N = 256 samples.
#[inline(always)]
fn heapless_fft_256(samples: &[i16], out_magnitudes: &mut [f32; 128]) {
    let mut data = [Complex32::default(); 256];
    let n = 256;

    // 1. Copy samples and apply bit-reversal permutation
    for i in 0..n {
        let mut rev = 0;
        let mut temp = i;
        for _ in 0..8 {
            rev = (rev << 1) | (temp & 1);
            temp >>= 1;
        }
        let val = if i < samples.len() { samples[i] as f32 } else { 0.0 };
        data[rev] = Complex32::new(val, 0.0);
    }

    // 2. Cooley-Tukey butterfly stage calculations
    let mut len = 2;
    while len <= n {
        let angle = -2.0f32 * core::f32::consts::PI / (len as f32);
        let w_len = Complex32::new(libm::cosf(angle), libm::sinf(angle));

        let mut i = 0;
        while i < n {
            let mut w = Complex32::new(1.0, 0.0);
            for j in 0..(len / 2) {
                let u = data[i + j];
                let v = data[i + j + len / 2].mul(w);
                data[i + j] = u.add(v);
                data[i + j + len / 2] = u.sub(v);
                w = w.mul(w_len);
            }
            i += len;
        }
        len <<= 1;
    }

    // 3. Compute magnitude spectrum for the first half
    for i in 0..128 {
        out_magnitudes[i] = data[i].norm();
    }
}

/// Perform heapless FFT and identify the peak resonant frequency and amplitude.
#[inline(always)]
pub fn local_fixed_fft(samples: &[i16], sample_rate: f32) -> (u32, u16) {
    let mut magnitudes = [0.0f32; 128];
    heapless_fft_256(samples, &mut magnitudes);

    let mut max_mag = -1.0f32;
    let mut peak_index = 0usize;

    // Exclude the DC offset component (index 0) from the peak finder
    for i in 1..128 {
        if magnitudes[i] > max_mag {
            max_mag = magnitudes[i];
            peak_index = i;
        }
    }

    let freq_resolution = sample_rate / 256.0f32;
    let peak_freq = (peak_index as f32) * freq_resolution;

    // Convert peak amplitude to Q15 (normalize relative to max possible amplitude)
    let amp_q15 = ((max_mag / 256.0) * 32768.0).min(65535.0) as u16;

    (peak_freq as u32, amp_q15)
}

/// Calculate energies in the Low (0~100Hz), Mid (100~1000Hz), and High (1000Hz+) frequency bands.
#[inline(always)]
fn calculate_band_energies(magnitudes: &[f32; 128], sample_rate: f32) -> (u16, u16, u16) {
    let freq_resolution = sample_rate / 256.0f32;
    let mut low_sum = 0.0f32;
    let mut mid_sum = 0.0f32;
    let mut high_sum = 0.0f32;

    for i in 1..128 {
        let freq = (i as f32) * freq_resolution;
        let mag = magnitudes[i];
        if freq < 100.0 {
            low_sum += mag;
        } else if freq < 1000.0 {
            mid_sum += mag;
        } else {
            high_sum += mag;
        }
    }

    let low_q15 = ((low_sum / 256.0) * 32768.0).min(65535.0) as u16;
    let mid_q15 = ((mid_sum / 256.0) * 32768.0).min(65535.0) as u16;
    let high_q15 = ((high_sum / 256.0) * 32768.0).min(65535.0) as u16;

    (low_q15, mid_q15, high_q15)
}

/// Embedded core executor for tiny_io_oi::std v1.0 and v1.1 bytecode steps.
pub struct StdExecutor;

impl StdExecutor {
    /// Executes semantic hardware standard library step.
    #[inline(always)]
    pub fn execute_step<M: Motor, A: Adc>(
        opcode: u8,
        pin: u8,
        param_a: u16,
        param_b: u32,
        motor: &mut M,
        adc: &A,
    ) -> Result<(), VmError> {
        // Read raw sensory ADC samples directly (using 256 stack samples)
        let mut raw_samples = [0i16; 256];
        adc.read_adc_buffer(pin, &mut raw_samples);

        // Constant simulated sampling rate (1000Hz)
        let sample_rate = 1000.0f32;

        match opcode {
            // ==========================================
            // 0x80: AssertVibration (震動安全斷言)
            // ==========================================
            0x80 => {
                let threshold_q15 = param_a;
                let max_hz = param_b;

                let (peak_hz, peak_amp_q15) = local_fixed_fft(&raw_samples, sample_rate);

                if peak_hz > max_hz && peak_amp_q15 > threshold_q15 {
                    motor.stop(); // Safe Shutdown
                    return Err(VmError::VibrationHazard {
                        hz: peak_hz,
                        amplitude: peak_amp_q15,
                    });
                }
            }

            // ==========================================
            // 0x81: AvoidResonance (共振規避/主動調頻)
            // ==========================================
            0x81 => {
                let resonance_hz = param_a;
                let tolerance_hz = (param_b >> 24) as u16;
                let _motor_channel = ((param_b >> 16) & 0xFF) as u8;

                let (peak_hz, _) = local_fixed_fft(&raw_samples, sample_rate);
                let diff = (peak_hz as i32 - resonance_hz as i32).abs() as u16;

                if diff <= tolerance_hz {
                    // Active frequency shifting: Adjust motor speed by +10% duty cycle
                    motor.set_speed(110);
                }
            }

            // ==========================================
            // 0x83: MultiBandAssert (多頻段複合斷言)
            // ==========================================
            0x83 => {
                let logic_op = (param_a >> 8) as u8; // 0 = AND, 1 = OR
                let band_mask = (param_a & 0xFF) as u8;
                
                let low_mid_threshold = (param_b >> 16) as u16;
                let high_threshold = (param_b & 0xFFFF) as u16;

                let mut magnitudes = [0.0f32; 128];
                heapless_fft_256(&raw_samples, &mut magnitudes);
                let (low_energy, mid_energy, high_energy) = calculate_band_energies(&magnitudes, sample_rate);

                let low_triggered = (band_mask & 0x01 != 0) && (low_energy > low_mid_threshold);
                let mid_triggered = (band_mask & 0x02 != 0) && (mid_energy > low_mid_threshold);
                let high_triggered = (band_mask & 0x04 != 0) && (high_energy > high_threshold);

                let is_hazard = if logic_op == 0 {
                    // AND Logic
                    let mut active_count = 0;
                    let mut trigger_count = 0;
                    if band_mask & 0x01 != 0 { active_count += 1; if low_triggered { trigger_count += 1; } }
                    if band_mask & 0x02 != 0 { active_count += 1; if mid_triggered { trigger_count += 1; } }
                    if band_mask & 0x04 != 0 { active_count += 1; if high_triggered { trigger_count += 1; } }
                    active_count > 0 && trigger_count == active_count
                } else {
                    // OR Logic
                    low_triggered || mid_triggered || high_triggered
                };

                if is_hazard {
                    motor.stop(); // Safe Shutdown
                    return Err(VmError::MultiBandSpectrumHazard);
                }
            }

            // ==========================================
            // 0x84: SpectrumAdaptive (頻譜自適應多段控制)
            // ==========================================
            0x84 => {
                let _motor_channel = (param_a >> 8) as u8;
                let _tolerance_hz = (param_a & 0xFF) as u16;
                let low_threshold = (param_b >> 16) as u16;
                let mid_threshold = (param_b & 0xFFFF) as u16;

                let mut magnitudes = [0.0f32; 128];
                heapless_fft_256(&raw_samples, &mut magnitudes);
                let (low_energy, mid_energy, high_energy) = calculate_band_energies(&magnitudes, sample_rate);

                if high_energy > 32768 {
                    // High-freq acoustic wear -> Force Safe Shutdown
                    motor.stop();
                    return Err(VmError::AcousticFailureDetected);
                } else if mid_energy > mid_threshold {
                    // Mid-freq structural resonance -> Auto调频/Speed offset +15%
                    motor.set_speed(115);
                } else if low_energy > low_threshold {
                    // Low-freq load imbalance -> Limit max speed to 70%
                    motor.set_speed(70);
                }
            }

            _ => return Err(VmError::InvalidStdOpCode),
        }
        Ok(())
    }
}
