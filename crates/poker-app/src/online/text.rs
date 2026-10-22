//! User-facing strings for the online screens.
//!
//! Sources: docs/specs/ui.md sections 5.2, 8.9 and 8.4 (reference client
//! wording, kept verbatim where the original defines it). Strings that were
//! never defined by the reference implementation are marked with a comment.

use poker_client::ClientError;

/// Connection page labels.
pub const SERVER_ADDRESS_HINT: &str = "服务器地址";
/// Player name input hint (reference: 输入你的昵称).
pub const PLAYER_NAME_HINT: &str = "输入你的昵称";
/// Connect button (reference: 连接).
pub const CONNECT: &str = "连接";
/// Disconnect button (reference: 断开).
pub const DISCONNECT: &str = "断开";
/// Back to home.
pub const BACK_HOME: &str = "← 返回主页";
/// Default local server address shown before the embedded server URL arrives.
pub const LOCAL_SERVER_STARTING: &str = "本地服务启动中…";
/// Local server hint, parameter is the ws URL.
pub fn local_server_ready(url: &str) -> String {
    format!("本机服务已就绪 · 当前地址 {url}")
}
/// Fallback when no embedded server is available.
pub const LOCAL_SERVER_UNAVAILABLE: &str = "本机服务不可用，请手动输入服务器地址";

/// Lobby labels.
pub const LOBBY_TITLE: &str = "联机对局";
pub const ROOM_CODE_LABEL: &str = "房间号";
pub const SHARE_ROOM_HINT: &str = "把房间号发给朋友加入";
pub const PLAYERS_LABEL: &str = "玩家列表";
pub const WAITING_HOST: &str = "等待房主开始游戏…";
pub const START_GAME: &str = "开始游戏";
pub const LEAVE_ROOM: &str = "离开房间";
pub const JOIN_ROOM: &str = "进入房间";
pub const ROOM_CODE_FIELD: &str = "房间号";
pub const MAX_PLAYERS_FIELD: &str = "人数";
pub const SHORT_DECK_FIELD: &str = "短牌";
pub const STATUS_HOST: &str = "房主";
pub const STATUS_ME: &str = "（你）";
pub const PLAYER_OFFLINE: &str = "断线中";
pub const PLAYER_BUSTED: &str = "已破产";
pub const ROLE_AI: &str = "AI";

/// Server version diagnostic line.
pub fn server_version(server_id: &str, app_version: &str, protocol_version: u8) -> String {
    format!("服务器：{server_id} v{app_version}（protocol {protocol_version}）")
}

/// Room joined / created status lines (reference: 房间 N / 房主).
pub fn room_joined(code: &str, is_host: bool) -> String {
    if is_host {
        format!("已进入房间 {code}（房主）")
    } else {
        format!("已进入房间 {code}")
    }
}

/// Reconnect banners (reference ui.md section 5.2).
pub const CONNECTED: &str = "已连接到服务器";
pub const RECONNECTED_RESUMING: &str = "已重新连接，正在恢复原座位…";
/// Reconnect banner, parameter is the delay in seconds.
pub fn link_lost(seconds: u64) -> String {
    format!("连接中断，约 {seconds} 秒后恢复原座位…")
}
/// Reconnect attempt line shown under the banner.
pub fn reconnecting_attempt(attempt: u32) -> String {
    format!("重连中…（第 {attempt} 次）")
}
pub const RECONNECT_FAILED: &str = "自动重连已停止，请手动重连";
pub const MANUAL_RECONNECT: &str = "重新连接";
pub const HOST_CHANGED_YOU: &str = "房主已离开，你现在是房主";
pub const HOST_CHANGED: &str = "房主已变更";
/// Server capacity message shown when close code 1013 arrives.
pub const KICKED: &str = "账号已在其他窗口恢复连接";
pub const RESUME_EXPIRED_HINT: &str = "恢复信息已失效，请重新加入房间";

/// Game starting banner (reference ui.md section 5.2).
pub fn game_starting(player_count: i64, human_count: i64, ai_count: i64) -> String {
    format!("游戏开始！{player_count}人 ({human_count}真人 + {ai_count}AI)")
}

