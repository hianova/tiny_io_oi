import { symbols } from "./index";
import { ptr } from "bun:ffi";

export interface Spectrum {
  /**
   * The calculated amplitude/magnitude spectrum.
   */
  magnitudes: Float32Array;
  /**
   * The calculated peak resonant frequency (Hz).
   */
  getPeakFrequency(): number;
}

/**
 * Perform a zero-allocation Fast Fourier Transform (FFT) on physical/IMU signal buffers.
 *
 * Utilizes direct-pointer passing to process standard JS Float32Array in Rust, bypassing all GC copy overheads.
 * 
 * @param rawBuffer Raw physical signals (e.g. IMU acceleration/vibration stream). Length must be a power of 2.
 * @param sampleRate Physical sensor sampling rate in Hz (defaults to 1000Hz).
 */
export function fft(rawBuffer: Float32Array, sampleRate: number = 1000): Spectrum {
  const n = rawBuffer.length;
  // Length safety checks for power-of-2 compliance
  if ((n & (n - 1)) !== 0 || n === 0) {
    throw new Error("FFT input buffer size must be a power of 2 (e.g., 128, 256, 512, 1024)");
  }

  const halfN = n / 2;
  const outputMag = new Float32Array(halfN);

  // Directly pass Float32Array pointers via Bun FFI
  const inputPtr = ptr(rawBuffer);
  const outputPtr = ptr(outputMag);

  const peakFreq = symbols.rust_fft(inputPtr, n, sampleRate, outputPtr);

  return {
    magnitudes: outputMag,
    getPeakFrequency() {
      return peakFreq;
    }
  };
}
