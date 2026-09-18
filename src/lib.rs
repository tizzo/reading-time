//! Medium-style reading time estimation.
//!
//! The formula follows Medium's published approach, which Confluence Cloud's
//! page read time also tracks:
//!
//!   seconds = words / wpm * 60
//!           + cjk_chars / cjk_cpm * 60
//!           + sum(image_weight(i) for i in 0..images)
//!
//! where the first image is worth 12 seconds and each later one a second less,
//! bottoming out at 3 seconds. Minutes are rounded up, with a floor of 1.

/// Medium's assumed adult reading speed, in words per minute.
pub const DEFAULT_WPM: f64 = 265.0;

/// Medium's assumed speed for Chinese, Japanese and Korean text, in
/// characters per minute.
pub const DEFAULT_CJK_CPM: f64 = 500.0;

const FIRST_IMAGE_SECS: u32 = 12;
const MIN_IMAGE_SECS: u32 = 3;

/// How to interpret the input before counting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Sniff HTML tags; fall back to Markdown handling.
    Auto,
    /// Count every whitespace-separated token as-is.
    Text,
    Markdown,
    Html,
}

#[derive(Debug, Clone, Copy)]
pub struct Options {
    pub wpm: f64,
    pub cjk_cpm: f64,
    pub format: Format,
    /// Add Medium's per-image viewing time.
    pub count_images: bool,
    /// Override the detected image count.
    pub images: Option<usize>,
    /// Drop fenced/indented code blocks and `<pre>`/`<code>` bodies.
    pub skip_code: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            wpm: DEFAULT_WPM,
            cjk_cpm: DEFAULT_CJK_CPM,
            format: Format::Auto,
            count_images: true,
            images: None,
            skip_code: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Estimate {
    pub words: usize,
    pub cjk_chars: usize,
    pub images: usize,
    pub seconds: f64,
    pub minutes: u64,
}

impl Estimate {
    /// The Medium-style byline, e.g. `7 min read`.
    pub fn label(&self) -> String {
        format!("{} min read", self.minutes)
    }
}

/// Seconds Medium credits to the `n`th image (0-indexed).
fn image_weight(n: usize) -> u32 {
    FIRST_IMAGE_SECS
        .saturating_sub(n as u32)
        .max(MIN_IMAGE_SECS)
}

fn images_seconds(count: usize) -> f64 {
    (0..count).map(|i| image_weight(i) as f64).sum()
}

pub fn estimate(input: &str, opts: &Options) -> Estimate {
    let format = match opts.format {
        Format::Auto => {
            if looks_like_html(input) {
                Format::Html
            } else {
                Format::Markdown
            }
        }
        other => other,
    };

    let extracted = match format {
        Format::Text => Extracted {
            text: input.to_string(),
            images: 0,
        },
        Format::Markdown => strip_markdown(input, opts.skip_code),
        Format::Html => strip_html(input, opts.skip_code),
        Format::Auto => unreachable!("resolved above"),
    };

    let (words, cjk_chars) = count(&extracted.text);
    let images = match opts.images {
        Some(n) => n,
        None => extracted.images,
    };

    let mut seconds = words as f64 / opts.wpm * 60.0 + cjk_chars as f64 / opts.cjk_cpm * 60.0;
    if opts.count_images {
        seconds += images_seconds(images);
    }

    let minutes = (seconds / 60.0).ceil().max(1.0) as u64;

    Estimate {
        words,
        cjk_chars,
        images,
        seconds,
        minutes,
    }
}

/// Counts words and CJK characters. CJK characters are read per-character
/// rather than per-word, so they are pulled out of the word count; a token
/// only counts as a word if it still holds an alphanumeric character.
fn count(text: &str) -> (usize, usize) {
    let mut words = 0usize;
    let mut cjk = 0usize;

    for token in text.split_whitespace() {
        let mut has_word_char = false;
        for ch in token.chars() {
            if is_cjk(ch) {
                cjk += 1;
            } else if ch.is_alphanumeric() {
                has_word_char = true;
            }
        }
        if has_word_char {
            words += 1;
        }
    }

    (words, cjk)
}

/// Han, kana and hangul. Deliberately excludes CJK punctuation and the
/// full-width forms block, which carry no reading time of their own.
fn is_cjk(ch: char) -> bool {
    matches!(ch as u32,
        0x1100..=0x11FF   // Hangul Jamo
        | 0x3040..=0x30FF // Hiragana, Katakana
        | 0x3130..=0x318F // Hangul Compatibility Jamo
        | 0x31F0..=0x31FF // Katakana Phonetic Extensions
        | 0x3400..=0x4DBF // CJK Unified Ideographs Extension A
        | 0x4E00..=0x9FFF // CJK Unified Ideographs
        | 0xA960..=0xA97F // Hangul Jamo Extended-A
        | 0xAC00..=0xD7AF // Hangul Syllables
        | 0xF900..=0xFAFF // CJK Compatibility Ideographs
        | 0x20000..=0x2FA1F // CJK Unified Ideographs Extensions B-F
    )
}

struct Extracted {
    text: String,
    images: usize,
}

fn looks_like_html(input: &str) -> bool {
    let probe: String = input.chars().take(4096).collect::<String>().to_lowercase();
    ["<p>", "<p ", "<div", "<br", "<img", "<span", "<h1", "<h2", "<li", "<table", "</"]
        .iter()
        .any(|tag| probe.contains(tag))
}

/// Removes Markdown syntax that is not read aloud: image alt text and link
/// destinations, fence markers, and optionally code bodies. Inline emphasis
/// and heading markers are left alone because they never form a word on their
/// own.
fn strip_markdown(input: &str, skip_code: bool) -> Extracted {
    let mut out = String::with_capacity(input.len());
    let mut images = 0usize;
    let mut in_fence = false;

    for line in input.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence && skip_code {
            continue;
        }
        if skip_code && line.starts_with("    ") && !trimmed.starts_with(['-', '*', '+']) {
            continue;
        }

        let (text, line_images) = strip_markdown_line(line);
        images += line_images;
        out.push_str(&text);
        out.push('\n');
    }

