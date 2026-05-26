// ============================================================
// io_oi 分散式共識協議 — Lean Spec v2
// 模型：簽核不可否認 + 主觀跟隨 + 分叉即退出（Git 式仲裁）
// 非 BFT / 非 CFT / 非 CRDT
// ============================================================

#![allow(dead_code, unused_variables)]

use std::sync::Arc;

// --- 外部依賴佔位（實際引入時替換）---
// use rkyv::{Archive, Deserialize, Serialize};
// use iroh::net::{Endpoint, NodeId};
// use wasmtime::{Engine, Module};
// use arc_swap::ArcSwap;
// use crate::dualcache_ff::DualCacheFF;

// ============================================================
// 0. 全域別名（先用 u8 陣列佔位，後期換 newtype）
// ============================================================

pub type NodeId      = [u8; 32];
pub type Signature   = [u8; 64];
pub type Hash32      = [u8; 32];
pub type EpochId     = u64;
pub type Sequence    = u64;

// ============================================================
// 0.5 專案子目錄與功能 (Project Structure)
// ============================================================
//
// - core:  定義核心資料結構、OpCode 指令集與 DualCacheFF 介面。純粹的邏輯與資料定義，不含 serde、I/O 與網路。
// - node:  實作 Iroh P2P 通訊、RESP 閘道、WASM 載入與 DualCacheFF 節點狀態維護。
// - cli:   提供創世啟動、節點加入與自然語言交互介面。
// - wasm:  定義仲裁合約的 WIT 介面與合約編譯環境。
//
// ============================================================

// ============================================================
// 1. 基礎資料結構
// ============================================================

// 1.1 全域時間線（由 Leader 發布，節點被動接收）
#[derive(Clone, Debug)]
pub struct Epoch {
    pub leader_id : NodeId,
    pub epoch_id  : EpochId,
    pub deadline  : u64,    // UNIX timestamp，epoch 截止時間
}

// 1.2 投票結構
#[derive(Clone, Debug)]
pub struct Vote {
    pub weight      : u64,
    pub record_hash : Hash32,
    pub epoch_id    : EpochId,
    pub sequence    : Sequence,          // 防重放序號
    pub club_id     : Option<Hash32>,    // None → 投給自己
    pub signature   : Signature,
}

// 1.3 應用層貢獻特徵（Record 由各應用自行實作）
pub trait Record: Send + Sync {
    fn hash(&self)     -> Hash32;
    fn epoch(&self)    -> EpochId;
    fn validate(&self) -> bool;
    // TODO: 定義 normalized_score 計算介面
    // fn score(&self) -> f64 { todo!() }
}

// 1.4 經 Leader 簽核的紀錄（網路傳輸單元）
#[derive(Clone, Debug)]
pub struct SignedRecord {
    pub payload         : Vec<u8>,
    pub judge_signature : Signature,     // ed25519
    pub record_type     : u32,
}

// 1.5 複合 Key（含 genesis_hash 做命名空間隔離）
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct ScopedKey {
    pub namespace   : Hash32,   // genesis_hash，創世寫死
    pub record_hash : Hash32,
}

// ============================================================
// 2. 節點結構
// ============================================================

pub struct Node {
    pub node_id      : NodeId,
    // pub current      : ArcSwap<Vec<u8>>,
    // pub state_tree   : DualCacheFF<ScopedKey, SignedRecord>,
    // pub protocol     : Endpoint,
    // pub global_epoch : ArcSwap<Epoch>,
    // pub state        : ArcSwap<NodeState>,
    _marker: std::marker::PhantomData<()>,
}

// ============================================================
// 3. 社會角色狀態機
// ============================================================

pub enum NodeState {
    // ---- 平民（無路由義務）----
    Idle,
    Hibernating,
    Handoff,
    Voting,
    Syncing,

