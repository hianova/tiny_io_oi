import { TinyNode } from "./index";
import { fft } from "./dsp";

console.log("⚡ Starting Bun FFI Bindings Integration Verification...");

// =========================================================================
// 1. Verify VmScript Compilation & Serialization FFI
// =========================================================================
console.log("\n[Test 1] Testing ScriptBuilder & TinyNode FFI...");
const bav = new TinyNode(0x01);

const bytecode = bav.buildScript((builder) => {
  builder.setPwm(1, 255)
         .delay(200)
         .assertOrYield(5, true);
});

console.log(`✓ Script compiled successfully! Bytecode length: ${bytecode.length} bytes`);
console.log(`✓ Hex dump: ${Array.from(bytecode).map(b => b.toString(16).padStart(2, "0")).join("")}`);

// Basic size validation
if (bytecode.length === 0) {
  throw new Error("Serialized bytecode is empty!");
}

// =========================================================================
// 2. Verify High-Speed FFT DSP FFI
// =========================================================================
console.log("\n[Test 2] Testing zero-allocation FFT computation...");
const N = 256;
const sampleRate = 200; // 200 Hz
const targetFreq = 25;  // 25 Hz sine wave
const signal = new Float32Array(N);

for (let i = 0; i < N; i++) {
  const t = i / sampleRate;
  signal[i] = Math.sin(2 * Math.PI * targetFreq * t);
}

const spectrum = fft(signal, sampleRate);
const peakResonance = spectrum.getPeakFrequency();

console.log(`✓ FFT calculations completed successfully!`);
console.log(`✓ Input Sine Wave Frequency: ${targetFreq} Hz`);
console.log(`✓ Detected Resonant Peak Frequency: ${peakResonance} Hz`);

// Assert resonance peak accuracy
if (Math.abs(peakResonance - targetFreq) > 1.0) {
  throw new Error(`Resonant peak frequency mismatch! Expected: ${targetFreq}, Got: ${peakResonance}`);
}

console.log("\n🎉 ALL Bun FFI integration tests passed successfully!");