    // HTML images embedded in Markdown still count.
    images += count_img_tags(input);
    let text = strip_tags(&out, skip_code);

    Extracted { text, images }
}

fn strip_markdown_line(line: &str) -> (String, usize) {
    let bytes: Vec<char> = line.chars().collect();
    let mut out = String::with_capacity(line.len());
    let mut images = 0usize;
    let mut i = 0;

    while i < bytes.len() {
        // Image: ![alt](dest) or ![alt][ref] — dropped whole, alt text is not read.
        if bytes[i] == '!' && bytes.get(i + 1) == Some(&'[') {
            if let Some(after_alt) = find_close(&bytes, i + 1, '[', ']') {
                images += 1;
                i = skip_destination(&bytes, after_alt + 1);
                continue;
            }
        }
        // Link: [text](dest) — keep the text, drop the destination.
        if bytes[i] == '[' {
            if let Some(close) = find_close(&bytes, i, '[', ']') {
                out.extend(&bytes[i + 1..close]);
                i = skip_destination(&bytes, close + 1);
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }

    (out, images)
}

/// Index of the matching `close`, honouring nesting.
fn find_close(chars: &[char], open_at: usize, open: char, close: char) -> Option<usize> {
    let mut depth = 0usize;
    for (offset, &ch) in chars[open_at..].iter().enumerate() {
        if ch == open {
            depth += 1;
        } else if ch == close {
            if depth == 0 {
                return None;
            }
            depth -= 1;
            if depth == 0 {
                return Some(open_at + offset);
            }
        }
    }
    None
}

/// Steps past a `(...)` or `[...]` destination that follows a link label.
fn skip_destination(chars: &[char], at: usize) -> usize {
    match chars.get(at) {
        Some('(') => find_close(chars, at, '(', ')').map_or(chars.len(), |end| end + 1),
        Some('[') => find_close(chars, at, '[', ']').map_or(chars.len(), |end| end + 1),
        _ => at,
    }
}

fn strip_html(input: &str, skip_code: bool) -> Extracted {
    Extracted {
        images: count_img_tags(input),
        text: strip_tags(input, skip_code),
    }
}

fn count_img_tags(input: &str) -> usize {
    let lower = input.to_lowercase();
    let mut count = 0usize;
    let mut rest = lower.as_str();
    while let Some(pos) = rest.find("<img") {
        let after = &rest[pos + 4..];
        if after.starts_with([' ', '\t', '\n', '\r', '/', '>']) {
            count += 1;
        }
        rest = after;
    }
    count
}

/// Drops tags and the bodies of elements whose text is never prose.
fn strip_tags(input: &str, skip_code: bool) -> String {
    let mut out = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    let mut dropped: &[&str] = DROPPED_ELEMENTS;
    let with_code: Vec<&str> = DROPPED_ELEMENTS
        .iter()
        .chain(["pre", "code"].iter())
        .copied()
        .collect();
    if skip_code {
        dropped = &with_code;
    }

    while i < chars.len() {
        if chars[i] == '<' {
            // Comments can contain '>', so they are matched on "-->" alone.
            if chars[i..].starts_with(&['<', '!', '-', '-']) {
                match find_sequence(&chars, i + 4, &['-', '-', '>']) {
                    Some(end) => {
                        i = end; // a comment renders as nothing at all
                        continue;
                    }
                    None => break, // unterminated comment: ignore the remainder
                }
            }

            let Some(end) = find_tag_end(&chars, i) else {
                break; // unterminated tag: ignore the remainder
            };
            let tag: String = chars[i + 1..end].iter().collect::<String>().to_lowercase();
            let name: String = tag
                .trim_start_matches('/')
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric())
                .collect();

            if !tag.starts_with('/') && dropped.contains(&name.as_str()) && !tag.ends_with('/') {
                if let Some(close) = find_close_tag(&chars, end + 1, &name) {
                    i = close;
                    out.push(' ');
                    continue;
                }
            }
            i = end + 1;
            if !INLINE_ELEMENTS.contains(&name.as_str()) {
                // Block boundaries separate words; inline ones do not, so
                // `<em>quick</em>er` stays the single word a browser renders.
                out.push(' ');
            }
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }

    decode_entities(&out)
}

/// Elements that do not interrupt a word. A browser renders
/// `<b>super</b><i>man</i>` as one word and `water<sup>1</sup>` as one token,
/// so these tag boundaries emit nothing. `<br>` is deliberately absent: it
/// does break text.
const INLINE_ELEMENTS: &[&str] = &[
    "a", "abbr", "b", "bdi", "bdo", "cite", "code", "data", "del", "dfn", "em", "i", "ins", "kbd",
    "mark", "q", "rp", "rt", "ruby", "s", "samp", "small", "span", "strong", "sub", "sup", "time",
    "u", "var", "wbr",
];

/// Elements whose text is never body prose. `<head>` covers `<title>`,
/// `<meta>` and friends in one go.
const DROPPED_ELEMENTS: &[&str] = &["script", "style", "head", "noscript", "template", "svg"];

/// Index of the `>` closing the tag that opens at `at`, ignoring any `>` that
/// sits inside a quoted attribute value. Falls back to the first bare `>` when
/// the quoting is unbalanced, so one stray quote cannot swallow the document.
fn find_tag_end(chars: &[char], at: usize) -> Option<usize> {
    let mut quote: Option<char> = None;
    for (offset, &ch) in chars[at + 1..].iter().enumerate() {
        match quote {
            Some(q) if ch == q => quote = None,
            Some(_) => {}
            None => match ch {
                '"' | '\'' => quote = Some(ch),
                '>' => return Some(at + 1 + offset),
                _ => {}
            },
        }
    }
    chars[at..].iter().position(|&c| c == '>').map(|p| at + p)
}

/// Index just past `needle`.
fn find_sequence(chars: &[char], from: usize, needle: &[char]) -> Option<usize> {
    if from >= chars.len() {
        return None;
    }
    chars[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| from + p + needle.len())
}

fn find_close_tag(chars: &[char], from: usize, name: &str) -> Option<usize> {
    let needle: Vec<char> = format!("</{name}").chars().collect();
    let mut i = from;
    while i + needle.len() <= chars.len() {
        let matches = chars[i..i + needle.len()]
            .iter()
            .zip(&needle)
            .all(|(a, b)| a.to_ascii_lowercase() == *b);
        if matches {
            return chars[i..].iter().position(|&c| c == '>').map(|p| i + p + 1);
        }
        i += 1;
    }
    None
}

/// Resolves character references. An entity that cannot be resolved becomes a
/// space rather than being left in place, so `&hellip;` is never read as a
/// word.
fn decode_entities(input: &str) -> String {
    if !input.contains('&') {
        return input.to_string();
    }

    let chars: Vec<char> = input.chars().collect();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '&' {
            // Entity names are short; a longer run is just a stray ampersand.
            if let Some(offset) = chars[i + 1..]
                .iter()
                .take(12)
                .position(|&c| c == ';')
                .filter(|&o| o > 0)
            {
                let name: String = chars[i + 1..i + 1 + offset].iter().collect();
                out.push(entity_char(&name).unwrap_or(' '));
                i += offset + 2;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }

    out
}

fn entity_char(name: &str) -> Option<char> {
    if let Some(number) = name.strip_prefix('#') {
        let code = match number.strip_prefix(['x', 'X']) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => number.parse::<u32>().ok()?,
        };
        return char::from_u32(code);
    }

    Some(match name {
        "nbsp" | "ensp" | "emsp" | "thinsp" => ' ',
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" | "ldquo" | "rdquo" => '"',
        "apos" | "lsquo" | "rsquo" => '\'',
        "mdash" => '\u{2014}',
        "ndash" => '\u{2013}',
        "hellip" => '\u{2026}',
        "middot" => '\u{00B7}',
        "bull" => '\u{2022}',
        "copy" => '\u{00A9}',
        "reg" => '\u{00AE}',
        "trade" => '\u{2122}',
        "deg" => '\u{00B0}',
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(n: usize) -> String {
        vec!["word"; n].join(" ")
    }

    #[test]
    fn image_weights_decay_to_three() {
        assert_eq!(image_weight(0), 12);
        assert_eq!(image_weight(1), 11);
        assert_eq!(image_weight(9), 3);
        assert_eq!(image_weight(50), 3);
    }

    #[test]
    fn ten_images_cost_seventy_five_seconds() {
        // 12+11+10+9+8+7+6+5+4+3
        assert_eq!(images_seconds(10), 75.0);
    }

    #[test]
    fn matches_medium_formula() {
        let est = estimate(&words(265), &Options::default());
        assert_eq!(est.words, 265);
        assert!((est.seconds - 60.0).abs() < 1e-9);
        assert_eq!(est.minutes, 1);
    }

    #[test]
    fn rounds_up_with_a_one_minute_floor() {
        assert_eq!(estimate("hello", &Options::default()).minutes, 1);
        assert_eq!(estimate("", &Options::default()).minutes, 1);
        assert_eq!(estimate(&words(266), &Options::default()).minutes, 2);
    }

    #[test]
    fn counts_cjk_by_character() {
        let est = estimate(&"漢字".repeat(250), &Options::default());
        assert_eq!(est.cjk_chars, 500);
        assert_eq!(est.words, 0);
        assert!((est.seconds - 60.0).abs() < 1e-9);
    }

    #[test]
    fn mixed_scripts_split_across_both_rates() {
        let est = estimate("hello 世界 world", &Options::default());
        assert_eq!(est.words, 2);
        assert_eq!(est.cjk_chars, 2);
    }

    #[test]
    fn punctuation_only_tokens_are_not_words() {
        let est = estimate("--- *** | hello", &Options::default());
        assert_eq!(est.words, 1);
    }

    #[test]
    fn markdown_images_count_and_alt_text_does_not() {
        let est = estimate(
            "![a long piece of alt text](cat.png)\n\nHello there friend",
            &Options::default(),
        );
        assert_eq!(est.images, 1);
        assert_eq!(est.words, 3);
    }

    #[test]
    fn markdown_links_keep_text_and_drop_urls() {
        let est = estimate(
            "See [the docs](https://example.com/a/very/long/path) now",
            &Options::default(),
        );
        // "See the docs now" — the URL is not read.
        assert_eq!(est.words, 4);
    }

    #[test]
    fn html_tags_and_scripts_are_stripped() {
        let html = "<p>Hello <b>brave</b> world</p><script>var ignored = 1;</script><img src=x>";
        let est = estimate(html, &Options::default());
        assert_eq!(est.words, 3);
        assert_eq!(est.images, 1);
    }

    #[test]
    fn html_is_autodetected() {
        let est = estimate("<div>one two three</div>", &Options::default());
        assert_eq!(est.words, 3);
    }

    #[test]
    fn adjacent_tags_do_not_glue_words_together() {
        let est = estimate("<p>one</p><p>two</p>", &Options::default());
        assert_eq!(est.words, 2);
    }

    #[test]
    fn entities_become_text() {
        let est = estimate("<p>salt&nbsp;and&nbsp;pepper &amp; more</p>", &Options::default());
        // nbsp splits the words; the bare "&" is punctuation, not a word.
        assert_eq!(est.words, 4);
    }

    #[test]
    fn skip_code_drops_fenced_blocks() {
        let md = "Intro words here\n\n```rust\nfn main() { println!(\"lots of code\"); }\n```\n\nOutro";
        let kept = estimate(md, &Options::default());
        let skipped = estimate(
            md,
            &Options {
                skip_code: true,
                ..Options::default()
            },
        );
        assert!(skipped.words < kept.words);
        assert_eq!(skipped.words, 4);
    }

    #[test]
    fn skip_code_drops_pre_bodies_in_html() {
        let html = "<p>Intro</p><pre>fn main() { lots of code here }</pre>";
        let skipped = estimate(
            html,
            &Options {
                skip_code: true,
                ..Options::default()
            },
        );
        assert_eq!(skipped.words, 1);
    }

    #[test]
    fn text_format_counts_markup_verbatim() {
        let est = estimate(
            "![alt](cat.png)",
            &Options {
                format: Format::Text,
                ..Options::default()
            },
        );
        assert_eq!(est.images, 0);
        assert_eq!(est.words, 1);
    }

    #[test]
    fn images_can_be_overridden_or_disabled() {
        let base = words(265);
        let forced = estimate(
            &base,
            &Options {
                images: Some(3),
                ..Options::default()
            },
        );
        assert!((forced.seconds - (60.0 + 33.0)).abs() < 1e-9);

        let off = estimate(
            &base,
            &Options {
                images: Some(3),
                count_images: false,
                ..Options::default()
            },
        );
        assert!((off.seconds - 60.0).abs() < 1e-9);
    }

    #[test]
    fn custom_wpm_scales_the_estimate() {
        let est = estimate(
            &words(500),
            &Options {
                wpm: 500.0,
                ..Options::default()
            },
        );
        assert!((est.seconds - 60.0).abs() < 1e-9);
    }

    #[test]
    fn nested_brackets_in_link_text_survive() {
        let est = estimate("[a [nested] label](http://x.com) tail", &Options::default());
        assert_eq!(est.words, 4);
    }

    #[test]
    fn inline_elements_do_not_split_a_word() {
        // A browser renders "The quicker fox".
        let est = estimate("<p>The <em>quick</em>er fox</p>", &Options::default());
        assert_eq!(est.words, 3);
    }

    #[test]
    fn footnote_markers_ride_along_with_their_word() {
        let est = estimate("<p>Boiling water<sup>1</sup> matters</p>", &Options::default());
        assert_eq!(est.words, 3);
    }

    #[test]
    fn adjacent_inline_elements_form_one_word() {
        let est = estimate("<p><b>super</b><i>man</i> flies</p>", &Options::default());
        assert_eq!(est.words, 2);
    }

    #[test]
    fn br_still_breaks_words() {
        assert_eq!(estimate("one<br>two", &Options::default()).words, 2);
    }

    #[test]
    fn comment_mid_word_renders_as_nothing() {
        assert_eq!(estimate("foo<!-- x -->bar", &Options::default()).words, 1);
    }

    #[test]
    fn html_comments_are_dropped_even_with_angle_brackets() {
        let est = estimate(
            "<p>one two</p><!-- if a > b then these words are hidden -->",
            &Options::default(),
        );
        assert_eq!(est.words, 2);
    }

    #[test]
    fn attribute_values_are_never_words() {
        let est = estimate(
            r#"<p class="lede intro-copy">one two</p><img src="cat.png" alt="a long description of a cat">"#,
            &Options::default(),
        );
        assert_eq!(est.words, 2);
        assert_eq!(est.images, 1);
    }

    #[test]
    fn angle_bracket_inside_an_attribute_does_not_leak() {
        let est = estimate(r#"<a title="a > b">link text</a>"#, &Options::default());
        assert_eq!(est.words, 2);
    }

    #[test]
    fn unbalanced_quote_does_not_swallow_the_document() {
        let est = estimate("<p class=\"oops>one two three</p>", &Options::default());
        assert_eq!(est.words, 3);
    }

    #[test]
    fn head_content_is_not_prose() {
        let html = "<html><head><title>Page Title Here</title><meta name=x></head>\
                    <body><p>real prose here</p></body></html>";
        let est = estimate(html, &Options::default());
        assert_eq!(est.words, 3);
    }

    #[test]
    fn numeric_and_unknown_entities_are_not_words() {
        let est = estimate("<p>one two &#8212; three &hellip; four</p>", &Options::default());
        assert_eq!(est.words, 4);
    }

    #[test]
    fn entities_inside_a_word_keep_it_one_word() {
        let est = estimate("<p>don&#8217;t stop</p>", &Options::default());
        assert_eq!(est.words, 2);
    }

    #[test]
    fn stray_ampersand_is_left_alone() {
        let est = estimate("<p>salt & pepper</p>", &Options::default());
        assert_eq!(est.words, 2);
    }

    #[test]
    fn unterminated_comment_ignores_the_remainder() {
        let est = estimate("<p>one two</p><!-- dangling words here", &Options::default());
        assert_eq!(est.words, 2);
    }

    #[test]
    fn svg_internals_are_dropped_but_captions_survive() {
        let est = estimate(
            "<figure><svg><text>M0 L10</text></svg><figcaption>A caption here</figcaption></figure>",
            &Options::default(),
        );
        assert_eq!(est.words, 3);
    }

    #[test]
    fn unterminated_tag_does_not_panic() {
        let est = estimate("<p>hello <span class=", &Options::default());
        assert_eq!(est.words, 1);
    }
}