    // ---- 中產（有路由義務，無 WASM 執行權）----
    Chief {
        routing_table: Vec<NodeId>,
    },
    /// 退役 Leader 保留已簽核紀錄副本，WASM 已銷毀
    Manager {
        routing_table    : Vec<NodeId>,
        archived_records : Vec<SignedRecord>,
    },

    // ---- 仲裁者（唯一持有 WASM 執行權）----
    JudgeActive(JudgeState),
}

pub struct JudgeState {
    pub private_key : [u8; 32],
    pub public_key  : [u8; 32],
    // pub wasm_engine : wasmtime::Engine,
    // pub wasm_module : wasmtime::Module,
    pub circle      : Vec<NodeId>,   // 信任圈：現任 Manager + Chief
}

// ============================================================
// 4. 二進制協議指令集
// ============================================================

/// opcode 空間（1 byte）
/// 0x00        保留（心跳應答）
/// 0x01-0x0F   系統級
/// 0x10-0x1F   資料操作
/// 0x20-0x2F   IoT 任務
/// 0x30-0x3F   律法更新
/// 0x40-0x7F   io_oi 保留
/// 0x80-0xFF   應用層自訂

#[repr(u8)]
pub enum OpCode {
    // 系統級
    SyncReq      = 0x01,
    SyncData     = 0x02,
    Vote         = 0x03,
    Heartbeat    = 0x04,
    Redirect     = 0x05,
    NotFound     = 0x06,
    // 資料操作
    Get          = 0x10,
    Put          = 0x11,
    Query        = 0x12,
    // IoT 任務
    TaskDispatch = 0x20,
    TaskAchieved = 0x21,
    TaskFailed   = 0x22,
    // 律法更新
    Promulgate   = 0x30,
}

// ============================================================
// 5. 任務與能力
// ============================================================

pub struct Task {
    pub task_id    : Hash32,
    pub capability : Capability,
    pub priority   : u8,
    pub deadline   : Epoch,
    pub depends_on : Vec<Hash32>,
    pub app_payload: Vec<u8>,   // io_oi 不解析，應用層自行處理
}

pub enum Capability {
    TaskDispatch,   // Leader → 節點：起始點，payload 為空
    Achieved,       // 節點 → Manager：工作量證明
    Jury,           // 開庭：帶衝突的兩個 SignedRecord hash
    Promote,        // 晉升：帶目標 NodeId
}

// ============================================================
// 6. 角色行為契約
// ============================================================

pub struct SyncReq {
    pub hash         : Hash32,
    pub ping_distance: u32,
}

pub enum Response {
    SyncData(SignedRecord),
    Redirect(Vec<NodeId>),
    NotFound,
    /// 狀態差異：回傳起始 Epoch 到目標 Epoch 之間的摘要
    SyncDiff {
        start_epoch: EpochId,
        end_epoch:   EpochId,
        record_hashes: Vec<Hash32>,
    },
}

pub struct NodeContext; // TODO: 補齊上下文欄位

pub trait StateBehavior {
    fn handle_sync_req(&self, req: &SyncReq, ctx: &NodeContext) -> Response;
    fn handle_heartbeat(&self, from: NodeId, load: u8, ctx: &NodeContext);
    fn can_relay(&self)  -> bool;
    fn can_judge(&self)  -> bool;
}

// ============================================================
// 7. 內生系統函式（只能由協議層事件觸發，應用層無直接路徑）
// ============================================================

impl Node {
    /// 7.1 推進 epoch（Leader 發布新 Epoch，廣播全網）
    pub fn advance_epoch(&self, new_epoch: Epoch) {
        // TODO: 驗證 leader 簽名
        // TODO: 更新 global_epoch（ArcSwap）
        // TODO: 廣播 SYNC_REQ 給信任圈
        todo!("advance_epoch")
    }

    /// 7.2 結算 epoch（Event Sourcing：從 DualCacheFF 重放合法 Record）
    pub fn finalize_epoch(&self, epoch_id: EpochId) -> Vec<(NodeId, u64)> {
        // TODO: 從 DualCacheFF 取出 epoch_id 的所有 SignedRecord
        // TODO: WASM 重放計算 normalized_score
        // TODO: 回傳 (node_id, stake_weight) 列表
        todo!("finalize_epoch")
    }

