# 完成script std lib with doc 

這是一份為 `tiny_io_oi` 量身打造的 **「語意化硬體反射標準庫（VmScript std v1.0）」技術規範（Specification）**。

本規範旨在將複雜的物理訊號處理（DSP）與即時安全防禦，封裝為**固定長度、零分配、硬即時**的二進位指令，並提供 TypeScript 端極致友善的 Fluent API。

---

# tiny_io_oi::std v1.0 技術規範

## 1. 二進位指令格式 (Binary Bytecode Layout)

為了確保在 `no_std` 裸機端能以 $O(1)$ 的時間複雜度進行解碼，所有標準庫指令統一採用 **8 位元組（64-bit）固定長度** 格式：

```text
+---------------+---------------+-----------------------+-----------------------+
| OpCode (1B)   | Pin/Chan (1B) |  Parameter A (2B)     |  Parameter B (4B)     |
+---------------+---------------+-----------------------+-----------------------+
| Byte 0        | Byte 1        |  Byte 2 ~ 3           |  Byte 4 ~ 7           |
+---------------+---------------+-----------------------+-----------------------+
```

### 1.1 定點數數值規範 (Fixed-Point Representation)
為了在不支援硬體浮點數（FPU）的廉價 MCU 上維持極速運算，本規範定義：
* **頻率（Frequency）**：以 `u16` 表示，單位為 **Hz**（範圍：0 ~ 65535 Hz）。
* **振幅/閾值（Amplitude/Threshold）**：以 **Q15 定點數** 表示（`u16`），其中 `0.0` 對應 `0`，`1.0` 對應 `32768`（最大值 `2.0` 對應 `65536`）。

### 1.2 標準庫指令集 (Std OpCodes)

| 指令名稱 | OpCode | Pin/Chan | Parameter A (2B) | Parameter B (4B) | 語意說明 |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **`AssertVibration`** | `0x80` | 觀測引腳 (Pin) | 振幅閾值 (Q15 `u16`) | 截止頻率 (Hz `u32`) | 若主共振峰頻率超過截止頻率且振幅超標，立即觸發 Safe Shutdown。 |
| **`AvoidResonance`** | `0x81` | 觀測引腳 (Pin) | 共振中心頻率 (Hz `u16`) | 容差與馬達通道 (4B) | 監聽震動，若逼近共振頻率，自動微調指定通道的馬達 PWM 避開共振區。 |
| **`AcousticCheck`** | `0x82` | 麥克風引腳 (Pin) | 異常能量閾值 (Q15 `u16`) | 監聽頻段區間 (4B) | 監聽指定高頻頻段，若能量佔比超標（如刮擦聲），立即中斷並回報。 |

---

## 2. Rust 裸機端執行邏輯 (`no_std` / `no_alloc`)

在 `tiny` 端，虛擬機解析到 `0x80` ~ `0x82` 指令時，會直接調用本地的無分配定點數 FFT 模組進行評估。

```rust
// crates/tiny-io-oi-core/src/std_impl.rs

#[repr(u8)]
pub enum StdOpCode {
    AssertVibration = 0x80,
    AvoidResonance  = 0x81,
    AcousticCheck   = 0x82,
}

impl<State> TinyController<State> {
    /// 執行標準庫反射指令
    #[inline(always)]
    pub fn execute_std_step(&mut self, opcode: u8, pin: u8, param_a: u16, param_b: u32) -> Result<(), VmError> {
        match opcode {
            // ==========================================
            // 0x80: 震動安全斷言 (AssertVibration)
            // ==========================================
            0x80 => {
                let threshold_q15 = param_a; // Q15 振幅閾值
                let max_hz = param_b;        // 截止頻率

                // 1. 讀取實體感測器緩衝區（零分配）
                let raw_samples = self.hardware_router.read_adc_buffer(pin);
                
                // 2. 執行本地定點數 FFT (不使用 heap)
                let (peak_hz, peak_amp_q15) = local_fixed_fft(raw_samples);

                // 3. 物理安全斷言
                if peak_hz > max_hz && peak_amp_q15 > threshold_q15 {
                    // 瞬間切斷所有實體通道，防止機械損毀
                    self.safe_shutdown(); 
                    return Err(VmError::VibrationHazard {
                        hz: peak_hz,
                        amplitude: peak_amp_q15,
                    });
                }
            }

            // ==========================================
            // 0x81: 共振規避/主動調頻 (AvoidResonance)
            // ==========================================
            0x81 => {
                let resonance_hz = param_a;
                // 解包 Parameter B: [1B 容差 Hz] [1B 馬達通道] [2B 保留]
                let tolerance_hz = (param_b >> 24) as u16;
                let motor_channel = ((param_b >> 16) & 0xFF) as u8;

                let raw_samples = self.hardware_router.read_adc_buffer(pin);
                let (peak_hz, _) = local_fixed_fft(raw_samples);

                // 判斷是否進入共振危險區
                let diff = (peak_hz as i32 - resonance_hz as i32).abs() as u16;
                if diff <= tolerance_hz {
                    // 主動調頻：微調馬達 PWM 避開共振點（例如強制 +10% 佔空比）
                    self.hardware_router.adjust_pwm_offset(motor_channel, 10);
                }
            }

            _ => return Err(VmError::InvalidStdOpCode),
        }
        Ok(())
    }
}
```