/// Settlement overlay.
pub const SETTLEMENT_SHOWDOWN: &str = "🏆 摊牌";
pub const SETTLEMENT_WON_BY_FOLD: &str = "🏆 胜出";
pub fn pot(value: u64) -> String {
    format!("底池 {value}")
}
pub fn winner_line(name: &str, amount: u64) -> String {
    format!("{name} 赢得 {amount}")
}
pub const NEXT_HAND: &str = "下一手 →";
/// Ready button while other humans are not ready yet.
pub fn waiting_ready(waiting_for: u32) -> String {
    format!("等待中（{waiting_for} 人未准备）…")
}
pub const STARTING_NEXT_HAND: &str = "正在开始下一手…";
pub const ALL_READY: &str = "所有玩家已准备，下一手即将开始";

/// Table status lines (reference ui.md section 8.4).
pub const WAITING_OTHERS: &str = "等待其他玩家行动…";
pub const WAITING_SERVER: &str = "等待服务器确认…";
pub const FOLDED_WAITING: &str = "你已弃牌，等待本局结束";
pub const ALLIN_WAITING: &str = "你已全下，等待摊牌";
pub const BUSTED: &str = "你已破产 — 筹码输光，等待新一局或离开";
pub fn your_turn(phase: &str, active: usize) -> String {
    format!("轮到你了 — {phase} · {active} 人在局")
}
pub fn thinking(name: &str) -> String {
    format!("{name} 思考中…")
}
pub const ACTION_ACK_OK: &str = "行动已确认";

/// Phase labels (reference ui.md section 8.3).
pub const PHASE_PREFLOP: &str = "翻牌前";
pub const PHASE_FLOP: &str = "翻牌";
pub const PHASE_TURN: &str = "转牌";
pub const PHASE_RIVER: &str = "河牌";
pub const PHASE_SHOWDOWN: &str = "摊牌";
pub const PHASE_IDLE: &str = "空闲";

/// Action labels (reference ui.md section 8.3).
pub fn action_label(token: &str) -> &'static str {
    match token {
        "fold" => "弃牌",
        "check" => "过牌",
        "call" => "跟注",
        "raise" => "加注",
        "allin" => "全下",
        "small blind" => "小盲",
        "big blind" => "大盲",
        "ante" => "前注",
        "dealer ante" => "庄家前注",
        _ => "行动",
    }
}

/// Error text for a connection attempt, following ui.md section 5.2 where the
/// reference client defines one.
pub fn connect_error(error: &ClientError) -> String {
    match error {
        ClientError::InvalidUrl(_) => "服务器地址必须以 ws:// 或 wss:// 开头".to_owned(),
        ClientError::UnsupportedScheme(_) => "当前版本只支持 ws:// 服务器地址".to_owned(),
        ClientError::WebSocket(detail) => format!("无法连接: {detail}"),
        ClientError::HandshakeTimeout { timeout_ms } => {
            format!("连接服务器超时（{timeout_ms} ms）")
        }
        ClientError::Hello(_) => "连接的服务器不是德州扑克服务（协议 v2）".to_owned(),
        ClientError::NotConnected => "尚未连接到服务器".to_owned(),
        ClientError::SendFailed(detail) => format!("发送失败，正在重连…（{detail}）"),
        ClientError::MessageTooLarge { .. } => "消息内容过长，已取消发送".to_owned(),
        ClientError::NoSession => "没有可恢复的会话".to_owned(),
        ClientError::Protocol { .. } => "服务器消息格式错误，连接已关闭".to_owned(),
        ClientError::Closed => "连接已关闭".to_owned(),
    }
}

/// Human-readable notice for a WebSocket close code (reference matrix in
/// docs/specs/ui.md section 4/5.2 and protocol-v2 section 7).
pub fn close_notice(code: Option<u16>) -> String {
    match code {
        Some(4001) => KICKED.to_owned(),
        Some(1008) => "消息发送过于频繁，已被服务器断开".to_owned(),
        Some(1009) => "消息过大，已被服务器断开".to_owned(),
        Some(1013) => "服务器容量已满，请稍后再试".to_owned(),
        Some(1000) => "已断开连接".to_owned(),
        Some(other) => format!("连接已断开（{other}）"),
        None => "连接中断".to_owned(),
    }
}

/// One decimal-free chip formatter used by online screens.
pub fn chips(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, byte) in digits.bytes().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(byte as char);
    }
    out
}
