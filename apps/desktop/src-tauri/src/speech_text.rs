//! Speech-only cleanup. Display/history retain the original body and ordinary asides.
fn stage(text: &str) -> bool {
    let mut s = text.trim().trim_matches('*').trim();
    if [
        "动作：",
        "动作:",
        "神态：",
        "神态:",
        "旁白：",
        "旁白:",
        "舞台：",
        "舞台:",
    ]
    .iter()
    .any(|p| s.starts_with(p))
    {
        return true;
    }
    if [
        "例如",
        "比如",
        "意思",
        "术语",
        "函数",
        "算法",
        "星期",
        "周日",
        "周一",
        "周二",
        "周三",
        "周四",
        "周五",
        "周六",
        "注：",
        "备注：",
    ]
    .iter()
    .any(|p| s.contains(p))
    {
        return false;
    }
    for _ in 0..4 {
        let old = s;
        for p in [
            "她",
            "他",
            "我",
            "轻轻地",
            "轻轻",
            "微微",
            "慢慢地",
            "慢慢",
            "悄悄",
            "缓缓",
            "忽然",
            "害羞地",
            "开心地",
        ] {
            if let Some(rest) = s.strip_prefix(p) {
                s = rest;
                break;
            }
        }
        if old == s {
            break;
        }
    }
    if [
        "过来蹭",
        "蹭了",
        "蹭蹭",
        "蹭一",
        "靠过来",
        "走过来",
        "走近",
        "抱住",
        "抱紧",
        "抱着你",
        "扑到",
        "扑进",
        "贴过来",
        "凑近",
        "歪头",
        "摇尾",
        "摆尾",
        "尾巴轻",
        "耳朵动",
        "眨眼",
        "眨了",
        "微笑",
        "笑了",
        "笑着",
        "抿嘴",
        "嘟嘴",
        "点头",
        "摇头",
        "伸手",
        "抬手",
        "抬头",
        "低头",
        "脸红",
        "红着脸",
        "看向",
        "望向",
        "打哈欠",
        "伸懒腰",
        "打滚",
        "すり寄",
        "頬を染",
        "手を振",
        "尻尾を",
        "撫で",
        "頭を傾",
    ]
    .iter()
    .any(|p| s.starts_with(p))
    {
        return true;
    }
    let lower = s.to_ascii_lowercase();
    [
        "smiles",
        "smiling",
        "purrs",
        "rubs against",
        "tilts her head",
        "tilts his head",
        "nods",
        "waves",
        "blushes",
        "hugs you",
        "looks at you",
        "stretches",
    ]
    .iter()
    .any(|p| lower.starts_with(p))
}
fn close(c: char) -> Option<char> {
    match c {
        '(' => Some(')'),
        '（' => Some('）'),
        '[' => Some(']'),
        '【' => Some('】'),
        _ => None,
    }
}
pub fn spoken(text: &str) -> String {
    let chars = text.chars().collect::<Vec<_>>();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let Some(end) = close(chars[i]) else {
            out.push(chars[i]);
            i += 1;
            continue;
        };
        let mut stack = vec![end];
        let mut j = i + 1;
        while j < chars.len() {
            if let Some(c) = close(chars[j]) {
                stack.push(c);
            } else if stack.last() == Some(&chars[j]) {
                stack.pop();
                if stack.is_empty() {
                    break;
                }
            }
            j += 1;
        }
        let inner = chars[i + 1..j].iter().collect::<String>();
        if !stage(&inner) {
            out.push(chars[i]);
            out.push_str(&spoken(&inner));
            if j < chars.len() {
                out.push(chars[j]);
            }
        }
        i = if j < chars.len() { j + 1 } else { j };
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stage_directions_are_removed_but_explanations_survive() {
        for (s, expected) in [
            ("喵（过来蹭了一下）", "喵"),
            ("（轻轻地抱住你）明天（周日）见", "明天（周日）见"),
            (
                "你好(smiles)，2 * (3 + 4)，（抱歉）",
                "你好，2 * (3 + 4)，（抱歉）",
            ),
            ("にゃ（すり寄ってくる）", "にゃ"),
            ("说明（动作：眨眼）继续", "说明继续"),
            ("喵（轻轻蹭了蹭（摇尾巴））", "喵"),
            ("记得（例如：微笑这个词）", "记得（例如：微笑这个词）"),
            ("喵（过来蹭了一下", "喵"),
        ] {
            assert_eq!(spoken(s), expected, "{s}");
        }
    }
}
