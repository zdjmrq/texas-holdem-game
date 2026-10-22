//! Private reconnect credentials stored beside the app settings.
use poker_client::SessionCredentials;
use serde_json::json;
fn path() -> std::path::PathBuf {
    crate::assets::config_dir().join("online-session.json")
}
pub fn load() -> Option<SessionCredentials> {
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(path()).ok()?).ok()?;
    Some(SessionCredentials {
        server_url: v["serverUrl"].as_str()?.into(),
        room_code: v["roomCode"].as_str()?.into(),
        resume_token: v["resumeToken"].as_str()?.into(),
        player_id: v["playerId"].as_str().map(str::to_owned),
    })
}
pub fn save(session: Option<&SessionCredentials>) {
    if let Some(s) = session {
        let _ = std::fs::create_dir_all(crate::assets::config_dir());
        let _=std::fs::write(path(),json!({"serverUrl":s.server_url,"roomCode":s.room_code,"resumeToken":s.resume_token,"playerId":s.player_id}).to_string());
    } else {
        let _ = std::fs::remove_file(path());
    }
}