    /// 7.3 選舉（依計分卡結果決定下任 Leader）
    pub fn elect_leader(&self, scores: Vec<(NodeId, u64)>) -> NodeId {
        // TODO: 收集 Vote，依 weight 排序
        // TODO: club_id.is_none() → 投自己；is_some() → 委託 Club
        // TODO: 得票最高者成為下任 Leader
        // TODO: 次高者可晉升為 Manager / Chief
        todo!("elect_leader")
    }

    /// 7.4 衝突仲裁（雙重簽核 = 密碼學自證其罪）
    pub fn handle_conflict(&self, a: &SignedRecord, b: &SignedRecord) {
        // TODO: 驗證兩者確實出自同一 Leader（相同 epoch + 相同簽名公鑰）
        // TODO: 廣播衝突證明給全網
        // TODO: 由下一任 Leader 的 WASM 將作惡者計分卡歸零
        // TODO: Jury Capability：通知陪審節點存檔 tombstone
        todo!("handle_conflict")
    }

    /// 7.5 負載管理（超過 80% 時 Redirect）
    pub fn route_or_redirect(&self, req: &SyncReq) -> Response {
        // TODO: 取得目前負載（從 Heartbeat 維護的 load u8）
        // TODO: load > 0.8 → Response::Redirect(nearest_chiefs)
        // TODO: 否則查詢 state_tree.get(req.hash)
        todo!("route_or_redirect")
    }

    /// 7.6 狀態修剪
    pub fn prune(&self, current_epoch: EpochId, k: u64) {
        // 記憶體：保留最近 K 個 epoch，更舊以機率 p = max(0.01, 1 - d/K) 保留
        // 硬碟：所有歷史 SignedRecord 冷存完整保留，供審計或復原
        // TODO: 計算每筆 SignedRecord 的 epoch 距離 d
        // TODO: 依機率決定是否驅逐至硬碟
        // TODO: 硬碟封存（append-only，不可刪除）
        todo!("prune")
    }

    /// 7.7 Record 提交管線（節點 → Manager → Leader）
    pub fn submit_record<R: Record>(&self, record: R) {
        // TODO: record.validate() → 失敗則靜默丟棄
        // TODO: 本地簽名後送交所屬 Manager（PUT 0x11）
        // TODO: Manager 批次合併為 MergedRecord 送交 Leader
        // TODO: Leader WASM 驗證 → 簽核 → SignedRecord 廣播全網
        todo!("submit_record")
    }

    /// 7.8 新節點加入流程
    pub fn join(&self, entry_point: NodeId) {
        // 1. 連接 Chief，取得活躍 Manager 清單與 ping 延遲
        // 2. 選擇最近 Manager 歸屬，同步 SignedRecord
        // 3. 初始 weight = 0，等待 Manager 背書 InviteRecord 後才能投票
        // TODO: 實作 ping 測距與拓樸局部抽樣（防日蝕攻擊自檢）
        // TODO: Manager 背書邏輯（邀請制或 POW 首個 Record）
        todo!("join")
    }

    /// 7.9 Leader 斷線處理
    pub fn handle_leader_offline(&self) {
        // 不急著立即改選，record 繼續持有視為「繼續任期」
        // 等到 init wasm duration 到期時，順帶進行遴選，簡化選舉分支
        // TODO: 監聽 Heartbeat 超時（load 欄位長時間沉默）
        // TODO: 觸發 elect_leader()
        todo!("handle_leader_offline")
    }

    /// 7.10 重放攻擊防禦（sequence + epoch_id 聯合驗證）
    pub fn check_replay(&self, vote: &Vote) -> bool {
        // TODO: 查 DualCacheFF 確認 (node_id, epoch_id, sequence) 未出現過
        // TODO: 可附帶 ping ms 進 epoch hash，反向驗證距離合理性
        todo!("check_replay")
    }

