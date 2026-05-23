關於 FFI 記憶體安全（FFI Hygiene）的微調建議

既然您已經實作了記憶體安全釋放 API，這套系統在安全性上已經非常穩固。這裡提供一個關於 Bun GC 與 Rust 交互 的微小提醒：

    同步呼叫的天然安全：

        由於 Bun 的 FFI 呼叫預設是同步（Synchronous）且單執行緒的，當 Bun 傳入 Float32Array 的指針給 Rust 執行 FFT 時，JS 的垃圾回收器（GC）會處於暫停狀態，絕對不會在 Rust 執行期間將該陣列釋放或移動。這在物理上保證了指針的安全性。

    ScriptBuilder 的生命週期管理：

        確保在 bun/index.ts 的 ScriptBuilder 類別中實作了 Symbol.dispose（TS 5.2+ 的 Explicit Resource Management）或簡單的 destroy() 方法，在 JS 對象被銷毀時，主動調用 Rust 端的 free_script_builder，防止 Rust 堆積記憶體洩漏：
    code TypeScript

    // TS 5.2+ 顯式資源釋放範例
    class ScriptBuilder implements Disposable {
        private ptr: number;
        // ...
        [Symbol.dispose]() {
            lib.symbols.free_script_builder(self.ptr);
        }
    }
