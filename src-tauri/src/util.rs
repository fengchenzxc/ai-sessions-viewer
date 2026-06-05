// 跨 agent / 跨模块共享的工具函数。
// 这里只放"agent 无关"的逻辑——目录定位、时间戳、JSONL 文件写入、标题清洗等。
// agent-specific 的解析逻辑请放到对应的 `agents/<name>.rs` 文件里。

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::types::{Block, Msg};

pub fn home() -> PathBuf {
    dirs::home_dir().expect("无法定位用户主目录")
}

pub fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn mtime_millis(p: &Path) -> u64 {
    fs::metadata(p)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn is_jsonl(p: &Path) -> bool {
    p.extension().map(|x| x == "jsonl").unwrap_or(false)
}

/// 把首条用户消息清洗成简短标题：去掉 <...> 标记块、折叠空白、截断。
pub fn clean_title(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.starts_with("Caveat:") {
        return String::new();
    }
    let mut out = String::new();
    let mut depth = 0i32;
    for c in trimmed.chars() {
        match c {
            '<' => depth += 1,
            '>' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    let collapsed: String = out.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.chars().take(100).collect()
}

pub fn text_block(kind: &str, s: &str) -> Block {
    Block {
        kind: kind.to_string(),
        text: Some(s.to_string()),
        ..Default::default()
    }
}

pub fn simple_msg(role: &str, ts: Option<String>, block: Block) -> Msg {
    Msg {
        uuid: None,
        role: role.to_string(),
        timestamp: ts,
        model: None,
        sidechain: false,
        blocks: vec![block],
    }
}

/// 简易 ISO-8601 UTC 时间字符串：`YYYY-MM-DDTHH:MM:SS.mmmZ`。
/// 只用于写入 codex 的 thread_name_updated / session_index 行，精度够用。
pub fn format_iso8601_utc(secs: i64, ms: u32) -> String {
    let s = secs.rem_euclid(60) as u32;
    let m = (secs.div_euclid(60)).rem_euclid(60) as u32;
    let h = (secs.div_euclid(3600)).rem_euclid(24) as u32;
    let mut days = secs.div_euclid(86400);
    let mut year: i64 = 1970;
    loop {
        let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
        let yd = if leap { 366 } else { 365 };
        if days < yd {
            break;
        }
        days -= yd;
        year += 1;
    }
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let mdays = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month: usize = 0;
    while month < 12 && days >= mdays[month] as i64 {
        days -= mdays[month] as i64;
        month += 1;
    }
    let day = days as u32 + 1;
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        year,
        month + 1,
        day,
        h,
        m,
        s,
        ms
    )
}

/// 毫秒时间戳 → `YYYY-MM-DD`（UTC）。给统计 dashboard 的活跃度热图按日分桶用。
/// 与 `format_iso8601_utc` 共享同一套手写历法（不引 chrono）—— 这里只截前 10 位日期部分。
pub fn yyyymmdd_utc(ms: u64) -> String {
    let s = format_iso8601_utc((ms / 1000) as i64, (ms % 1000) as u32);
    s.chars().take(10).collect()
}

/// 毫秒时间戳 → `YYYY-MM-DD`（用户所在时区）。统计窗口按本地日历日切的话，
/// daily 热图也必须按本地日切，否则 "Today" 总额对得上 codeburn，但热图上
/// 同一笔花费会被画到错误的格子里。
pub fn yyyymmdd_local(ms: u64) -> String {
    use chrono::{Local, TimeZone};
    let secs = (ms / 1000) as i64;
    let nsecs = ((ms % 1000) as u32) * 1_000_000;
    match Local.timestamp_opt(secs, nsecs).single() {
        Some(dt) => dt.format("%Y-%m-%d").to_string(),
        None => yyyymmdd_utc(ms),
    }
}

/// ISO-8601 → unix 毫秒。只解析 `YYYY-MM-DDTHH:MM:SS[.fff]Z` 这一形态；
/// 其他形态退到 None（聚合器会用文件 mtime 兜底）。手写以免引 chrono。
/// 给统计聚合器从 JSONL 时间戳串还原 unix ms 用。
pub fn parse_iso8601_ms(s: &str) -> Option<i64> {
    let bytes = s.as_bytes();
    if bytes.len() < 19 || bytes[4] != b'-' || bytes[7] != b'-' || bytes[10] != b'T' {
        return None;
    }
    let year: i64 = std::str::from_utf8(&bytes[0..4]).ok()?.parse().ok()?;
    let mon: u32 = std::str::from_utf8(&bytes[5..7]).ok()?.parse().ok()?;
    let day: u32 = std::str::from_utf8(&bytes[8..10]).ok()?.parse().ok()?;
    let h: u32 = std::str::from_utf8(&bytes[11..13]).ok()?.parse().ok()?;
    let m: u32 = std::str::from_utf8(&bytes[14..16]).ok()?.parse().ok()?;
    let sec: u32 = std::str::from_utf8(&bytes[17..19]).ok()?.parse().ok()?;
    let mut ms: u32 = 0;
    if bytes.len() > 19 && bytes[19] == b'.' {
        let end = (20 + 3).min(bytes.len());
        let frac = std::str::from_utf8(&bytes[20..end]).ok()?;
        if let Ok(n) = frac.parse::<u32>() {
            ms = n * 10u32.pow(3 - frac.len() as u32);
        }
    }
    // 转 unix epoch 秒：简易历法
    let mut days: i64 = 0;
    for y in 1970..year {
        let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
        days += if leap { 366 } else { 365 };
    }
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let mdays: [i64; 12] = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    for &md in mdays.iter().take((mon - 1) as usize) {
        days += md;
    }
    days += (day - 1) as i64;
    let secs = days * 86400 + (h as i64) * 3600 + (m as i64) * 60 + sec as i64;
    Some(secs * 1000 + ms as i64)
}

/// 校验 rename 名称：去空白后非空且不过长。返回 trimmed 切片。
pub fn validate_rename_name(name: &str) -> Result<&str, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("名称不能为空".to_string());
    }
    if trimmed.chars().count() > 200 {
        return Err("名称过长".to_string());
    }
    Ok(trimmed)
}