    /// 7.11 WAL 防呆（防止 Leader 崩潰重啟意外 Double Sign）
    pub fn wal_before_sign(&self, record: &SignedRecord) {
        // TODO: 寫入 WAL，確認 fsync 成功後才執行簽核
        // TODO: 啟動時先掃 WAL，有未完成簽核則恢復或放棄
        todo!("wal_before_sign")
    }
}

// ============================================================
// 8. Club 機制（後期 stake 過度集中才啟用）
// ============================================================

pub struct Club {
    pub club_id  : Hash32,
    pub stake    : u64,     // 聚攏的總票數（Vote，非 Record 工作量）
    pub ideology : Vec<u8>, // 意識形態宣言（應用層自訂）
}

impl Club {
    /// 聚攏成員 Vote，統一投給指定候選人
    pub fn cast_votes(&self, candidate: NodeId) -> Vec<Vote> {
        // TODO: 驗證成員 Vote 的 club_id 確實指向 self.club_id
        // TODO: 彙整後簽核送出
        // TODO: 平局判定：先到者優先（timestamp 由 Leader 發布，不存在信任問題）
        todo!("cast_votes")
    }
}

// ============================================================
// 9. 授權與 WASM 載入
// ============================================================

pub struct WasmLoader;

impl WasmLoader {
    /// CLI 打包時嵌入授權金鑰 sha256，WASM 讀取後決定是否執行簽核
    pub fn load_and_verify(wasm_bytes: &[u8], license_key: Hash32) -> Result<(), String> {
        // TODO: sha256(wasm_bytes) 與 license_key 比對
        // TODO: config.cranelift_nan_canonicalization(true) 確保 float hash 確定性
        // TODO: 失敗 → 節點只能以 Idle 啟動，無法競選 Leader
        todo!("load_and_verify")
    }

    /// WASM 升級：等同開新主幹，舊社群不強制遷移
    pub fn migrate_to_new_trunk(new_wasm: &[u8]) {
        // TODO: 廣播遷移邀請（非強制）
        // TODO: 舊 WASM 停止激活，保留簽核紀錄副本
        // TODO: 信任圈成員自行決定是否跟隨新主幹
        todo!("migrate_to_new_trunk")
    }
}

// ============================================================
// 10. 創世（Genesis）
// ============================================================

pub struct GenesisConfig {
    pub namespace    : Hash32,    // 寫死，全網唯一
    pub founder_id   : NodeId,
    pub initial_stake: u64,       // 創始節點 = 100%，後續靠 Record 稀釋
    pub epoch_duration: u64,      // ms
    // pub preset     : serde_json::Value,  // 方便 Leader 掛掉後重新投票生成
}

pub fn genesis(cfg: GenesisConfig) -> Node {
    // TODO: cli tool（clap）解析參數，或接 LLM 自然語言介面
    // TODO: 建立 mono node，自我 = 100% stake，無背信空間
    // TODO: 簽發第一個 Epoch，啟動 epoch 計時
    todo!("genesis")
}

