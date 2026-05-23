# ⚡️ tiny_io_oi Bun FFI Bindings

Bilingual Developer Guide & Examples (中英文開發者指南與範例)

---

## 🌐 Introduction / 簡介

### English
The `bun` bindings enable high-level, extremely fast control of bare-metal swarm endpoints (e.g., ESP32-C6 microcontrollers) from the comfort of JavaScript/TypeScript using **Bun's ultra-low-latency Native FFI**. 
By compiling high-level directives into extremely compact, zero-copy `VmScript` bytecode, developers can deploy complex reflex automation scripts to swarms in real-time. Additionally, high-speed DSP calculations (like FFT) are executed in raw Rust threads with zero garbage collection (GC) overhead.

### 繁體中文
`bun` 綁定使開發者能直接在 JavaScript/TypeScript 環境中，利用 **Bun 的極速 Native FFI**，無縫控制 bare-metal 蜂群端點（如 ESP32-C6 微控制器）。
高階的邏輯與決策會在 Bun 容器中被瞬間編譯成極簡、零拷貝的二進位 `VmScript` 位元組流，並透過無線網絡分發。同時，耗費性能的高頻數位訊號處理（如 FFT 傅立葉轉換）會在 Rust 底層以微秒級速度處理，對 JS 主執行緒達成零 GC 負擔。

---

## 🛠️ Setup & Compilation / 環境配置與編譯

### English
Before running the TypeScript scripts, you must compile the Rust dynamic library (`.dylib` on macOS, `.so` on Linux, `.dll` on Windows):

```bash
# 1. Compile the cdylib dynamic library
cargo build

# 2. Verify that the library is generated
# On macOS: target/debug/libtiny_io_oi.dylib
# On Linux: target/debug/libtiny_io_oi.so
```

### 繁體中文
在執行 TypeScript 程式碼前，您必須編譯 Rust 動態庫：

```bash
# 1. 編譯 cdylib 動態庫
cargo build

# 2. 確認動態庫成功生成
# macOS 系統下路徑為: target/debug/libtiny_io_oi.dylib
# Linux 系統下路徑為: target/debug/libtiny_io_oi.so
```

---

## 📐 Core API Reference / 核心 API 說明

### `TinyNode(nodeId: number)`
*   `buildScript(callback: (builder: ScriptBuilder) => void): Uint8Array`
    *   Compiles a dynamic hardware script and returns a compiled `Uint8Array` ready for serial/network broadcast.
    *   將硬體控制邏輯編譯成可用於序列埠或網絡廣播的 `Uint8Array` 位元組流。

### `ScriptBuilder`
*   `setPwm(channel: number, speed: number): this`
    *   Sets PWM duty cycle (`0`–`255`) on a specified channel.
    *   設定指定通道的實體 PWM 佔空比（`0`–`255`）。
*   `delay(ticks: number): this`
    *   Busy-waits for a specific number of CPU ticks.
    *   使端點虛擬機忙等待指定的 CPU 週期。
*   `assertOrYield(pin: number, expected: boolean | number): this`
    *   Asserts a GPIO pin level. Aborts execution and triggers `Safe Shutdown` if assertion fails.
    *   斷言指定 GPIO 腳位電平。若斷言失敗則立刻中斷執行並觸發「安全馬達關閉」。

### `fft(rawBuffer: Float32Array, sampleRate?: number): Spectrum`
*   Performs a zero-allocation Fast Fourier Transform (FFT) on the raw buffer.
*   對傳感器浮點陣列進行零拷貝、零分配的傅立葉轉換。
*   Returns `Spectrum` with `getPeakFrequency(): number`.
*   回傳包含 `getPeakFrequency()`（獲取最大振幅共振頻率）的 `Spectrum` 對象。

---

## 📚 Complete Examples / 完整開發範例

### 💡 Example 1: Blinking Swarm LED (Bytecode Compilation & Broadcast)
#### 範例一：控制蜂群 LED 閃爍（字節碼編譯與廣播）

This example demonstrates how a TypeScript developer can dynamically compile a 3-cycle blinking VmScript and prepare it for broadcast.
此範例展示如何使用 TS 動態編譯一個 3 週期的 LED 閃爍字節碼指令流。

```typescript
import { TinyNode } from "./index";

// 1. Create a virtual actuator node instance representing BAV (Node ID: 0x01)
// 建立一個代表 BAV 寶特瓶載具的端點實例 (節點 ID: 0x01)
const bav = new TinyNode(0x01);

// 2. Compile hardware control directives into compact binary VmScript bytecode
// 將硬體控制指令編譯為緊湊的二進位 VmScript 字節碼
const blinkBytecode = bav.buildScript((builder) => {
  builder
    .setPwm(1, 255)       // Cycle 1: Turn LED on channel 1 ON (100% duty cycle)
    .delay(50)            // Wait for 50 CPU ticks (approx 500ms)
    .setPwm(1, 0)         // Turn LED OFF
    .delay(50)            // Wait 50 ticks
    
    .setPwm(1, 255)       // Cycle 2: ON
    .delay(50)
    .setPwm(1, 0)         // OFF
    .delay(50)
    
    .setPwm(1, 255)       // Cycle 3: ON
    .delay(50)
    .setPwm(1, 0)         // OFF
    .delay(50)
    
    .assertOrYield(5, 1); // Safety check: Assert GPIO pin 5 must be HIGH (1) to yield safely
                          // 安全檢測：斷言 GPIO 腳位 5 必須為高電平，否則掛起虛擬機
});

console.log(`✨ VmScript Bytecode compiled successfully!`);
console.log(`Size: ${blinkBytecode.length} bytes`);
console.log(`Hex: ${Array.from(blinkBytecode).map(b => b.toString(16).padStart(2, "0")).join("")}`);

// 3. Forward this bytecode to ServerGo / hardware Serial gateway
// 隨後，您可以將此字節碼直接發送給 ServerGo 或物理閘道器進行 ESP-NOW 廣播
// Example: redisClient.set("vm:broadcast", blinkBytecode);
```

