/// Convert markdown text to Telegram-compatible HTML.
///
/// Handles: code blocks, inline code, bold, italic, links, headings, bullet lists.
/// HTML special characters in plain text are escaped.
pub fn md_to_telegram_html(md: &str) -> String {
    let mut result = String::with_capacity(md.len());
    let mut lines = md.lines().peekable();

    while let Some(line) = lines.next() {
        // Code block
        if line.starts_with("```") {
            if !result.is_empty() && !result.ends_with('\n') {
                result.push('\n');
            }
            let lang = line.trim_start_matches('`').trim();
            let mut code = String::new();
            for code_line in lines.by_ref() {
                if code_line.starts_with("```") {
                    break;
                }
                if !code.is_empty() {
                    code.push('\n');
                }
                code.push_str(code_line);
            }
            let escaped_code = escape_html(&code);
            if lang.is_empty() {
                result.push_str(&format!("<pre><code>{escaped_code}</code></pre>"));
            } else {
                result.push_str(&format!(
                    "<pre><code class=\"language-{lang}\">{escaped_code}</code></pre>"
                ));
            }
        } else {
            if !result.is_empty() && !result.ends_with('\n') {
                result.push('\n');
            }
            result.push_str(&convert_line(line));
        }
    }

    result
}

/// Split markdown text into chunks that fit within Telegram's 4096-char limit
/// after HTML conversion. Splits at paragraph/line boundaries in the markdown
/// source at the 3000-char mark, then converts each chunk independently.
pub fn split_and_convert(md: &str) -> Vec<String> {
    const SPLIT_AT: usize = 3000;
    const MAX_HTML: usize = 4096;

    if md.len() <= SPLIT_AT {
        let html = md_to_telegram_html(md);
        if html.len() <= MAX_HTML {
            return vec![html];
        }
    }

    let mut chunks = Vec::new();
    let mut remaining = md;

    while !remaining.is_empty() {
        if remaining.len() <= SPLIT_AT {
            chunks.push(remaining.to_string());
            break;
        }

        let search_region = &remaining[..SPLIT_AT];
        let split_pos = search_region
            .rfind("\n\n")
            .map(|p| p + 2)
            .or_else(|| search_region.rfind('\n').map(|p| p + 1))
            .unwrap_or(SPLIT_AT);

        chunks.push(remaining[..split_pos].to_string());
        remaining = &remaining[split_pos..];
    }

    chunks
        .into_iter()
        .map(|chunk| {
            let html = md_to_telegram_html(&chunk);
            if html.len() <= MAX_HTML {
                html
            } else {
                html[..MAX_HTML].to_string()
            }
        })
        .collect()
}

fn convert_line(line: &str) -> String {
    // Headings
    for prefix in &["### ", "## ", "# "] {
        if let Some(heading) = line.strip_prefix(prefix) {
            return format!("<b>{}</b>", escape_html(heading));
        }
    }

    // Bullet lists
    if let Some(item) = line.strip_prefix("- ") {
        return format!("\u{2022} {}", convert_inline(&escape_html(item)));
    }
    if let Some(item) = line.strip_prefix("* ") {
        return format!("\u{2022} {}", convert_inline(&escape_html(item)));
    }

    convert_inline(&escape_html(line))
}

/// Apply inline formatting: `code`, **bold**, *italic*, [links](url).
/// Input text must already be HTML-escaped.
fn convert_inline(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        // Inline code
        if chars[i] == '`' {
            if let Some(end) = find_char(&chars, '`', i + 1) {
                result.push_str("<code>");
                result.extend(&chars[i + 1..end]);
                result.push_str("</code>");
                i = end + 1;
                continue;
            }
        }

        // Bold **...**
        if i + 1 < len && chars[i] == '*' && chars[i + 1] == '*' {
            if let Some(end) = find_double_char(&chars, '*', i + 2) {
                result.push_str("<b>");
                result.extend(&chars[i + 2..end]);
                result.push_str("</b>");
                i = end + 2;
                continue;
            }
        }

        // Italic *...*
        if chars[i] == '*' && (i + 1 < len && chars[i + 1] != '*') {
            if let Some(end) = find_char(&chars, '*', i + 1) {
                result.push_str("<i>");
                result.extend(&chars[i + 1..end]);
                result.push_str("</i>");
                i = end + 1;
                continue;
            }
        }

        // Links [text](url)
        if chars[i] == '[' {
            if let Some((link_text, url, end_pos)) = parse_link(&chars, i) {
                result.push_str(&format!("<a href=\"{url}\">{link_text}</a>"));
                i = end_pos;
                continue;
            }
        }

        result.push(chars[i]);
        i += 1;
    }

    result
}