// ============================================================
// 11. 威脅模型備忘（非程式碼，僅供實作參考）
// ============================================================
//
// | 攻擊               | 防禦                                              |
// |--------------------|---------------------------------------------------|
// | 女巫攻擊           | 初始 weight = 0，需邀請背書；計分卡無法憑空生成   |
// | 重放攻擊           | (node_id, epoch_id, sequence) 聯合唯一             |
// | 日蝕攻擊           | 節點定期向 Leader 直接請求最新 epoch 自檢         |
// | 惡意 Leader        | 雙重簽核立即削權；WASM 不可修改                   |
// | 資產雙花           | 協議層不管資產，應用層自行確保最終性              |
// | WASM 盜用          | 授權金鑰嵌入 CLI；潛規則 + 社群信任（非技術封閉） |
//
// ============================================================
// ============================================================
// 12. 狀態同步協議 (State Synchronization)
// ============================================================
//
// 節點加入或恢復連線時，必須對齊全網最新的 SignedRecord 主幹。
//
// 12.1 同步策略
// - 增量同步 (Incremental): 優先同步最近的 K 個 Epoch。
// - 摘要對齊 (Snapshot/Diff): 使用 Merkle Root 或 Hash 列表快速定位缺失資料。
// - 驗證優先: 每一筆同步進來的 SignedRecord 必須經過現任 Leader/Judge 的公鑰驗證。
//
// 12.2 同步流程 (Stage Machine)
// 1. [Discover]: 向 Seed Node 或 Chief 請求 Manager 列表。
// 2. [Handshake]: 發送 SyncReq { hash: genesis_hash, ... }，取得目標節點的 Epoch 高度。
// 3. [Diff]: 請求 SyncDiff，取得自己缺失的 Record Hash 列表。
// 4. [Fetch]: 批次發送 SyncReq { hash: record_hash } 取得完整的 SignedRecord。
// 5. [Commit]: 驗簽成功後寫入 DualCacheFF，更新本地 Epoch 指標。

// ============================================================
// 13. 測試設計 (Test Design for All Paths)
// ============================================================
//
// 為確保系統達到「兆元級別」穩定性，必須覆蓋以下路徑：
//
// 13.1 核心路徑 (Happy Path - P0)
// - [T-P0-1] 單節點創世與 Epoch 推進。
// - [T-P0-2] 多節點 (3+) 共識循環：提交 -> 仲裁 -> 簽核 -> 同步。
// - [T-P0-3] Leader 正常換屆：舊 Leader 卸任 -> 新 Leader 獲選 -> 狀態接管。
//
// 13.2 安全路徑 (Security Path - P1)
// - [T-P1-1] 惡意 rkyv Payload：餵入畸形、超長或循環引用的位元組，驗證 check_archived_root 攔截率。
// - [T-P1-2] WASM 資源耗盡：上傳無限迴圈或內存溢出的 Arbitrator，驗證 Fuel 與 Memory 限制。
// - [T-P1-3] 雙重簽核 (Double Sign)：模擬 Leader 對同一 Epoch 簽發不同內容，驗證削權邏輯。
// - [T-P1-4] 重放攻擊 (Replay)：重複發送舊 Vote，驗證 sequence 檢查。
//
// 13.3 異常與混沌路徑 (Chaos Path - P2)
// - [T-P2-1] 網路分割 (Partition)：模擬 50% 節點斷開，恢復後驗證主幹收斂。
// - [T-P2-2] 負載重定向：模擬節點 CPU/Mem 飽和，驗證 0x05 REDIRECT 是否生效。
// - [T-P2-3] 磁碟損壞恢復：模擬 WAL 損壞，驗證系統能否從鄰居節點重新同步 (Resync)。
//
// 13.4 效能路徑 (Performance Path - P3)
// - [T-P3-1] 萬級 TPS 壓力測試：大量小 Record 提交，觀察 DualCacheFF 吞吐量。
// - [T-P3-2] WASM 冷啟動延遲：連續執行 1000 次仲裁，計算 Instance Pooling 優化效果。
//
// ============================================================
// 14. 網路治理模式 (Governance Modes)
// ============================================================
//
// 14.1 信任模式 (Trust Mode)
// - Full: 節點完全信任廣播，所有接收到的合法 Record 都會轉發給所有已知 Peer。
// - Localized: 局部性廣播。Record 僅在局部鄰里（隨機 3 個節點）內流動，需透過主管節點 (Chief/Manager) 進行跨區晉升。
//
// 14.2 管制模式 (Control Mode)
// - Strict: 由創世 Leader 完全管制。註冊新節點需經 Leader 授權，且不進行全網競選。
// - Competitive: 競爭性共識。普通節點只要 Stake 足夠且具備 WASM 能力，即可參與 Leader 競選。
//
// ============================================================
// 15. 生命週期與優雅關閉規範 (Lifecycle & Graceful Shutdown)
// ============================================================
//
// 為了在測試、節點重啟或崩潰復原時實現完美的資源清理、避免執行緒洩漏與磁碟資料毀損：
//
// 15.1 背景任務生命週期
// - WAL Worker: 在收到全域關閉通知 (shutdown_tx) 時，必須立刻關閉接收通道 (rx.close())，
//   將殘留在緩衝區與通道中的所有紀錄全部同步寫入磁碟 (fsync / file.sync_all())，隨後安全退出。
// - Serial Driver: 在串口連接或重連的任意階段，都必須註冊並檢測取消信號，在關閉時立即釋放串口驅動並退出迴圈。
// - P2P Broadcast Connect: P2P 網路連接具備微秒級的 select 偵聽，防止在關閉時產生背景懸掛 (orphan) 連接任務。
// - TCP RESP Gateway: 提供 Gateway 獨立的優雅關閉 API，關閉時除了立即停止 accept 新連線外，
//   必須同時向所有活躍中的客戶端連線發送退出取消信號，強制優雅中斷，防止 TCP 端口與 socket 持續佔用。
//
// ============================================================
// 16. 封裝訊號與直通硬體路由 (Waveform & HardwareRouter)
// ============================================================
//
// 為在邊緣端 (no_std) 實現零開銷、零記憶體碎片的物理訊號控制，導入以下機制：
// - Waveform 協定：在 L1 協定中定義標準電氣波形 Enum (DigitalOut, Pwm8Bit, AnalogOut, ServoAngle)，支援 rkyv 序列化。
// - HardwareRouter 路由表：零動態分配的固定大小路由矩陣。透過 bind_digital 與 bind_pwm 動態將協定通道綁定到實體 GPIO/PWM 驅動上。
// - 零拷貝轉譯：透過 apply_waveforms 直接以 zero-copy 方式解析 WaveformMatrix 並將其導向已綁定之實體引腳，消除邊緣端業務邏輯解析開銷。
//
// ============================================================
// 17. 客戶端自癒路由 (Client-side Failover via Heartbeat Decay)
// ============================================================
//
// 為了解決分散式車隊/群體中的「狀態孤兒」與單點故障問題：
// - 心跳計時衰減：TinyNode 的 tick 流程會以極低 CPU 週期開銷對 scores 中的 Manager 心跳分數進行 saturating 遞減。
// - 斷線自癒觸發：若在設定時間內（例如 100 次 tick）未收到心跳，該 Manager 評分歸零並被移除。若 get_leader() 為 None，自動觸發自癒尋主廣播。
// - 尋主 Exception 廣播：發送帶有 0xFE 自癒代碼的 Exception 封包，主動請求鄰近小主管接管，達到 P2P 網路的反脆弱性。
//

