# 編譯檢查
一、 為什麼「20 個指令就該往下封裝」是神級規則？
在 tiny_io_oi 的非對稱架構中，這個「20 指令限制」在物理上受到兩個硬性條件的制約：
1. 網路載荷限制（Network Payload Limit）：
    * ESP-NOW 的最大有效載荷（Payload）是 250 位元組。
    * 您的 VmScript 每個指令固定為 8 位元組。
    * 20 個指令 = 160 位元組。這剛好能完美塞進一個 ESP-NOW 封包中，並留有足夠的空間給協議標頭（Header）與校驗和。
    * 如果超過 20 個指令，封包就必須分片（Fragmentation），這會導致丟包率暴增、延遲翻倍。
2. 即時性與燃料限制（Real-time & Fuel Limit）：
    * 20 個指令在 tiny 端執行只需要幾微秒。超過這個數量，就會開始佔用過多的 CPU 週期，甚至觸發 OutOfFuel。
    * 結論：如果一個 TS 腳本編譯出來超過 20 個指令，它在物理上就不再適合當作「即時反射弧」，必須立刻封裝成一個底層的 std OpCode（如 AssertVibration）。

二、 相關係數分析（Correlation Coefficient）：無腦且精準的重構演算法
您提到的「相關係數分析」可以用來做自動化 API 重構，完全不需要 AI：
* 原理：統計所有發送過的 VmScript 歷史紀錄。
* 演算法：計算指令 A（如 ReadAdc）與指令 B（如 Assert）同時出現的共現機率（Co-occurrence Probability）。
* 規則：如果 $P(B|A) > 0.9$（只要出現 A，有 90% 的機率緊接著出現 B），這在數學上強烈證實了 A 與 B 存在極高的耦合度。
* 重構決策：系統自動提示（或在編譯期警告）：「偵測到 ReadAdc 與 Assert 相關係數極高，已達到 20 指令封裝閾值，建議將其合併為單一的 AssertVibration 指令。」

三、 有沒有現成的工具？如何實作？
在 Rust 和嵌入式生態中，確實有一些工具可以幫您做這件事，但最優雅的做法是直接在您的 ScriptBuilder 裡寫一個「零成本 Linter」。
1. 現有的 Rust 底層分析工具
* cargo-show-asm：可以直接列印出 Rust 函數編譯後的實體 ASM 指令行數。您可以寫一個 CI 腳本，如果某個 no_std 函數的 ASM 超過 20 行，直接拒絕 Merge。
* cargo-bloat：用來分析二進位檔中各個函數佔用的實體空間（Bytes）。
2. 最推薦：在 Bun TS 端實作「零 Token Linter」
既然您控制了 ScriptBuilder，您可以在 serialize() 輸出二進位流時，直接加上一個完全免費、執行只需 0.1 毫秒的靜態檢查：
// bun/index.ts
export class AdvancedScriptBuilder {
    // ...
    public serialize(): Uint8Array {
        const stepCount = this.offset / 8;

        // ⚠️ 20 個指令的物理封裝邊界檢查
        if (stepCount > 20) {
            console.warn(
                `\x1b[33m[tiny_io_oi Linter] 警告: 當前腳本包含 ${stepCount} 個指令，已超過 20 個指令的物理安全邊界！\n` +
                `這會增加網路丟包率與執行延遲。請考慮將這些動作封裝為底層的 std OpCode。\x1b[0m`
            );
        }

        return this.buffer.slice(0, this.offset);
    }
}
# 網路安全
一、 防 DDoS 與廣播風暴（Anti-DDoS & Broadcast Storm）
在 ESP-NOW 或 CAN-bus 這種共享介質（Shared Medium）中，最怕某個節點被惡意控制，或者因為硬體故障（如 Bit Flip）陷入無限發包的死迴圈，導致整個通道被塞爆。
1. 拓撲級防禦：被動接收（Passive-Only）與 TDMA 時間片
* 機制：在您的「班長-小兵」拓撲中，嚴格執行「小兵不被問，就閉嘴」的原則。
* 實作：引入 TDMA（時分多路復用）。班長（Leader）為每個小兵分配一個微秒級的專屬發言時間片（Time Slot）。
* 防禦效果：如果某個小兵在非分配時間片內發包，班長與其他小兵在硬體驅動層直接丟棄（Drop）該封包。這能瞬間瓦解來自內部故障節點的洪泛攻擊（Flooding DDoS）。
2. 驅動級限流（Rate Limiting）
* 實作：在 tiny 的接收中斷服務程式（ISR）中，建立一個極簡的靜態計數器。
* 機制：如果來自某個 MAC 位址的封包頻率超過閾值（例如 > 100 packets/sec），直接將該 MAC 加入硬體過濾黑名單（ESP32-C6 的 Wi-Fi 晶片支援硬體 MAC 過濾），不將數據送入 CPU。

