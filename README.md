# readtime

A `wc`-shaped CLI that estimates how long a piece of text takes to read, using
Medium's published read-time formula.

```console
$ cat article.md | readtime
7 min read
```

## Install

```console
cargo install --path .    # or: cargo build --release
```

## Usage

```
cat article.md | readtime [OPTIONS]
readtime [OPTIONS] [FILE]...

-w, --wpm <N>        Words per minute (default: 265, Medium's figure)
    --cjk-cpm <N>    Characters per minute for CJK text (default: 500)
-f, --format <FMT>   auto (default), text, markdown, html
-i, --images <N>     Use N images instead of the detected count
    --no-images      Ignore image viewing time
    --skip-code      Exclude code blocks and <pre>/<code> bodies
-s, --seconds        Print raw seconds
-m, --minutes        Print just the minute count
    --json           Print a JSON object
-v, --verbose        Print a breakdown of the estimate
```

With no `FILE`, or with `FILE` of `-`, it reads standard input. Several files
are reported one per line with a total, like `wc`.

```console
$ readtime -v posts/*.md
3 min read  posts/one.md
  742 words at 265 wpm, 2 images  (191.0s total)
...

$ cat article.md | readtime --json
{"minutes":7,"seconds":382.19,"words":1602,"cjk_chars":0,"images":4,"wpm":265,"text":"7 min read"}
```

## The algorithm

```
seconds = words / 265 * 60
        + cjk_chars / 500 * 60
        + image_time
minutes = max(1, ceil(seconds / 60))
```

Images follow Medium's decay: the first is worth 12 seconds and each later one
a second less, with a floor of 3 seconds — so 12, 11, 10 … 3, 3, 3. Ten or
more images is 75 seconds plus 3 for each extra.

### Where the numbers come from

- **265 wpm** is Medium's stated assumption for average adult reading speed,
  and the same figure Atlassian quotes for Confluence Cloud's page read time.
- **The image decay (12s → 3s)** is Medium's; Atlassian says Confluence
  "factors in some additional time for images" without publishing the curve,
  so this uses Medium's.
- **500 characters per minute** is Medium's rate for Chinese, Japanese and
  Korean text, which is counted per character rather than per word. Hugo uses
  the same figure for CJK.
- **Round up, minimum one minute** matches Medium's output — nothing is ever
  "0 min read".

### Counting rules

A token counts as a word if it contains at least one alphanumeric character,
so bullets, table pipes and stray punctuation are free. CJK characters are
pulled out of their tokens and billed at the character rate, which makes mixed
English/CJK text come out sensibly.

In `markdown` mode, image alt text and link destinations are dropped (neither
is read) while link text is kept. `auto` sniffs for HTML tags and otherwise
treats the input as Markdown. Use `text` to count raw tokens with no
preprocessing.

### HTML

Tags never reach the word counter. Everything between `<` and `>` is
discarded, so attributes cost nothing — `<img src="cat.png" alt="a long
description">` is worth zero words. Specifically:

- **Non-prose elements are skipped whole**, not just their tags: `<script>`,
  `<style>`, `<head>` (which covers `<title>` and `<meta>`), `<noscript>`,
  `<template>` and `<svg>`. With `--skip-code`, `<pre>` and `<code>` join them.
- **Tag ends are quote-aware**, so a `>` inside an attribute value
  (`<a title="a > b">`) does not end the tag early and leak `b">` as a word.
  Unbalanced quotes fall back to the first bare `>` rather than swallowing the
  rest of the document.
- **Comments are matched on `-->`**, so `<!-- if a > b then ... -->` is dropped
  entirely rather than leaking its tail.
- **Character references are resolved**: named entities, plus numeric `&#8212;`
  and hex `&#x27;`. An unresolvable entity becomes a space, so `&hellip;` is
  never counted as a word. `don&#8217;t` stays one word; `&nbsp;` correctly
  splits two.
- **Block boundaries separate words; inline ones do not.** `<p>one</p><p>two</p>`
  is two words, but `<b>super</b><i>man</i>` is one and `water<sup>1</sup>` is
  one — matching what a browser actually renders. `<br>` does break. Comments
  render as nothing, so `foo<!-- x -->bar` is one word.

Two things it deliberately does not do. It is a stripper, not a parser, so it
has no notion of the article body: pipe a full web page and site chrome
(`<nav>`, `<footer>`, sidebars, reference lists) is counted as prose. Feed it
article HTML — or the output of a readability extractor — for a figure that
matches what Medium would show. And it counts `<img>` tags only; CSS background
images and `<picture>`/`<video>` posters are invisible to it, so use
`--images N` when you know better.

### Why a scanner and not an html5ever parser

This was measured rather than assumed. A `scraper`/`html5ever` implementation of
the same counting rules costs 46 transitive crates, a 1.3 MB binary (vs 393 KB)
and a ~13s cold build (vs ~1s), and is no faster at runtime — both finish a
623 KB page in about 10 ms.

What it buys in accuracy, on two real Wikipedia pages:

| Page | html5ever | this scanner | delta |
|---|---|---|---|
| *Reading* (623 KB) | 7477 words | 7489 | 0.16% |
| *Rust (programming language)* | 10942 words | 10992 | 0.46% |

Both round to the same number of minutes. The one place a parser was genuinely
ahead was the inline/block distinction, which was worth 2.7% — so that got
ported over as a table of inline element names instead of a dependency. What is
left is sub-0.5% noise against a 265 wpm model whose own error for a real human
reader is tens of percent.

A parser earns its keep the moment this needs to reason about the *structure* of
a document — extracting the article body out of site chrome, honouring
`display:none`, or handling implied tag closing in genuinely broken markup. If
that day comes, `lol_html` or `html5ever` is the right call. Counting words is
not that day.

Code blocks are **counted** by default, matching Confluence — which is exactly
[the complaint](https://jira.atlassian.com/browse/CONFSERVER-98219) people have
about it, since a page of code reads as an 83-minute epic. Pass `--skip-code`
if you would rather exclude them. Confluence's other known gap, not counting
text inside macros, has no equivalent here.

## Development

```console
cargo test
cargo clippy --all-targets
```