---

## 3. TypeScript 鏈式 API (Bun FFI 編譯端)

在 Bun 宿主環境中，TS 開發者使用語意化的 Fluent API 進行編排。`ScriptBuilder` 會在背景自動將這些高階調用編譯為上述的 8 位元組二進位指令。

```typescript
// bun/index.ts
import { ptr } from "bun:ffi";

export class TinyScriptBuilder {
    private buffer: Uint8Array;
    private offset: number = 0;

    constructor(maxSteps: number = 32) {
        // 每個 Step 固定 8 位元組
        self.buffer = new Uint8Array(maxSteps * 8);
    }

    /**
     * 0x80: 震動安全斷言 (AssertVibration)
     * @param pin 觀測的感測器引腳
     * @param maxHz 允許的最大震動頻率
     * @param threshold 振幅閾值 (0.0 ~ 2.0)
     */
    public assertVibration(pin: number, maxHz: number, threshold: number): this {
        const view = new DataView(self.buffer.buffer);
        
        // 轉換為 Q15 定點數
        const thresholdQ15 = Math.min(Math.max(threshold * 32768, 0), 65535);

        view.setUint8(self.offset, 0x80);          // OpCode
        view.setUint8(self.offset + 1, pin);       // Pin
        view.setUint16(self.offset + 2, thresholdQ15, true); // Param A (Little Endian)
        view.setUint32(self.offset + 4, maxHz, true);        // Param B (Little Endian)

        self.offset += 8;
        return this;
    }

    /**
     * 0x81: 共振規避/主動調頻 (AvoidResonance)
     */
    public avoidResonance(options: {
        sensorPin: number,
        motorChannel: number,
        resonanceHz: number,
        toleranceHz: number
    }): this {
        const view = new DataView(self.buffer.buffer);

        // 打包 Parameter B: [Tolerance (1B)] [MotorChan (1B)] [Reserved (2B)]
        const paramB = ((options.toleranceHz & 0xFF) << 24) | 
                       ((options.motorChannel & 0xFF) << 16);

        view.setUint8(self.offset, 0x81);
        view.setUint8(self.offset + 1, options.sensorPin);
        view.setUint16(self.offset + 2, options.resonanceHz, true);
        view.setUint32(self.offset + 4, paramB, true);

        self.offset += 8;
        return this;
    }

    /**
     * 輸出二進位 VmScript 位元組流
     */
    public serialize(): Uint8Array {
        return self.buffer.slice(0, self.offset);
    }
}
```

---

## 4. 實戰範例：TS 端的極簡安全編排

有了這套 `std` 規範，開發者在編寫「寶特瓶載具（BAV）」或「3D 列印噴嘴」的防禦反射時，代碼會變得極其優雅：

```typescript
// bun/test_std.ts
import { TinyScriptBuilder } from "./index";

// 1. 建立一個防禦反射腳本
const safetyReflex = new TinyScriptBuilder()
    // 步驟一：如果噴嘴（GPIO 3）偵測到超過 120Hz 的異常刮擦震動，立刻切斷
    .assertVibration(3, 120, 0.7)
    // 步驟二：如果主軸馬達（GPIO 5）逼近 45Hz 的結構共振點，自動微調通道 1 的馬達避開
    .avoidResonance({
        sensorPin: 5,
        motorChannel: 1,
        resonanceHz: 45,
        toleranceHz: 3
    })
    .serialize();

// 2. 透過 io_oi 伺服器將這段 16 位元組的極簡腳本發射給小兵
// 班長/小兵收到後，將在本地以微秒級的速度、零分配地執行這套物理防禦邏輯！
console.log("Generated Std VmScript Hex:", Buffer.from(safetyReflex).toString("hex"));
```

