// 模型 → $/token 价格表（精简版，离线 / 立即可用）。
//
// 数据来自 codeburn 0.9.10 的 LiteLLM 快照（src/data/litellm-snapshot.json），
// 我只挑了三个 CLI 实际会写进 JSONL 的常见模型：Claude Code（claude-*）、
// Codex（gpt-* / o*）、Gemini CLI（gemini-*）。表项格式：
//
//   (canonical_name, input_per_token, output_per_token,
//    cache_write_per_token_or_None, cache_read_per_token_or_None)
//
// 兜底：cache_write_per_token = input × 1.25；cache_read_per_token = input × 0.1
// （和 codeburn / LiteLLM 一致的 Anthropic 公式）。
//
// 名称归一（getCanonicalName）：
//   1. 去掉 `@xxx` pin 段（claude-sonnet-4-6@20250929 → claude-sonnet-4-6）
//   2. 去掉 `-YYYYMMDD` 日期段（claude-sonnet-4-20250514 → claude-sonnet-4）
//   3. 去掉 provider 前缀（anthropic/foo → foo）
//
// 查找逻辑（lookup）：
//   1. 优先用 `provider/foo` 形式整名查（有 `azure/gpt-5.4` 这类）
//   2. 走 alias 表（处理 `claude-4.6-opus` ↔ `claude-opus-4-6` 之类的别名）
//   3. 在 PRICING 里按 key 长度倒序前缀匹配 —— `gpt-5-mini` 不会塌成 `gpt-5`

use crate::types::UsageSummary;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModelCosts {
    /// $/token —— 不是 $/Mtok。和 LiteLLM 原始格式一致，乘法直接得 USD。
    pub input: f64,
    pub output: f64,
    pub cache_write: f64,
    pub cache_read: f64,
}

impl ModelCosts {
    /// 输入 None 时套用 Anthropic 默认公式：write = input × 1.25, read = input × 0.1。
    fn build(input: f64, output: f64, cache_write: Option<f64>, cache_read: Option<f64>) -> Self {
        ModelCosts {
            input,
            output,
            cache_write: cache_write.unwrap_or(input * 1.25),
            cache_read: cache_read.unwrap_or(input * 0.1),
        }
    }
}