二、 防重放與指令鑑權（Anti-Replay & Authentication）
黑客最容易發動的攻擊是：監聽空中訊號，錄下您發送的 VmScript 二進位流（例如「開啟閥門」），並在半小時後重新發射（重放攻擊）。
1. 充分利用 ESP32-C6 的「硬體密碼學加速器」
* 分析：ESP32-C6 晶片內建了 AES-128/256、SHA-256、以及 ECC 的硬體加速器。在 no_std 下調用這些硬體加速，CPU 開銷近乎為零。
* 防禦方案（AES-CCM / ChaCha20-Poly1305）：
    * 班長與小兵之間共享一個對稱金鑰（在創世 Genesis 階段寫入）。
    * 傳輸 VmScript 時，使用 ChaCha20-Poly1305 進行關聯數據加密（AEAD）。
    * 單調遞增序號（Monotonic Nonce）：每個封包帶有一個嚴格遞增的 sequence。小兵收到後，如果 sequence 小於或等於本地記錄的最大值，直接丟棄。這能 100% 免疫重放攻擊。

三、 防惡意腳本注入（Anti-Malicious Injection）
如果黑客破解了通訊，試圖發送一個惡意的 VmScript（例如：讓馬達無限旋轉直到燒毀）。
1. 您的 fuel 機制（已實作，極佳）
* 分析：您的 run_vm_script 內建了 fuel 限制。這能確保任何試圖耗盡 CPU 資源（CPU Exhaustion DDoS）的惡意死迴圈腳本，會在幾微秒內被強制終止。
2. 靜態邊界檢查（Static Boundary Check）
* 實作：在 tiny 執行 VmScript 之前，進行一次 $O(1)$ 的靜態掃描： rust // 驗證腳本中的所有 Target Pin 是否在 HardwareRouter 的授權範圍內 for step in script.steps.iter() { if let ArchivedVmStep::SetPwm { channel } = step { if !self.hardware_router.is_authorized_channel(*channel) { return Err(VmError::UnauthorizedAccess); // 越權直接拒絕 } } }
* 防禦效果：防止黑客透過腳本去操作未授權的敏感引腳（例如自檢引腳或電源控制引腳）。

四、 傳統資安套件 vs. 自研輕量安全套件
安全維度	傳統套件（如 TLS / mTLS）	tiny_io_oi 自研輕量套件
記憶體開銷	❌ 極大 (100KB+ RAM，需要 Heap)	極小 (< 2KB RAM，完全 no_alloc)
延遲（Latency）	❌ 極高 (握手需要 100~300ms)	極低 (硬體 AES 加密 < 5 微秒)
防禦維度	僅防竊聽與篡改，無法防廣播風暴。	結合 TDMA 與硬體過濾，兼防廣播風暴與重放。
# vm script 注入延遲
方向一：微秒級群體時間同步（PTP over ESP-NOW）
* 痛點：當您有 50 台小兵（BAV）或 8 個 3D 列印噴嘴時，雖然您可以用 ESP-NOW 快速發送 VmScript，但因為無線網路的抖動（Jitter），各個節點收到並執行指令的時間會有幾毫秒的落差。這在需要極致同步的場景（如多軸協同加工、無人機陣列、主動協同抑振）中是致命的。
* 突破點：在 tiny_io_oi 的網路層實作一個極輕量化的 PTP（精密時間協議 / IEEE 1588）。
    * 機制：班長（Leader）與小兵（Node）之間定時進行微秒級的時鐘對齊。
    * 效果：您的 VmScript 可以支援「絕對時間戳執行」。例如，伺服器發送：[在絕對時間 T + 5000 微秒時，執行 SetPwm]。
    * 價值：這能讓 50 台設備在完全沒有通訊延遲干擾的情況下，在同一個微秒瞬間做出整齊劃一的物理動作。
# 編譯形式化
VmScript 的形式化驗證（Formal Verification）
* 痛點：在航太、醫療、軍工或重型工業領域，客戶不會因為您說「我的系統很安全」就採用。他們需要數學上的證明。
* 突破點：在 io_oi 伺服器端（Rust）實作一個 靜態安全驗證器（Static Verifier）。
    * 機制：利用符號執行（Symbolic Execution）或抽象詮釋（Abstract Interpretation），在 VmScript 發射出去之前，對其二進位 Bytecode 進行數學證明。
    * 效果：驗證器能自動產出證明報告：「此腳本已通過驗證，在物理上 100% 絕不存取未授權引腳、絕不產生死迴圈、且最大電流消耗在安全範圍內。」
    * 價值：這能讓您的系統直接去申請 ISO 26262（車載安全） 或 DO-178C（航太安全） 認證，直接打入高溢價的軍工與航太市場。
