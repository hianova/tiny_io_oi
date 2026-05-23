use spin::Mutex;

/// Global thread-safe PTP Clock instance.
pub static PTP_CLOCK: Mutex<PtpClock> = Mutex::new(PtpClock::new());

/// Platform time provider in microseconds.
/// In a standard environment, uses SystemTime.
/// In no_std environment, uses a static hardware clock updated by the driver or mock.
static mut LOCAL_HARDWARE_TIME_US: u64 = 0;

/// Set the local hardware time in microseconds. Can be called from ESP32 timer ISR/tick loops.
pub fn set_local_hardware_time(us: u64) {
    unsafe {
        LOCAL_HARDWARE_TIME_US = us;
    }
}

/// Precise Time Protocol Clock.
pub struct PtpClock {
    offset_us: i64,
}

impl PtpClock {
    pub const fn new() -> Self {
        Self { offset_us: 0 }
    }

    /// Sets the synchronized PTP clock offset relative to Leader node.
    pub fn set_offset(&mut self, offset: i64) {
        self.offset_us = offset;
    }

    /// Gets the current synchronized PTP clock offset.
    pub fn get_offset(&self) -> i64 {
        self.offset_us
    }

    /// Computes the synchronized microsecond timestamp.
    /// node_time = local_time + offset
    pub fn get_time_us(&self) -> u64 {
        #[cfg(feature = "std")]
        {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_micros() as u64;
            (now as i64 + self.offset_us) as u64
        }
        #[cfg(not(feature = "std"))]
        {
            unsafe { (LOCAL_HARDWARE_TIME_US as i64 + self.offset_us) as u64 }
        }
    }
}

/// Microsecond precision PTP clock offset calculation.
///
/// Under IEEE 1588 Precision Time Protocol standard exchange:
/// 1. T1: Master sends Sync packet (Master timestamp)
/// 2. T2: Slave receives Sync packet (Slave timestamp)
/// 3. T3: Slave sends Delay Request (Slave timestamp)
/// 4. T4: Master receives Delay Request (Master timestamp)
///
/// Formula:
/// - Offset = ((T2 - T1) - (T4 - T3)) / 2
/// - Delay = ((T2 - T1) + (T4 - T3)) / 2
pub fn calculate_offset_and_delay(t1: u64, t2: u64, t3: u64, t4: u64) -> (i64, u64) {
    let offset = ((t2 as i64 - t1 as i64) - (t4 as i64 - t3 as i64)) / 2;
    let delay = ((t2 as i64 - t1 as i64) + (t4 as i64 - t3 as i64)) / 2;
    (offset, delay.max(0) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ptp_precision_offset_math() {
        // Master T1: 1000us
        // Slave receives at T2: 1050us
        // Slave sends Delay Req at T3: 2000us
        // Master receives Delay Req at T4: 2030us
        let (offset, delay) = calculate_offset_and_delay(1000, 1050, 2000, 2030);
        
        // Offset = ((1050 - 1000) - (2030 - 2000)) / 2 = (50 - 30) / 2 = 10us
        // Delay = ((1050 - 1000) + (2030 - 2000)) / 2 = (50 + 30) / 2 = 40us
        assert_eq!(offset, 10);
        assert_eq!(delay, 40);
    }

    #[test]
    fn test_ptp_clock_offset_application() {
        set_local_hardware_time(5000);
        let mut clock = PtpClock::new();
        clock.set_offset(-200);
        assert_eq!(clock.get_offset(), -200);
        
        #[cfg(not(feature = "std"))]
        {
            assert_eq!(clock.get_time_us(), 4800);
        }
    }
}