fn find_char(chars: &[char], target: char, start: usize) -> Option<usize> {
    for i in start..chars.len() {
        if chars[i] == target {
            return Some(i);
        }
    }
    None
}

fn find_double_char(chars: &[char], target: char, start: usize) -> Option<usize> {
    for i in start..chars.len().saturating_sub(1) {
        if chars[i] == target && chars[i + 1] == target {
            return Some(i);
        }
    }
    None
}

fn parse_link(chars: &[char], start: usize) -> Option<(String, String, usize)> {
    // Expect [text](url) starting at chars[start] == '['
    let close_bracket = find_char(chars, ']', start + 1)?;
    if close_bracket + 1 >= chars.len() || chars[close_bracket + 1] != '(' {
        return None;
    }
    let close_paren = find_char(chars, ')', close_bracket + 2)?;

    let text: String = chars[start + 1..close_bracket].iter().collect();
    let url: String = chars[close_bracket + 2..close_paren].iter().collect();
    Some((text, url, close_paren + 1))
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inline_code() {
        assert_eq!(
            md_to_telegram_html("use `foo` here"),
            "use <code>foo</code> here"
        );
    }

    #[test]
    fn test_code_block() {
        let input = "before\n```rust\nfn main() {}\n```\nafter";
        let expected =
            "before\n<pre><code class=\"language-rust\">fn main() {}</code></pre>\nafter";
        assert_eq!(md_to_telegram_html(input), expected);
    }

    #[test]
    fn test_code_block_no_lang() {
        let input = "```\nplain code\n```";
        let expected = "<pre><code>plain code</code></pre>";
        assert_eq!(md_to_telegram_html(input), expected);
    }

    #[test]
    fn test_bold() {
        assert_eq!(
            md_to_telegram_html("this is **bold** text"),
            "this is <b>bold</b> text"
        );
    }

    #[test]
    fn test_italic() {
        assert_eq!(
            md_to_telegram_html("this is *italic* text"),
            "this is <i>italic</i> text"
        );
    }

    #[test]
    fn test_link() {
        assert_eq!(
            md_to_telegram_html("click [here](https://example.com)"),
            "click <a href=\"https://example.com\">here</a>"
        );
    }

    #[test]
    fn test_heading() {
        assert_eq!(md_to_telegram_html("# Title"), "<b>Title</b>");
        assert_eq!(md_to_telegram_html("## Subtitle"), "<b>Subtitle</b>");
    }

    #[test]
    fn test_bullet_list() {
        assert_eq!(
            md_to_telegram_html("- item one\n- item two"),
            "\u{2022} item one\n\u{2022} item two"
        );
        assert_eq!(md_to_telegram_html("* item one"), "\u{2022} item one");
    }

    #[test]
    fn test_html_escaping() {
        assert_eq!(
            md_to_telegram_html("a < b & c > d"),
            "a &lt; b &amp; c &gt; d"
        );
    }

    #[test]
    fn test_code_block_no_formatting_inside() {
        let input = "```\na < b\n```";
        let expected = "<pre><code>a &lt; b</code></pre>";
        assert_eq!(md_to_telegram_html(input), expected);
    }

    #[test]
    fn test_split_short_message() {
        let short = "Hello world";
        let chunks = split_and_convert(short);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], "Hello world");
    }

    #[test]
    fn test_split_long_message() {
        let para = "A".repeat(1500);
        let input = format!("{para}\n\n{para}\n\n{para}");
        let chunks = split_and_convert(&input);
        assert!(chunks.len() >= 2);
        for chunk in &chunks {
            assert!(chunk.len() <= 4096, "Chunk too long: {} chars", chunk.len());
        }
    }
}