/// 安全地把一行追加到 JSONL：若文件末尾不是换行，先补一个，再写 `line + "\n"`。
pub fn append_jsonl_line(path: &Path, line: &str) -> Result<(), String> {
    let needs_nl = fs::metadata(path)
        .map(|m| m.len())
        .ok()
        .and_then(|len| {
            if len == 0 {
                Some(false)
            } else {
                use std::io::{Read, Seek, SeekFrom};
                let mut g = fs::File::open(path).ok()?;
                g.seek(SeekFrom::End(-1)).ok()?;
                let mut buf = [0u8; 1];
                g.read_exact(&mut buf).ok()?;
                Some(buf[0] != b'\n')
            }
        })
        .unwrap_or(false);
    let mut f = fs::OpenOptions::new()
        .append(true)
        .open(path)
        .map_err(|e| format!("打开会话文件失败: {e}"))?;
    if needs_nl {
        f.write_all(b"\n")
            .map_err(|e| format!("追加换行失败: {e}"))?;
    }
    f.write_all(line.as_bytes())
        .map_err(|e| format!("写入 rename 行失败: {e}"))?;
    f.write_all(b"\n")
        .map_err(|e| format!("写入换行失败: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yyyymmdd_at_unix_epoch_is_1970_01_01() {
        assert_eq!(yyyymmdd_utc(0), "1970-01-01");
    }

    #[test]
    fn yyyymmdd_handles_leap_day() {
        // 2024-02-29T00:00:00Z = 1709164800 s
        assert_eq!(yyyymmdd_utc(1_709_164_800_000), "2024-02-29");
    }

    #[test]
    fn yyyymmdd_strips_to_date_only() {
        // 2026-05-23T12:34:56Z = 1779539696 s
        assert_eq!(yyyymmdd_utc(1_779_539_696_000), "2026-05-23");
    }
}
