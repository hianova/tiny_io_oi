# 智慧地釘路演展示專案 (Smart Geopin Roadshow)

本資料夾包含了為「智慧地釘」專案量身打造的路演友善介面與展示流程。透過修改韌體特徵並配合高質感的 Web 儀表板，您可以輕鬆向客戶或投資人展示硬即時地釘系統的預警能力。

## 🎯 展示情境
1. **平靜期**：所有地釘部署完成，處於低功耗監聽狀態。
2. **手動觸發**：路演過程中，按下實體地釘 (ESP32-C6) 上的 `BOOT` 按鈕 (GPIO 9)。
3. **警報觸發**：韌體開始模擬 `20Hz` 的深層剪切波（土石流前兆）。
4. **介面連動**：Web 儀表板會即時捕捉到異常狀態並呈現紅色警告特效，同時展示多節點共識的成功與 `0xFE` 救援廣播。

---

## 🛠 操作流程 (Step-by-Step)

### Step 1: 燒錄帶有 Roadshow 特徵的韌體
我們在 `tiny_io_oi` 韌體中新增了 `roadshow` feature。請進入韌體目錄進行編譯與燒錄：

```bash
cd /Users/hianova/Documents/tiny_io_oi/firmware/esp32

# 燒錄到 Board B (Soldier 地釘節點)
cargo run --features "soldier,roadshow" --release
```

### Step 2: 啟動 ServerGo 閘道伺服器
開啟新的終端機，啟動 `ServerGo` 服務：

```bash
cd /Users/hianova/Documents/ServerGo
cargo run --release -- --config config.yaml
```

### Step 3: 啟動路演友善 Web 介面 (UI)
請進入 `tiny_io_oi` 專案下的 `smart_geopin_roadshow/smart_geopin_ui` 資料夾，使用 Cargo 啟動內建的輕量 Web 伺服器：

```bash
cd /Users/hianova/Documents/tiny_io_oi/smart_geopin_roadshow/smart_geopin_ui
cargo run --release
```

### Step 4: 展演開始
1. 邀請觀眾觀看 Web UI 的地釘狀態面板（顯示連線正常）。
2. 請觀眾親自按下 Board B 上的 `BOOT` 按鈕。
3. 觀察 Web UI 的動畫與警報觸發過程！

---

## 💻 介面技術棧
- **Frontend**: Vanilla JS + HTML + CSS
- **Design Style**: Glassmorphism (玻璃擬物化)、Dark Mode (深色模式)、Micro-animations (微動畫)。
- **Backend Bridge**: 輕量 Rust 伺服器負責將 Web 介面的動作透過 RESP 協定橋接至 ServerGo。