// ============================================================
// 18. 寫前日誌與狀態重啟自癒 (Write-Ahead Logging & State Recovery)
// ============================================================
//
// 為確保 no_std 裸機與嵌入式環境在突發斷電時的資料完整性與狀態恢復：
// - 虛擬 FlashFileSystem 抽象：實作對接 `cdDB::FileSystem` trait 的模擬 Raw Flash FS。
// - 寫前日誌同步：在 `OpCode::StateUpdate` 發生的第一時間，先將狀態變化編碼為 `cdDB::WriteCommand::Insert` 寫入日誌，隨後套用 Delta 遮罩。
// - 二進位 WAL 重放：系統重啟時調用 `recover_from_wal`，逐條解碼日誌序列，以與歷史完全相同的順序重放 RCU Delta，使狀態原樣恢復。

// ============================================================
// 19. 多路 PWM 路由與巨集代碼生成 (Multi-Channel PWM Routing & Macro CodeGen)
// ============================================================
//
// 支援在嵌入式端更為彈性的多通道硬體驅動綁定：
// - `#[io_oi_node]` 欄位反射：巨集自動掃描所有被標註 `#[bind(channel = X, strategy = "PWM")]` 的欄位。
// - 多通道指令分派：`VmStep::SetPwm` 擴充 `channel` 欄位，巨集會為所有綁定的實體/模擬 PWM 欄位生成 channel-matching 的 `match` 分支，實現單個指令流驅動多個馬達的獨立控制。
//
// ============================================================
// 20. 安全關閉鉤子與斷言異常攔截 (Safe Shutdown Hook & Assertion Exception Traps)
// ============================================================
//
// 確保邊緣控制端在遭遇到 VM 物理環境斷言失敗或燃料耗盡（Out of Fuel）等軟體異常時，不會因馬達維持舊速度而失控：
// - 自動生成清理區：`#[io_oi_node]` 巨集在編譯時，會為所有綁定的 PWM/GPIO 引腳生成 `Safe Shutdown` 程式區塊，將其全數強制設定/重設為零。
// - 異常回呼與 Trap 攔截：在 `run_vm_script` 遭遇燃料耗盡或 AssertOrYield 斷言失敗返回 `Err` 之前，巨集會自動插入並調用 `Safe Shutdown` 鉤子，隨後再向協定層廣播 `OpCode::Exception` 封包，保證物理硬體本體絕對安全。
//
// ============================================================
// 21. 主動防禦安全模式與雙重簽核削權 (Failover Safe Mode & Double-Sign Demotion)
// ============================================================
//
// 針對網路不穩定或惡意 Leader 削權、分叉進行主動式硬體反射安全防禦：
// - 主觀心跳衰退防禦：當本地 Tick 迴圈偵測到 Leader 的心跳分數降至零（即 `leader_active` 為真且 `get_leader()` 變為 `None`）時，節點立刻切換至 `Safe Mode`。
// - 雙重簽核（Double Sign）主動偵測：當同一個 Epoch 內同一個 Leader 送出不相符的重複狀態更新/指令時，立刻判斷為雙分叉衝突，將該 Leader 記為 `disqualified_leader` 並寫入 WAL/Jury 衝突日誌中（寫入 Entity 999 專用區），並使節點進入 `Safe Mode`。
// - 安全隔離行為：進入 `Safe Mode` 後，節點將馬達 PWM 強制歸零、GPIO 輸出歸零，並拒絕任何來自失效/衝突 Leader 的 `TaskDispatch`, `StateUpdate`, `VmScriptDispatch` 指令。
// - 網路自癒康復：當此節點在網路中接收到其他合法的、且未被 disqualified 的新 Leader 之 Heartbeat 時，它會重新計算並切換回正常操作模式。