---

### 🌀 Example 2: Real-time Active Vibration Damping via high-speed FFT
#### 範例二：利用極速傅立葉轉換（FFT）進行即時主動防震抑振

This example demonstrates high-frequency IMU vibration sensor buffering, zero-copy FFT analysis in Rust, and dynamic anti-vibration scripts injection.
此範例展示如何讀取高頻 IMU 震動傳感器數據，透過 Rust 進行零拷貝 FFT 分析，並動態注入反向抑振字節碼。

```typescript
import { TinyNode } from "./index";
import { fft } from "./dsp";

const bav = new TinyNode(0x01);

// Mocking high-frequency 1000Hz sensory buffer (Length must be a power of 2, e.g., 256)
// 模擬一個 1000Hz 採樣率的高頻傳感器緩衝區 (長度必須為 2 的冪次方，例如 256)
const imuBuffer = new Float32Array(256);
const sensorSampleRate = 1000; // 1000 Hz

// Fill mock buffer with a 75Hz physical resonance vibration + 50Hz background motor noise
// 填入模擬物理信號：一個 75Hz 的物理共振震動 + 50Hz 的馬達背景噪音
for (let i = 0; i < imuBuffer.length; i++) {
  const t = i / sensorSampleRate;
  imuBuffer[i] = Math.sin(2 * Math.PI * 75 * t) + 0.5 * Math.sin(2 * Math.PI * 50 * t);
}

console.log("🌀 Analyzing high-frequency physical vibration IMU data stream...");

// 1. Trigger the zero-allocation Rust FFT (under 10 microseconds, zero JS GC overhead!)
// 觸發零分配 Rust FFT 運算 (10微秒內完成，JS 主執行緒完全零 GC 開銷！)
const spectrum = fft(imuBuffer, sensorSampleRate);
const peakFrequency = spectrum.getPeakFrequency();

console.log(`✓ FFT Analysis complete! Dominant Resonance Frequency: ${peakFrequency.toFixed(2)} Hz`);

// 2. Active feedback control loop: Inject anti-vibration bytecode if resonance exceeds safety limits
// 主動反饋控制迴圈：若共振頻率超過安全閾值，動態注入反向阻尼抑振字節碼
if (peakFrequency > 60) {
  console.log(`⚠️ Danger! High Resonance detected at ${peakFrequency.toFixed(2)} Hz. Injecting damping script...`);
  
  const dampingScript = bav.buildScript((builder) => {
    // Dynamically adjust PWM frequency and apply active reverse force on channel 2
    // 動態調節 PWM 頻率，對通道 2 馬達施加反向抑振阻力
    builder
      .setPwm(2, 180) 
      .delay(10)
      .setPwm(2, 0);
  });
  
  // Forward damping script via broadcast instantly
  // 瞬間透過序列網關發射抑振指令流
  console.log(`✓ Dynamic Damping Script compiled: ${dampingScript.length} bytes.`);
}
```

---

## 🔒 Memory Safety & Zero-Copy Architecture / 記憶體安全與零拷貝設計

### English
*   **Zero GC Overhead**: The `Float32Array` buffer allocated in JS is directly passed as a raw C float pointer (`ptr(rawBuffer)`) to Rust. Rust performs inplace spectrum calculations, completely avoiding copying raw arrays across the language boundary.
*   **Leak-Free Allocation**: The serialized byte array generated in Rust is fetched in JS using `Bun.ArrayBuffer.fromAddress` to compile a managed `Uint8Array`. The raw pointer is then immediately deallocated on the Rust heap via `free_serialized_bytes` to guarantee perfect memory cleanup.

### 繁體中文
*   **零 GC 開銷**：在 JS 中分配的 `Float32Array` 會透過 `ptr(rawBuffer)` 將 C 浮點數指標直接傳遞給 Rust。Rust 在底層直通記憶體進行頻譜運算，完全避開了跨語言邊界的數據拷貝開銷。
*   **完美記憶體回收**：在 Rust 堆中生成的 `VmScript` 序列化字節碼，在 JS 端使用 `Bun.ArrayBuffer.fromAddress` 完成受控拷貝後，會立刻透過 `free_serialized_bytes` 觸發 Rust 堆資源釋放，確保系統在 24/7 高頻運行下擁有完美的安全防線。