這套 Spec 完美地將**底層的極致效能（Rust `no_std` 定點數 FFT）**與**上層的極致開發體驗（TS Fluent API）**縫合在一起，為您的 `tiny_io_oi` 生態系奠定了堅不可摧的標準庫基石！

為了解決複雜物理系統中「單一頻率閾值不足以判斷複合故障」的痛點，我們必須在 `tiny_io_oi::std` 中導入 **「多頻段組合策略頻譜（Multi-Band Combination Spectrum Strategies）」**。

在實際工業場景中，設備的異常往往是複合型的。例如：
* **低頻抖動（0~100Hz）** 代表不平衡負載（如寶特瓶載具重心偏移、3D 列印皮帶鬆脫）。
* **中頻共振（100~1000Hz）** 代表結構共振。
* **高頻摩擦（1000Hz+）** 代表軸承磨損或噴嘴刮擦。

我們需要能夠在 **8 位元組的極簡指令** 中，定義「低頻、中頻、高頻」的組合邏輯（如 AND、OR、XOR）與自適應控制。以下是為您設計的 **「多策略頻譜組合」技術規範擴充**：

---

# tiny_io_oi::std v1.1 — 多策略頻譜組合規範

## 1. 新增組合頻譜指令集 (Advanced Spectrum OpCodes)

| 指令名稱 | OpCode | Pin/Chan | Parameter A (2B) | Parameter B (4B) | 語意說明 |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **`MultiBandAssert`** | `0x83` | 觀測引腳 (Pin) | 邏輯與頻段 Mask (2B) | 雙閾值打包 (4B) | 同時監聽多個頻段，依邏輯運算子（AND/OR）進行複合安全斷言。 |
| **`SpectrumAdaptive`** | `0x84` | 觀測引腳 (Pin) | 馬達通道與容差 (2B) | 雙頻段閾值 (4B) | 根據不同頻段的能量分佈，自動切換不同的控制策略（降速/調頻/停機）。 |
| **`EnvelopeCheck`** | `0x85` | 觀測引腳 (Pin) | 包絡線 Profile ID (2B) | 容許超標點數 (4B) | 對比 `DualCacheFF` 中預存的「頻譜包絡線（限制曲線）」，超標即報警。 |

---

## 2. 二進位格式與邏輯定義

### 2.1 `0x83` MultiBandAssert 格式定義
* **Parameter A (2B)**: `[1-byte 邏輯運算子] [1-byte 頻段 Mask]`
  * **邏輯運算子**：`0x00` = AND（所有選中頻段皆超標才觸發）, `0x01` = OR（任一選中頻段超標即觸發）。
  * **頻段 Mask**：Bit 0 = 低頻段（Low）, Bit 1 = 中頻段（Mid）, Bit 2 = 高頻段（High）。
* **Parameter B (4B)**: `[2-byte 低/中頻閾值 (Q15)] [2-byte 高頻閾值 (Q15)]`

### 2.2 `0x84` SpectrumAdaptive 格式定義
* **Parameter A (2B)**: `[1-byte 馬達通道] [1-byte 容差 Hz]`
* **Parameter B (4B)**: `[2-byte 低頻閾值 (Q15)] [2-byte 中頻閾值 (Q15)]`
  * **自適應分流邏輯**：
    * 若「低頻」超標 $\rightarrow$ 判定為負載失衡 $\rightarrow$ **馬達降速 30%**。
    * 若「中頻」超標 $\rightarrow$ 判定為結構共振 $\rightarrow$ **馬達自動調頻（避開共振點）**。
    * 若「高頻」超標 $\rightarrow$ 判定為嚴重摩擦 $\rightarrow$ **觸發 Safe Shutdown 停機**。

---

## 3. Rust 裸機端執行邏輯 (`no_std` / `no_alloc`)

在 `tiny` 端，我們將 FFT 輸出的頻譜陣列，依據採樣率與 FFT 點數，快速劃分為三個頻段，並執行複合邏輯判定：