// ============================================================
// 22. 精密時間同步協議與絕對時間戳執行 (PTP Clock Synchronization & Absolute Time Execution)
// ============================================================
//
// 針對多設備協同物理動作時因網路傳輸抖動（Jitter）造成的時序不同步問題：
// - 精密時間對齊（PTP over ESP-NOW）： Leader 與 Node 之間定時進行微秒級的時鐘同步對齊，利用 IEEE 1588 交換機制計算時鐘偏移量（Offset）與傳輸延遲（Delay）。
// - 絕對時間執行：透過 VmScript 的 standard library 擴充 OpCode 0x86 (DelayUntil)，其參數包含 48-bit 微秒級絕對時間戳。Node 在執行到此指令時，會自動延遲等待至本地 synchronized 系統時間到達該絕對時間戳，以實現在完全沒有通訊抖動干擾的情況下多設備整齊劃一的物理動作。
// - 輕量條件編譯：PTP 時間管理程式與絕對延遲操作封裝在 `ptp` feature 內。在嵌入式裸機端（`no_std`）若未開啟 `ptp` 特徵，該指令自動退化並編譯出，從而保障最小韌體足跡。
//
// ============================================================
// 23. VmScript 二進位形式化驗證引擎 (VmScript Formal Verification Engine)
// ============================================================
//
// 為了在腳本發射到邊緣節點執行前保證 100% 物理與邏輯安全，在主控主機/伺服器端提供 StaticVerifier：
// - 數學停機證明（Termination Proof）：形式化分析 VmScript / std 執行序列，利用無迴圈/無動態跳轉之拓撲性質，數學上嚴格證明任意給定 Fuel 的腳本均能在有限步驟內完成，從而 100% 杜絕 OutOfFuel 或死鎖陷阱。
// - 邊界路由安全證明（Boundary Safety Proof）：逐條指令掃描引腳（Pin < 32）和通道（Channel < 8）匹配授權規則，數學上證明 100% 絕不存取未經 HardwareRouter 授權的敏感資源。
// - 電氣電流保護證明（Current Draw Safety Proof）：透過對 PWM 脈寬調製（SetPwm, AvoidResonance, SpectrumAdaptive）所代表的電機電流消耗進行累加與峰值仿真，證明最大電流指標絕不超出硬件熱極限與配電保險上限。
// - 條件編譯封離：此龐大數學驗證器與報告生成模組完全基於 `verifier` 條件編譯特徵，絕不併入裸機韌體，只在伺服器端與上位機編譯，達成零 MCU 開銷。
//
// ============================================================
// 24. 空間共識協定防物理作弊 (Spatial Consensus Protocol & Anti-Spoofing)
// ============================================================
//
// 針對物理感測器可能遭受之惡意局部物理干擾（例如將高頻振動馬達貼在智慧地釘上模擬土石流前兆）進行空間維度共識防禦：
// - 局部單點不可信：單一智慧地釘地釘偵測到山體滑坡或異常波形時，不予直接觸發理賠以防止詐保。
// - 分散式 Gossip 網路宣告：當本地 FFT 頻譜計算之低/中頻震動能量超標時，節點透過 ESP-NOW Gossip 網路以 `OpCode::SpatialGossip` (0x05) 廣播其 MAC、時間戳與局部危險評分。
// - 鄰近多點關聯斷言（Spatial Consensus Assert）： 透過標準庫指令 `0x87`，僅當節點在指定的時間窗口 `TimeWindow_ms` 內，同時收到來自至少 `K` 個相鄰 unique 智慧地釘節點（其 hazard_score 均高於 `HighThreshold`）的相符宣告時，才判定為真實物理滑坡災害，並立刻進行 Safe Shutdown 與廣播 `OpCode::Exception` 以向 Oracle 觸發自動理賠。
// - 時間主動剪枝：`TinyNode` 在 tick() 中會自動對 Gossip 快取中超過 5 秒 (`5,000,000` 微秒) 的過期物理斷言進行主動剪枝與清除，以保證關聯性的微秒級時間精度。

// ============================================================
// 25. 標準庫指令集 (Standard Library Opcodes)
// ============================================================
//
// VmScript 除了基礎控制流外，更提供一系列高度優化的標準庫指令，用於邊緣運算與物理斷言：
//
// - AssertVibration (0x80): 擷取指定窗口的感測器資料，計算 FFT 頻譜並斷言高頻振動能量是否低於閾值。若不符則觸發 Exception。
// - AvoidResonance (0x81): 當偵測到機械共振頻率時，主動改變馬達 PWM 輸出頻率或避開特定頻段，以防硬體損壞。
// - SpectrumAdaptive (0x82): 基於持續的頻譜分析，自適應地調整控制參數（如 PID 常數或濾波器截止頻率）。
// - MultiBandAssert (0x83): 針對多個不同的頻段（如低頻滑坡特徵與高頻機具特徵）同時進行斷言，為「智慧地釘」災害防護提供精確判定。
// - DelayUntil (0x86): 基於 PTP 微秒時鐘的絕對時間延遲，確保多設備的協同動作在同一微秒級瞬間精確觸發。
// - SpatialConsensusAssert (0x87): 聚合周圍實體節點的 Gossip 訊號，若滿足 K 個鄰居的危險宣告，才觸發共識等級的異常事件。