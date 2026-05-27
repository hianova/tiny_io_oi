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
    #[cfg(feature = "ptp")]
    DelayUntil      = 0x86,
    SpatialConsensusAssert = 0x87,
    HardwareFadePwm = 0x88,
    SensorFusion    = 0x89,
    ClosedLoopPID   = 0x8A,
    #[cfg(feature = "ptp")]
    SyncHibernate   = 0x8B,
    SpatialRanging  = 0x8C,
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
    pub fn execute_step<M: Motor, A: Adc, N: crate::Network>(
        opcode: u8,
        pin: u8,
        param_a: u16,
        param_b: u32,
        motor: &mut M,
        adc: &A,
        gossip_ctx: &mut Option<crate::node::GossipContext<'_, N>>,
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
                let motor_channel = ((param_b >> 16) & 0xFF) as u8;
                if motor_channel >= 8 {
                    return Err(VmError::UnauthorizedAccess);
                }

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
                let motor_channel = (param_a >> 8) as u8;
                let _tolerance_hz = (param_a & 0xFF) as u16;
                if motor_channel >= 8 {
                    return Err(VmError::UnauthorizedAccess);
                }
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
                    // Mid-freq structural resonance -> Auto調頻/Speed offset +15%
                    motor.set_speed(115);
                } else if low_energy > low_threshold {
                    // Low-freq load imbalance -> Limit max speed to 70%
                    motor.set_speed(70);
                }
            }

            // ==========================================
            // 0x86: DelayUntil (精密時間同步絕對時間戳執行)
            // ==========================================
            #[cfg(feature = "ptp")]
            0x86 => {
                let target_us = ((param_a as u64) << 32) | (param_b as u64);
                let start_us = crate::ptp::PTP_CLOCK.lock().get_time_us();
                let max_timeout_us = 10_000_000u64; // 10s safety timeout guard
                loop {
                    let current = crate::ptp::PTP_CLOCK.lock().get_time_us();
                    if current >= target_us {
                        break;
                    }
                    if current.saturating_sub(start_us) > max_timeout_us {
                        return Err(VmError::OutOfFuel); // Timeout safety fallback
                    }
                    #[cfg(feature = "std")]
                    std::thread::yield_now();
                }
            }

            // ==========================================
            // 0x87: SpatialConsensusAssert (空間共識斷言)
            // ==========================================
            0x87 => {
                let threshold_q15 = param_a;
                
                let k_neighbors = (param_b >> 24) as u8;
                let time_window_ms = ((param_b >> 16) & 0xFF) as u32;
                let high_threshold = (param_b & 0xFFFF) as u16;

                // Perform fixed-point FFT to get energies
                let mut magnitudes = [0.0f32; 128];
                heapless_fft_256(&raw_samples, &mut magnitudes);
                let (low_energy, mid_energy, _high_energy) = calculate_band_energies(&magnitudes, sample_rate);

                // Use low_energy + mid_energy as local hazard score
                let local_hazard = low_energy.saturating_add(mid_energy);

                // Check if our local energy exceeds the threshold
                if local_hazard > threshold_q15 {
                    if let Some(ctx) = gossip_ctx {
                        // 1. Broadcast our own Gossip frame
                        let mut payload = [0u8; 16];
                        payload[0..6].copy_from_slice(&ctx.my_mac);
                        
                        let time_le = ctx.current_time_us.to_le_bytes();
                        payload[6..14].copy_from_slice(&time_le);
                        
                        let score_le = local_hazard.to_le_bytes();
                        payload[14..16].copy_from_slice(&score_le);
                        
                        // Send over the network
                        let _ = ctx.network.broadcast(crate::OpCode::SpatialGossip, &payload);

                        // 2. Count unique neighbor MAC addresses within the time window
                        let mut unique_neighbors = heapless::Vec::<[u8; 6], 8>::new();
                        let time_window_us = (time_window_ms as u64) * 1000;
                        
                        for entry in ctx.recent_gossip.iter() {
                            // Verify the assertion timestamp is within the time window
                            let time_diff = ctx.current_time_us.saturating_sub(entry.timestamp_us);
                            if time_diff <= time_window_us && entry.sender_mac != ctx.my_mac && entry.hazard_score >= high_threshold {
                                if !unique_neighbors.contains(&entry.sender_mac) {
                                    let _ = unique_neighbors.push(entry.sender_mac);
                                }
                            }
                        }

                        #[cfg(feature = "std")]
                        println!(
                            "[SpatialConsensus] Local hazard: {} > threshold: {}. Unique confirming neighbors: {}/{}",
                            local_hazard, threshold_q15, unique_neighbors.len(), k_neighbors
                        );

                        // If we have >= K_neighbors confirming, we trigger the hazard!
                        if unique_neighbors.len() as u8 >= k_neighbors {
                            motor.stop(); // Safe Shutdown
                            return Err(VmError::MultiBandSpectrumHazard);
                        } else {
                            // Else we wait for spatial consensus
                            #[cfg(feature = "std")]
                            println!("[SpatialConsensus] Vibration detected but waiting for spatial consensus...");
                        }
                    }
                }
            }

            // ==========================================
            // 0x88: HardwareFadePwm (硬體漸變)
            // ==========================================
            0x88 => {
                let channel = pin;
                let target_duty = (param_a & 0xFF) as u8;
                let fade_ms = (param_b & 0xFFFF) as u16;
                motor.fade_to(channel, target_duty, fade_ms);
            }

            // ==========================================
            // 0x89: SensorFusion (感測器融合 - 互補濾波)
            // ==========================================
            0x89 => {
                // param_a: alpha in Q15 (e.g. 32112 for 0.98)
                // param_b: max allowed tilt angle in degrees
                let alpha_q15 = param_a as i32;
                let max_tilt = param_b as f32;
                let dt = 1.0f32 / sample_rate;

                // Mocking Accel and Gyro readings from raw_samples
                let mut current_angle = 0.0f32;
                for i in 0..(raw_samples.len() / 2) {
                    let accel_val = raw_samples[i] as f32 / 16384.0; // Assume 2g scale
                    let gyro_val = raw_samples[i + 128] as f32 / 131.0; // Assume 250deg/s scale
                    
                    let accel_angle = libm::atan2f(accel_val, 1.0) * 180.0 / core::f32::consts::PI;
                    let alpha = alpha_q15 as f32 / 32768.0;
                    current_angle = alpha * (current_angle + gyro_val * dt) + (1.0 - alpha) * accel_angle;
                }

                if libm::fabsf(current_angle) > max_tilt {
                    motor.stop();
                    return Err(VmError::SensorFusionHazard);
                }
            }

            // ==========================================
            // 0x8A: ClosedLoopPID (閉環 PID 控制)
            // ==========================================
            0x8A => {
                // param_a: Setpoint
                // param_b: [Kp, Ki, Kd, MotorChannel]
                let setpoint = param_a as i32;
                let kp = ((param_b >> 24) & 0xFF) as i32;
                let _ki = ((param_b >> 16) & 0xFF) as i32;
                let _kd = ((param_b >> 8) & 0xFF) as i32;
                let motor_channel = (param_b & 0xFF) as u8;

                if motor_channel >= 8 {
                    return Err(VmError::UnauthorizedAccess);
                }

                // Average the sensor buffer as the current state
                let mut sum = 0i32;
                for val in raw_samples.iter() {
                    sum += *val as i32;
                }
                let current_val = sum / (raw_samples.len() as i32);

                let error = setpoint - current_val;
                
                // Simple P controller for stateless step execution
                let output = (kp * error) / 100; // Q-scaling
                let clamped_output = output.clamp(0, 255) as u8;
                
                motor.set_speed(clamped_output);
                
                // Optional: if error is too catastrophic
                if error.abs() > 10000 {
                    return Err(VmError::PidHazard);
                }
            }

            // ==========================================
            // 0x8B: SyncHibernate (微秒級同步休眠)
            // ==========================================
            #[cfg(feature = "ptp")]
            0x8B => {
                // param_a: Sleep duration ms
                // param_b: Target epoch alignment ms
                let duration_ms = param_a as u64;
                let align_ms = param_b as u64;
                
                let current_us = crate::ptp::PTP_CLOCK.lock().get_time_us();
                let current_ms = current_us / 1000;
                
                let target_ms = if align_ms > 0 {
                    let remainder = current_ms % align_ms;
                    current_ms + (align_ms - remainder) + duration_ms
                } else {
                    current_ms + duration_ms
                };

                let target_us = target_ms * 1000;
                let max_timeout_us = 10_000_000u64; // 10s max block
                
                loop {
                    let now = crate::ptp::PTP_CLOCK.lock().get_time_us();
                    if now >= target_us {
                        break;
                    }
                    if now.saturating_sub(current_us) > max_timeout_us {
                        return Err(VmError::OutOfFuel);
                    }
                    #[cfg(feature = "std")]
                    std::thread::yield_now();
                }
            }

            // ==========================================
            // 0x8C: SpatialRanging (空間測距濾波)
            // ==========================================
            0x8C => {
                // param_a: RSSI threshold (positive value, represents -dBm, e.g. 80 for -80dBm)
                // param_b: Window size for median filter
                let threshold = param_a as i16;
                let window_size = (param_b & 0xFF) as usize;
                let w = window_size.clamp(1, 128);

                let mut window = [0i16; 128];
                for i in 0..w {
                    // Convert raw ADC mock to absolute RSSI scale (0..100)
                    window[i] = raw_samples[i].abs() % 100;
                }

                // Simple bubble sort for median on stack
                for i in 0..w {
                    for j in 0..(w - 1 - i) {
                        if window[j] > window[j + 1] {
                            let tmp = window[j];
                            window[j] = window[j + 1];
                            window[j + 1] = tmp;
                        }
                    }
                }

                let median_rssi = window[w / 2];

                if median_rssi > threshold { // e.g. > 80 means weaker than -80dBm -> too far or disconnected
                    motor.stop();
                    return Err(VmError::SpatialRangingHazard);
                }
            }

            _ => return Err(VmError::InvalidStdOpCode),
        }
        Ok(())
    }
}