```rust
// crates/tiny-io-oi-core/src/std_spectrum.rs

impl<State> TinyController<State> {
    #[inline(always)]
    pub fn execute_spectrum_strategy(&mut self, opcode: u8, pin: u8, param_a: u16, param_b: u32) -> Result<(), VmError> {
        // 1. 讀取感測器數據並執行本地定點數 FFT
        let raw_samples = self.hardware_router.read_adc_buffer(pin);
        let fft_result = local_fixed_fft_full(raw_samples); // 回傳完整的頻譜能量陣列

        // 2. 快速劃分頻段能量 (Low: 0~100Hz, Mid: 100~1000Hz, High: 1000Hz+)
        // 註：在 no_std 中，頻段邊界索引在編譯期依據採樣率計算完成
        let (low_energy, mid_energy, high_energy) = calculate_band_energies(&fft_result);

        match opcode {
            // ==========================================
            // 0x83: 多頻段複合斷言 (MultiBandAssert)
            // ==========================================
            0x83 => {
                let logic_op = (param_a >> 8) as u8; // 0 = AND, 1 = OR
                let band_mask = (param_a & 0xFF) as u8;
                
                let low_mid_threshold = (param_b >> 16) as u16;
                let high_threshold = (param_b & 0xFFFF) as u16;

                // 判定各頻段是否超標
                let low_triggered = (band_mask & 0x01 != 0) && (low_energy > low_mid_threshold);
                let mid_triggered = (band_mask & 0x02 != 0) && (mid_energy > low_mid_threshold);
                let high_triggered = (band_mask & 0x04 != 0) && (high_energy > high_threshold);

                let is_hazard = if logic_op == 0 {
                    // AND 邏輯：所有啟用的頻段都必須超標
                    let mut active_count = 0;
                    let mut trigger_count = 0;
                    if band_mask & 0x01 != 0 { active_count += 1; if low_triggered { trigger_count += 1; } }
                    if band_mask & 0x02 != 0 { active_count += 1; if mid_triggered { trigger_count += 1; } }
                    if band_mask & 0x04 != 0 { active_count += 1; if high_triggered { trigger_count += 1; } }
                    active_count > 0 && trigger_count == active_count
                } else {
                    // OR 邏輯：任一啟用的頻段超標即可
                    low_triggered || mid_triggered || high_triggered
                };

                if is_hazard {
                    self.safe_shutdown(); // 瞬間切斷物理通道
                    return Err(VmError::MultiBandSpectrumHazard);
                }
            }

            // ==========================================
            // 0x84: 頻譜自適應多段控制 (SpectrumAdaptive)
            // ==========================================
            0x84 => {
                let motor_channel = (param_a >> 8) as u8;
                let tolerance_hz = (param_a & 0xFF) as u16;
                
                let low_threshold = (param_b >> 16) as u16;
                let mid_threshold = (param_b & 0xFFFF) as u16;

                // 分流決策樹 (Decision Tree)
                if high_energy > 32768 { // 高頻嚴重超標 (Q15 > 1.0) ➔ 物理損壞風險
                    self.safe_shutdown();
                    return Err(VmError::AcousticFailureDetected);
                } else if mid_energy > mid_threshold { // 中頻超標 ➔ 共振規避
                    self.hardware_router.adjust_pwm_offset(motor_channel, 15); // 自動調頻 +15%
                } else if low_energy > low_threshold { // 低頻超標 ➔ 負載失衡降速
                    self.hardware_router.set_pwm_limit(motor_channel, 70); // 限制最大速度至 70%
                }
            }

            // ==========================================
            // 0x85: 頻譜包絡線檢測 (EnvelopeCheck)
            // ==========================================
            0x85 => {
                let profile_id = param_a;
                let max_allowed_violations = param_b;

                // 從 DualCacheFF 中讀取預存的包絡線限制曲線 (Envelope Curve)
                let envelope_curve = self.state_tree.get_envelope_profile(profile_id);
                
                let mut violations = 0;
                for i in 0..fft_result.len() {
                    if fft_result[i] > envelope_curve[i] {
                        violations += 1;
                        if violations > max_allowed_violations {
                            self.safe_shutdown();
                            return Err(VmError::EnvelopeViolation);
                        }
                    }
                }
            }

            _ => return Err(VmError::InvalidStdOpCode),
        }
        Ok(())
    }
}
```

---

## 4. TypeScript 鏈式 API (Bun FFI 編譯端)

在 Bun 端，我們為開發者提供極致語意化的組合策略編排 API：