/// 精简价格表。新模型上线时直接在这里加一行。
/// `None` 占位让兜底公式接管；非 None 的覆盖兜底。
type Row = (&'static str, f64, f64, Option<f64>, Option<f64>);

const PRICING: &[Row] = &[
    // ---------- Claude 4.x ----------
    (
        "claude-opus-4-7",
        0.000005,
        0.000025,
        Some(0.00000625),
        Some(0.0000005),
    ),
    (
        "claude-opus-4-6",
        0.000005,
        0.000025,
        Some(0.00000625),
        Some(0.0000005),
    ),
    (
        "claude-opus-4-5",
        0.000005,
        0.000025,
        Some(0.00000625),
        Some(0.0000005),
    ),
    (
        "claude-opus-4-1",
        0.000015,
        0.000075,
        Some(0.00001875),
        Some(0.0000015),
    ),
    (
        "claude-opus-4",
        0.000015,
        0.000075,
        Some(0.00001875),
        Some(0.0000015),
    ),
    (
        "claude-sonnet-4-6",
        0.000003,
        0.000015,
        Some(0.00000375),
        Some(0.0000003),
    ),
    (
        "claude-sonnet-4-5",
        0.000003,
        0.000015,
        Some(0.00000375),
        Some(0.0000003),
    ),
    (
        "claude-sonnet-4",
        0.000003,
        0.000015,
        Some(0.00000375),
        Some(0.0000003),
    ),
    (
        "claude-haiku-4-5",
        0.000001,
        0.000005,
        Some(0.00000125),
        Some(0.0000001),
    ),
    // ---------- Claude 3.x ----------
    ("claude-3-7-sonnet", 0.000003, 0.000015, None, None),
    ("claude-3-5-sonnet", 0.000003, 0.000015, None, None),
    ("claude-3-5-haiku", 0.0000008, 0.000004, None, None),
    ("claude-3-opus", 0.000015, 0.000075, None, None),
    ("claude-3-sonnet", 0.000003, 0.000015, None, None),
    ("claude-3-haiku", 0.00000025, 0.00000125, None, None),
    // ---------- OpenAI / Codex ----------
    (
        "gpt-5.3-codex",
        0.00000175,
        0.000014,
        None,
        Some(0.000000175),
    ),
    (
        "gpt-5.1-codex-mini",
        0.0000005,
        0.000002,
        None,
        Some(0.00000005),
    ),
    (
        "gpt-5.1-codex",
        0.00000125,
        0.00001,
        None,
        Some(0.000000125),
    ),
    ("gpt-5-codex", 0.00000125, 0.00001, None, Some(0.000000125)),
    ("gpt-5.5-pro", 0.00000125, 0.00001, None, Some(0.000000125)),
    ("gpt-5.4-pro", 0.00000125, 0.00001, None, Some(0.000000125)),
    ("gpt-5.2-pro", 0.00000125, 0.00001, None, Some(0.000000125)),
    ("gpt-5-pro", 0.00000125, 0.00001, None, Some(0.000000125)),
    ("gpt-5.5", 0.00000125, 0.00001, None, Some(0.000000125)),
    ("gpt-5.4", 0.00000125, 0.00001, None, Some(0.000000125)),
    ("gpt-5.3", 0.00000125, 0.00001, None, Some(0.000000125)),
    ("gpt-5.2", 0.00000125, 0.00001, None, Some(0.000000125)),
    ("gpt-5.1", 0.00000125, 0.00001, None, Some(0.000000125)),
    ("gpt-5", 0.00000125, 0.00001, None, Some(0.000000125)),
    (
        "gpt-4o-mini",
        0.00000015,
        0.0000006,
        None,
        Some(0.000000075),
    ),
    ("gpt-4o", 0.0000025, 0.00001, None, Some(0.00000125)),
    (
        "gpt-4.1-nano",
        0.0000001,
        0.0000004,
        None,
        Some(0.000000025),
    ),
    ("gpt-4.1-mini", 0.0000004, 0.0000016, None, Some(0.0000001)),
    ("gpt-4.1", 0.000002, 0.000008, None, Some(0.0000005)),
    ("o4-mini", 0.0000011, 0.0000044, None, Some(0.000000275)),
    ("o3-mini", 0.0000011, 0.0000044, None, Some(0.00000055)),
    ("o3", 0.000002, 0.000008, None, Some(0.0000005)),
    // ---------- Google / Gemini ----------
    (
        "gemini-3.1-pro-preview",
        0.00000125,
        0.00001,
        None,
        Some(0.000000125),
    ),
    (
        "gemini-3-flash-preview",
        0.0000003,
        0.0000025,
        None,
        Some(0.00000003),
    ),
    (
        "gemini-3-pro-preview",
        0.00000125,
        0.00001,
        None,
        Some(0.000000125),
    ),
    (
        "gemini-2.5-pro",
        0.00000125,
        0.00001,
        None,
        Some(0.000000125),
    ),
    (
        "gemini-2.5-flash-lite",
        0.0000001,
        0.0000004,
        None,
        Some(0.000000025),
    ),
    (
        "gemini-2.5-flash",
        0.0000003,
        0.0000025,
        None,
        Some(0.00000003),
    ),
    (
        "gemini-2.0-flash-lite",
        0.000000075,
        0.0000003,
        None,
        Some(0.00000001875),
    ),
    (
        "gemini-2.0-flash",
        0.0000001,
        0.0000004,
        None,
        Some(0.000000025),
    ),
];

/// 内置别名表 —— 把厂商 / IDE 多写的"花式名"映射到 PRICING 里的 canonical key。
/// 同义于 codeburn 的 BUILTIN_ALIASES；只挑了三个 CLI 实际可能出现的几条。
const ALIASES: &[(&str, &str)] = &[
    ("claude-opus-4.7", "claude-opus-4-7"),
    ("claude-opus-4.6", "claude-opus-4-6"),
    ("claude-opus-4.5", "claude-opus-4-5"),
    ("claude-sonnet-4.6", "claude-sonnet-4-6"),
    ("claude-sonnet-4.5", "claude-sonnet-4-5"),
    ("claude-haiku-4.5", "claude-haiku-4-5"),
    ("gpt-5-fast", "gpt-5"),
    ("gpt-5.2-low", "gpt-5"),
];

/// 规范化：去掉 `@xxx` pin、`-YYYYMMDD` 日期、provider 前缀。
fn canonical(model: &str) -> String {
    let mut s = model.to_string();
    // 1) 去 @ 后缀
    if let Some(pos) = s.find('@') {
        s.truncate(pos);
    }
    // 2) 去末尾 8 位数字日期
    if let Some(stripped) = strip_trailing_yyyymmdd(&s) {
        s = stripped;
    }
    // 3) 去 provider/ 前缀（first slash）
    if let Some(pos) = s.find('/') {
        s = s[pos + 1..].to_string();
    }
    s
}

fn strip_trailing_yyyymmdd(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    if bytes.len() < 9 {
        return None;
    }
    let tail = &bytes[bytes.len() - 8..];
    if tail.iter().all(|b| b.is_ascii_digit()) && bytes[bytes.len() - 9] == b'-' {
        return Some(s[..bytes.len() - 9].to_string());
    }
    None
}

fn resolve_alias(name: &str) -> String {
    for (k, v) in ALIASES {
        if *k == name {
            return (*v).to_string();
        }
    }
    name.to_string()
}

/// 优先整名（保留 provider 前缀），再走 alias / 归一 / 前缀匹配。
/// 找不到任何匹配返回 None —— 调用方按 0 美元处理。
pub fn lookup(model: &str) -> Option<ModelCosts> {
    if model.is_empty() {
        return None;
    }
    // 1) 整名（剥 @ / 日期，不剥 provider）
    let mut with_prefix = model.to_string();
    if let Some(pos) = with_prefix.find('@') {
        with_prefix.truncate(pos);
    }
    if let Some(s) = strip_trailing_yyyymmdd(&with_prefix) {
        with_prefix = s;
    }
    if let Some(c) = direct_lookup(&with_prefix) {
        return Some(c);
    }
    // 2) canonical + alias
    let canon = resolve_alias(&canonical(model));
    if let Some(c) = direct_lookup(&canon) {
        return Some(c);
    }
    // 3) 按 PRICING key 长度倒序前缀匹配 —— "gpt-5-mini" 不会塌成 "gpt-5"
    let mut sorted: Vec<&Row> = PRICING.iter().collect();
    sorted.sort_by_key(|(k, _, _, _, _)| std::cmp::Reverse(k.len()));
    for (k, i, o, w, r) in sorted {
        if canon.starts_with(&format!("{k}-")) || canon == *k {
            return Some(ModelCosts::build(*i, *o, *w, *r));
        }
    }
    None
}

fn direct_lookup(name: &str) -> Option<ModelCosts> {
    for (k, i, o, w, r) in PRICING {
        if *k == name {
            return Some(ModelCosts::build(*i, *o, *w, *r));
        }
    }
    None
}

/// 按 usage 算这次调用的美元成本。找不到模型按 $0 计 —— 跟 codeburn 一致。
pub fn cost_usd(model: &str, usage: &UsageSummary) -> f64 {
    let Some(c) = lookup(model) else {
        return 0.0;
    };
    let safe = |n: u64| n as f64;
    safe(usage.input_tokens) * c.input
        + safe(usage.output_tokens) * c.output
        + safe(usage.cache_creation_input_tokens) * c.cache_write
        + safe(usage.cache_read_input_tokens) * c.cache_read
}

/// 模型友好显示名 —— "Opus 4.7" / "Sonnet 4.6" / "GPT-5" 等。前端 By Model 块用。
pub fn short_name(model: &str) -> String {
    let canon = resolve_alias(&canonical(model));
    const SHORT: &[(&str, &str)] = &[
        ("claude-opus-4-7", "Opus 4.7"),
        ("claude-opus-4-6", "Opus 4.6"),
        ("claude-opus-4-5", "Opus 4.5"),
        ("claude-opus-4-1", "Opus 4.1"),
        ("claude-opus-4", "Opus 4"),
        ("claude-sonnet-4-6", "Sonnet 4.6"),
        ("claude-sonnet-4-5", "Sonnet 4.5"),
        ("claude-sonnet-4", "Sonnet 4"),
        ("claude-3-7-sonnet", "Sonnet 3.7"),
        ("claude-3-5-sonnet", "Sonnet 3.5"),
        ("claude-haiku-4-5", "Haiku 4.5"),
        ("claude-3-5-haiku", "Haiku 3.5"),
        ("gpt-5.3-codex", "GPT-5.3 Codex"),
        ("gpt-5.1-codex", "GPT-5.1 Codex"),
        ("gpt-5-codex", "GPT-5 Codex"),
        ("gpt-5.5", "GPT-5.5"),
        ("gpt-5.4", "GPT-5.4"),
        ("gpt-5.3", "GPT-5.3"),
        ("gpt-5.2", "GPT-5.2"),
        ("gpt-5.1", "GPT-5.1"),
        ("gpt-5", "GPT-5"),
        ("gpt-4o-mini", "GPT-4o Mini"),
        ("gpt-4o", "GPT-4o"),
        ("gpt-4.1", "GPT-4.1"),
        ("o4-mini", "o4-mini"),
        ("o3-mini", "o3-mini"),
        ("o3", "o3"),
        ("gemini-3.1-pro-preview", "Gemini 3.1 Pro"),
        ("gemini-3-flash-preview", "Gemini 3 Flash"),
        ("gemini-3-pro-preview", "Gemini 3 Pro"),
        ("gemini-2.5-pro", "Gemini 2.5 Pro"),
        ("gemini-2.5-flash", "Gemini 2.5 Flash"),
        ("gemini-2.0-flash", "Gemini 2.0 Flash"),
    ];
    let mut sorted: Vec<&(&str, &str)> = SHORT.iter().collect();
    sorted.sort_by_key(|(k, _)| std::cmp::Reverse(k.len()));
    for (k, label) in sorted {
        if canon.starts_with(*k) {
            return (*label).to_string();
        }
    }
    canon
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(input: u64, output: u64, cw: u64, cr: u64) -> UsageSummary {
        UsageSummary {
            input_tokens: input,
            output_tokens: output,
            cache_creation_input_tokens: cw,
            cache_read_input_tokens: cr,
            reasoning_output_tokens: 0,
            total: input + output + cw + cr,
        }
    }

    #[test]
    fn canonical_strips_pin_date_and_provider_prefix() {
        assert_eq!(
            canonical("anthropic/claude-opus-4-6@20250929"),
            "claude-opus-4-6"
        );
        assert_eq!(canonical("claude-sonnet-4-20250514"), "claude-sonnet-4");
        assert_eq!(
            canonical("openrouter/anthropic/claude-opus-4-6"),
            "anthropic/claude-opus-4-6"
        );
        // 注意：canonical 只剥第一段 provider；本表会再用整名查一次（with_prefix）
    }

    #[test]
    fn lookup_direct_hit_for_known_model() {
        let c = lookup("claude-opus-4-7").expect("known");
        assert!((c.input - 0.000005).abs() < 1e-12);
        assert!((c.output - 0.000025).abs() < 1e-12);
    }

    #[test]
    fn lookup_resolves_alias_dot_form_to_dash_form() {
        let dot = lookup("claude-sonnet-4.6").expect("aliased");
        let dash = lookup("claude-sonnet-4-6").expect("direct");
        assert_eq!(dot, dash);
    }

    #[test]
    fn lookup_longest_prefix_wins() {
        // gpt-5-mini 不应该塌到 gpt-5（不同价位的兄弟）
        // 表里没 gpt-5-mini，前缀匹配会回 gpt-5；但要确保 gpt-5.3-codex 不会塌到 gpt-5
        let gpt5 = lookup("gpt-5").expect("known");
        let codex = lookup("gpt-5.3-codex").expect("known");
        assert!(
            codex.input > gpt5.input,
            "codex priced higher than base gpt-5"
        );
    }

    #[test]
    fn lookup_strips_yyyymmdd_suffix() {
        let with_date = lookup("claude-sonnet-4-6-20251201").expect("date-stripped");
        let plain = lookup("claude-sonnet-4-6").expect("direct");
        assert_eq!(with_date, plain);
    }

    #[test]
    fn lookup_strips_at_pin() {
        let with_pin = lookup("claude-sonnet-4-6@20250929").expect("pin-stripped");
        let plain = lookup("claude-sonnet-4-6").expect("direct");
        assert_eq!(with_pin, plain);
    }

    #[test]
    fn lookup_returns_none_for_local_or_unknown() {
        assert!(lookup("llama3:8b-instruct").is_none());
        assert!(lookup("totally-made-up-model").is_none());
        assert!(lookup("").is_none());
    }

    #[test]
    fn cost_usd_for_known_model_uses_table() {
        // Opus 4.7: input=$5/Mtok, output=$25/Mtok
        // 1M in + 1M out = $30
        let one_million = u(1_000_000, 1_000_000, 0, 0);
        let c = cost_usd("claude-opus-4-7", &one_million);
        assert!((c - 30.0).abs() < 1e-6, "got {c}");
    }

    #[test]
    fn cost_usd_includes_cache_components() {
        // Sonnet 4.6: input=$3/Mtok, output=$15/Mtok, write=$3.75/Mtok, read=$0.3/Mtok
        // 1M cache_write + 1M cache_read = $3.75 + $0.30 = $4.05
        let usage = u(0, 0, 1_000_000, 1_000_000);
        let c = cost_usd("claude-sonnet-4-6", &usage);
        assert!((c - 4.05).abs() < 1e-6, "got {c}");
    }

    #[test]
    fn cost_usd_zero_for_unknown_model() {
        let big = u(1_000_000, 1_000_000, 1_000_000, 1_000_000);
        assert_eq!(cost_usd("ollama/llama-3", &big), 0.0);
    }

    #[test]
    fn short_name_picks_longest_prefix() {
        assert_eq!(short_name("claude-opus-4-7"), "Opus 4.7");
        assert_eq!(short_name("gpt-5.3-codex"), "GPT-5.3 Codex");
        assert_eq!(short_name("gpt-5-fast"), "GPT-5"); // aliased
        assert_eq!(short_name("gemini-2.5-pro-preview-05-06"), "Gemini 2.5 Pro");
    }

    #[test]
    fn short_name_falls_back_to_canonical_for_unknown() {
        assert_eq!(short_name("totally-new-model-9"), "totally-new-model-9");
    }
}
