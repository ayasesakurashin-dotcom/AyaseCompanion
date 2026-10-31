//! Markdown is data, never executable HTML. Produce one stable body before pagination/TTS.
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

pub fn plain(source: &str) -> String {
    let source = source.replace("\r\n", "\n");
    let mut out = String::new();
    let mut lists: Vec<Option<u64>> = vec![];
    let mut cell = 0;
    fn boundary(out: &mut String, count: usize) {
        if out.is_empty() {
            return;
        }
        let existing = out.chars().rev().take_while(|c| *c == '\n').count();
        for _ in existing..count {
            out.push('\n');
        }
    }
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    for event in Parser::new_ext(&source, options) {
        match event {
            Event::Start(Tag::Heading { .. } | Tag::CodeBlock(_) | Tag::BlockQuote(_)) => {
                boundary(&mut out, 2)
            }
            Event::Start(Tag::List(start)) => {
                boundary(&mut out, 1);
                lists.push(start);
            }
            Event::Start(Tag::Item) => {
                boundary(&mut out, 1);
                out.push_str(&"  ".repeat(lists.len().saturating_sub(1)));
                if let Some(Some(n)) = lists.last_mut() {
                    out.push_str(&format!("{n}. "));
                    *n += 1;
                } else {
                    out.push_str("• ");
                }
            }
            Event::Start(Tag::TableHead | Tag::TableRow) => {
                boundary(&mut out, 1);
                cell = 0;
            }
            Event::Start(Tag::TableCell) => {
                if cell > 0 {
                    out.push_str("；");
                }
                cell += 1;
            }
            Event::End(TagEnd::List(_)) => {
                lists.pop();
                boundary(&mut out, 2);
            }
            Event::End(TagEnd::Item | TagEnd::TableHead | TagEnd::TableRow) => {
                boundary(&mut out, 1)
            }
            Event::End(TagEnd::Paragraph) => {
                boundary(&mut out, if lists.is_empty() { 2 } else { 1 })
            }
            Event::End(
                TagEnd::Heading(_) | TagEnd::CodeBlock | TagEnd::BlockQuote(_) | TagEnd::Table,
            ) => boundary(&mut out, 2),
            Event::Text(text)
            | Event::Code(text)
            | Event::Html(text)
            | Event::InlineMath(text)
            | Event::DisplayMath(text) => out.push_str(&text),
            Event::SoftBreak | Event::HardBreak => out.push('\n'),
            Event::Rule => boundary(&mut out, 2),
            Event::TaskListMarker(checked) => out.push_str(if checked { "☑ " } else { "☐ " }),
            _ => {}
        }
    }
    out.trim_matches('\n')
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect()
}

/// A hard character budget must not silently leave a half-sentence at the end.
pub fn bounded_reply(text: &str, max: usize, incomplete: bool) -> String {
    if text.trim().is_empty() {
        return String::new();
    }
    let all = text.chars().collect::<Vec<_>>();
    if all.len() <= max && !incomplete {
        return text.to_owned();
    }
    let prefix = all.iter().take(max).collect::<String>();
    let mut end = None;
    let mut depth = 0i32;
    for (i, c) in prefix.char_indices() {
        if "（([【".contains(c) {
            depth += 1;
        } else if "）)]】".contains(c) {
            depth = (depth - 1).max(0);
        }
        if depth == 0 && "。！？!?；;\n".contains(c) {
            end = Some(i + c.len_utf8());
        }
        if depth == 0
            && c == '.'
            && prefix[i + c.len_utf8()..]
                .chars()
                .next()
                .is_none_or(|c| c.is_whitespace())
        {
            end = Some(i + 1);
        }
    }
    if let Some(end) = end {
        let result = prefix[..end].trim_end();
        if !result.is_empty() {
            return result.into();
        }
    }
    if max == 0 {
        return String::new();
    }
    let prefix = all.iter().take(max.saturating_sub(1)).collect::<String>();
    format!(
        "{}…",
        prefix.trim_end_matches([' ', '\n', ',', '，', ':', '：'])
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reply_limits_keep_a_complete_sentence_or_mark_an_excerpt() {
        assert_eq!(
            bounded_reply("先说完。然后是被截断的后半句", 8, false),
            "先说完。"
        );
        assert_eq!(
            bounded_reply("没有可用完整句子的很长内容", 5, false),
            "没有可用…"
        );
        assert_eq!(
            bounded_reply("这是一段中断内容", 100, true),
            "这是一段中断内容…"
        );
        assert_eq!(
            bounded_reply("明天（周日）见。", 100, false),
            "明天（周日）见。"
        );
        assert_eq!(bounded_reply("long content", 1, false), "…");
        assert_eq!(
            bounded_reply("第一句（这里有。括号内容）仍未结束", 12, false)
                .chars()
                .count(),
            12
        );
    }
    #[test]
    fn headings_lists_emphasis_links_and_blank_lines_become_readable() {
        let text=plain("# 项目摘要\n\n\n\n项目 **已完成**，保留 38% 和 🙂。\n\n1. **第一项**：说明。\n2. [第二项](https://example.org)：补充。\n\n---\n\n结束。");
        assert!(text.starts_with("项目摘要\n\n项目 已完成"));
        for keep in ["1. 第一项：说明。", "2. 第二项：补充。", "38%", "🙂"] {
            assert!(text.contains(keep));
        }
        for raw in ["# ", "**", "https://example.org", "\n\n\n"] {
            assert!(!text.contains(raw));
        }
    }
    #[test]
    fn code_and_literal_symbols_are_kept_without_fences_or_language_names() {
        let text=plain("```python\n  a_b = 2 * 3\n\n  print('<b>text</b>')\n```\n\n`x*y*z`，2 * 3，snake_case 和 C#。");
        assert!(text.starts_with("  a_b = 2 * 3\n\n  print('<b>text</b>')"));
        assert!(text.contains("x*y*z，2 * 3，snake_case 和 C#。"));
        assert!(!text.contains("```"));
    }
    #[test]
    fn table_cells_and_paragraphs_keep_boundaries() {
        assert_eq!(
            plain("| 名称 | 状态 |\n| --- | --- |\n| A | 完成 |\n\n下一段。"),
            "名称；状态\nA；完成\n\n下一段。"
        );
    }
}
