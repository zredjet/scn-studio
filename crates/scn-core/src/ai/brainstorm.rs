//! アイデアメモのネタ出し（T4-29、D088）：方向性と数を伝えて、アイデアの候補を出してもらう（出力の形
//! `idea_candidates`）。候補のうち人が選んだものだけを、アイデアメモ（未整理）として足す（`workspace::add_ai_ideas`。
//! 出自は `ai`）。

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::run::parse_json_items;
use super::task::AiTask;

/// 1回に頼める候補の数の上限。
pub const MAX_IDEA_CANDIDATES: u32 = 10;
/// 候補の数を書かなかったときの数。
pub const DEFAULT_IDEA_CANDIDATES: u32 = 5;

/// アイデアの候補。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdeaCandidate {
    /// 一言の題（無ければ空）。
    #[serde(default)]
    pub title: String,
    /// メモの本文。
    pub body: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

impl IdeaCandidate {
    /// メモの本文にする（題があれば1行目に置く）。
    pub fn memo_body(&self) -> String {
        let title = self.title.trim();
        let body = self.body.trim();
        if title.is_empty() || body.starts_with(title) {
            body.to_owned()
        } else {
            format!("{title}\n\n{body}")
        }
    }
}

/// ネタ出しの頼み（アプリが実行のたびに渡す）。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct IdeaRequest {
    /// 方向性（どんなアイデアが欲しいか）。
    #[serde(default)]
    pub direction: String,
    /// 候補の数（1〜[`MAX_IDEA_CANDIDATES`]。0 や書かなければ [`DEFAULT_IDEA_CANDIDATES`]）。
    #[serde(default)]
    pub count: u32,
}

impl IdeaRequest {
    /// 頼む数（1〜上限に収める）。
    pub fn count(&self) -> u32 {
        match self.count {
            0 => DEFAULT_IDEA_CANDIDATES,
            n => n.min(MAX_IDEA_CANDIDATES),
        }
    }
}

/// ネタ出しの依頼に、今回の方向性と数を書き足したタスク。
pub fn with_idea_request(task: &AiTask, request: &IdeaRequest) -> AiTask {
    let mut t = task.clone();
    let prompt = t.prompt.unwrap_or_default();
    let direction = request.direction.trim();
    let direction = if direction.is_empty() {
        "（指定なし。企画書とメモから、足りなそうな方向を選んでください）"
    } else {
        direction
    };
    t.prompt = Some(format!(
        "{}\n\n今回の方向性：{direction}\n候補の数：{}",
        prompt.trim_end(),
        request.count()
    ));
    t
}

fn candidate_of(value: &Value) -> Option<IdeaCandidate> {
    let text = |key: &str| {
        value
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default()
            .to_owned()
    };
    let body = text("body");
    if body.is_empty() {
        return None;
    }
    let tags = match value.get("tags") {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_owned)
            .collect(),
        Some(Value::String(tag)) if !tag.trim().is_empty() => vec![tag.trim().to_owned()],
        _ => Vec::new(),
    };
    Some(IdeaCandidate {
        title: text("title"),
        body,
        tags,
    })
}

/// 応答から候補を読む（`{"ideas": [...]}`、配列、1行に1つの JSON のどれでも）。読めなかった行も返す。
pub fn parse_idea_candidates(text: &str) -> (Vec<IdeaCandidate>, Vec<String>) {
    parse_json_items(text, "ideas", &candidate_of)
}