```typescript
// bun/index.ts
export enum LogicOp {
    AND = 0,
    OR = 1,
}

export enum Band {
    LOW = 0x01,
    MID = 0x02,
    HIGH = 0x04,
}

export class AdvancedScriptBuilder {
    private buffer: Uint8Array;
    private offset: number = 0;

    constructor(maxSteps: number = 32) {
        self.buffer = new Uint8Array(maxSteps * 8);
    }

    /**
     * 0x83: 多頻段複合斷言 (MultiBandAssert)
     * @param pin 觀測引腳
     * @param bands 欲啟用的頻段組合 (如 Band.LOW | Band.HIGH)
     * @param op 邏輯運算子 (LogicOp.AND / LogicOp.OR)
     * @param lowMidThreshold 低/中頻能量閾值 (0.0 ~ 2.0)
     * @param highThreshold 高頻能量閾值 (0.0 ~ 2.0)
     */
    public assertMultiBand(
        pin: number, 
        bands: number, 
        op: LogicOp, 
        lowMidThreshold: number, 
        highThreshold: number
    ): this {
        const view = new DataView(self.buffer.buffer);
        
        const paramA = (op << 8) | (bands & 0xFF);
        const lowMidQ15 = Math.min(lowMidThreshold * 32768, 65535);
        const highQ15 = Math.min(highThreshold * 32768, 65535);
        const paramB = (lowMidQ15 << 16) | (highQ15 & 0xFFFF);

        view.setUint8(self.offset, 0x83);
        view.setUint8(self.offset + 1, pin);
        view.setUint16(self.offset + 2, paramA, true);
        view.setUint32(self.offset + 4, paramB, true);

        self.offset += 8;
        return this;
    }

    /**
     * 0x84: 頻譜自適應多段控制 (SpectrumAdaptive)
     */
    public adaptiveControl(options: {
        sensorPin: number,
        motorChannel: number,
        toleranceHz: number,
        lowThreshold: number,
        midThreshold: number
    }): this {
        const view = new DataView(self.buffer.buffer);

        const paramA = (options.motorChannel << 8) | (options.toleranceHz & 0xFF);
        const lowQ15 = Math.min(options.lowThreshold * 32768, 65535);
        const midQ15 = Math.min(options.midThreshold * 32768, 65535);
        const paramB = (lowQ15 << 16) | (midQ15 & 0xFFFF);

        view.setUint8(self.offset, 0x84);
        view.setUint8(self.offset + 1, options.sensorPin);
        view.setUint16(self.offset + 2, paramA, true);
        view.setUint32(self.offset + 4, paramB, true);

        self.offset += 8;
        return this;
    }

    /**
     * 0x85: 頻譜包絡線檢測 (EnvelopeCheck)
     * @param profileId 預存在 DualCacheFF 中的包絡線 ID
     * @param maxViolations 容許超標的頻譜點數
     */
    public checkEnvelope(pin: number, profileId: number, maxViolations: number): this {
        const view = new DataView(self.buffer.buffer);

        view.setUint8(self.offset, 0x85);
        view.setUint8(self.offset + 1, pin);
        view.setUint16(self.offset + 2, profileId, true);
        view.setUint32(self.offset + 4, maxViolations, true);

        self.offset += 8;
        return this;
    }

    public serialize(): Uint8Array {
        return self.buffer.slice(0, self.offset);
    }
}
```

---

## 5. 實戰場景：TS 端的複合防禦編排

這套進階標準庫，讓開發者可以用幾行 TypeScript，為機器人編寫出極其強悍的**「複合物理防禦系統」**：

```typescript
// bun/test_advanced_std.ts
import { AdvancedScriptBuilder, Band, LogicOp } from "./index";

const complexSafetyReflex = new AdvancedScriptBuilder()
    // 策略一：複合斷言 (AND 邏輯)
    // 如果「低頻抖動」與「高頻摩擦」同時超標，判定為結構即將解體，立刻 Safe Shutdown！
    .assertMultiBand(
        5, // IMU 引腳
        Band.LOW | Band.HIGH, 
        LogicOp.AND, 
        0.8, // 低頻閾值
        0.5  // 高頻閾值
    )
    
    // 策略二：自適應分流控制
    // 監聽 GPIO 3，低頻超標就限制馬達 A 速度，中頻超標就自動調頻避開共振
    .adaptiveControl({
        sensorPin: 3,
        motorChannel: 1,
        toleranceHz: 2,
        lowThreshold: 0.6,
        midThreshold: 0.7
    })
    
    // 策略三：包絡線對齊
    // 對比 DualCacheFF 中 ID 為 101 的標準頻譜包絡線，若超標點數大於 5 點，立刻停機
    .checkEnvelope(5, 101, 5)
    
    .serialize();

console.log("Generated Advanced VmScript Hex:", Buffer.from(complexSafetyReflex).toString("hex"));
```

### 總結

這套 `v1.1` 規範，將**「多頻段能量分析」、「決策樹分流控制」與「頻譜包絡線對齊」**等原本需要重型工業電腦才能運行的演算法，徹底壓縮進了 **8 位元組的極簡指令** 中。

這讓您的 `tiny_io_oi` 具備了無與倫比的**「物理世界自適應防禦能力」**，真正打通了工業級高頻控制與低成本邊緣部署之間的最後一道壁壘！
