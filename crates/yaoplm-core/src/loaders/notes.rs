//! Release texts and checksum files: someone else's data (ADR-0038). The
//! change list a user sees is plain text, at most ten items, and keeps
//! links only to github.com; everything else is dropped, never rendered.
//!
//! Shapes seen on 2026-10-25 (research/17 §5–§7): upstream `latest` is a
//! heading and bullets ending "by @author in <commit link>"; `v1.1.0` is a
//! long text of sections and bullets; YAOPL's is three service lines;
//! RiptOPL's wraps its list in `<details>`.

/// At most this many items are shown (SPEC §5.10.2).
pub const MAX_ITEMS: usize = 10;
/// Longer items are cut.
const MAX_ITEM_CHARS: usize = 200;
/// Lines about the build itself, not about what changed.
const SERVICE: [&str; 6] = [
    "sha256sums",
    "sha-256",
    "checksum",
    "full changelog",
    "automatic ",
    "not an official",
];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Changelog {
    /// Empty: "No change list was published".
    pub items: Vec<ChangelogItem>,
    /// The release lists more than [`MAX_ITEMS`].
    pub more: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangelogItem {
    pub text: String,
    /// A github.com link of the item: its commit or pull request, else the
    /// first one.
    pub link: Option<String>,
}

/// `https://github.com/…` and nothing that only looks like it.
pub fn is_github_link(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://github.com") else {
        return false;
    };
    (rest.is_empty() || rest.starts_with('/'))
        && !url
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '"' || c == '<')
}

/// Removes HTML tags: `<details>`, `<summary>`, `<br>`.
fn strip_tags(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut in_tag = false;
    for c in line.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

/// The text of a list item, or `None` for any other line.
fn list_item(line: &str) -> Option<&str> {
    if let Some(rest) = line
        .strip_prefix("- ")
        .or_else(|| line.strip_prefix("* "))
        .or_else(|| line.strip_prefix("+ "))
    {
        return Some(rest);
    }
    let digits = line.bytes().take_while(u8::is_ascii_digit).count();
    (digits > 0)
        .then(|| line[digits..].strip_prefix(". "))
        .flatten()
}

/// Markdown links become their text; bare URLs go. The first github.com
/// target is kept as the item's link.
fn plain(text: &str) -> (String, Option<String>) {
    let mut out = String::new();
    let mut link = None;
    let mut rest = text;
    // A commit or pull request says more than an author's profile.
    let keep = |url: &str, link: &mut Option<String>| {
        let change = |u: &str| u.contains("/commit/") || u.contains("/pull/");
        if is_github_link(url)
            && (link.is_none() || (change(url) && !link.as_deref().is_some_and(change)))
        {
            *link = Some(url.to_owned());
        }
    };
    while !rest.is_empty() {
        if let Some(inner) = rest.strip_prefix('[')
            && let Some(close) = inner.find("](")
            && let Some(end) = inner[close + 2..].find(')')
        {
            out.push_str(&inner[..close]);
            keep(&inner[close + 2..close + 2 + end], &mut link);
            rest = &inner[close + 2 + end + 1..];
            continue;
        }
        if rest.starts_with("http://") || rest.starts_with("https://") {
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            let url = rest[..end].trim_end_matches(['.', ',', ')', ';']);
            keep(url, &mut link);
            rest = &rest[url.len()..];
            continue;
        }
        let mut chars = rest.chars();
        if let Some(c) = chars.next()
            && !matches!(c, '*' | '_' | '`')
        {
            out.push(c);
        }
        rest = chars.as_str();
    }
    let mut text = out.split_whitespace().collect::<Vec<_>>().join(" ");
    // "by @someone in" with the link gone leaves a dangling "in".
    for tail in [" in", " in:"] {
        if let Some(cut) = text.strip_suffix(tail) {
            text = cut.to_owned();
        }
    }
    if text.chars().count() > MAX_ITEM_CHARS {
        text = text.chars().take(MAX_ITEM_CHARS - 1).collect::<String>() + "…";
    }
    (text, link)
}

fn service(text: &str) -> bool {
    let lower = text.to_lowercase();
    SERVICE.iter().any(|s| lower.contains(s))
}

/// The start of a release text as plain items (SPEC §5.10.2).
pub fn changelog(body: &str) -> Changelog {
    let lines: Vec<String> = body
        .lines()
        .map(|l| strip_tags(l).trim().to_owned())
        .filter(|l| {
            !l.is_empty() && !l.starts_with('#') && !l.starts_with("---") && !l.starts_with('|')
        })
        .collect();
    let bullets: Vec<&str> = lines.iter().filter_map(|l| list_item(l)).collect();
    // A text with a list is its list; one without is its lines.
    let raw: Vec<&str> = if bullets.is_empty() {
        lines.iter().map(String::as_str).collect()
    } else {
        bullets
    };
    let mut items = Vec::new();
    let mut more = false;
    for line in raw {
        let (text, link) = plain(line);
        if text.is_empty() || service(&text) {
            continue;
        }
        if items.len() == MAX_ITEMS {
            more = true;
            break;
        }
        items.push(ChangelogItem { text, link });
    }
    Changelog { items, more }
}

/// The SHA-256 a `sha256sum` file gives `name`: `<hex>  <name>` or
/// `<hex> *<name>` (binary mode).
pub fn sums_line(text: &str, name: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (hex, file) = line.trim().split_once(char::is_whitespace)?;
        let file = file.trim_start().trim_start_matches('*');
        (file == name && hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
            .then(|| hex.to_ascii_lowercase())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(log: &Changelog) -> Vec<&str> {
        log.items.iter().map(|i| i.text.as_str()).collect()
    }

    #[test]
    fn upstream_bullets_keep_their_commit_link() {
        let body = "## Documentation\n* Add note about different projects by @someone in https://github.com/ps2homebrew/Open-PS2-Loader/commit/b3a2199\n* Fix `games.bin` **sizes** by [@other](https://github.com/other) in https://github.com/ps2homebrew/Open-PS2-Loader/pull/1700\n";
        let log = changelog(body);
        assert_eq!(
            texts(&log),
            [
                "Add note about different projects by @someone",
                "Fix games.bin sizes by @other"
            ]
        );
        assert_eq!(
            log.items[0].link.as_deref(),
            Some("https://github.com/ps2homebrew/Open-PS2-Loader/commit/b3a2199")
        );
        assert_eq!(
            log.items[1].link.as_deref(),
            Some("https://github.com/ps2homebrew/Open-PS2-Loader/pull/1700")
        );
        assert!(!log.more);
    }

    #[test]
    fn only_github_links_survive() {
        let log = changelog(
            "- See [the site](https://example.org/x) and https://evil.example/y\n- Mirror https://github.com.evil.org/z\n",
        );
        assert_eq!(texts(&log), ["See the site and", "Mirror"]);
        assert!(log.items.iter().all(|i| i.link.is_none()));
        assert!(is_github_link("https://github.com/a/b"));
        assert!(!is_github_link("https://github.com.evil.org/a"));
        assert!(!is_github_link("http://github.com/a"));
        assert!(!is_github_link("https://github.com/a\"onclick"));
    }

    #[test]
    fn html_goes_and_ten_items_stay() {
        let mut body = String::from("**Rolling build** <details><summary>Changelog</summary>\n\n");
        for n in 1..=12 {
            body.push_str(&format!("- change {n}\n"));
        }
        body.push_str("</details>\n");
        let log = changelog(&body);
        assert_eq!(log.items.len(), MAX_ITEMS);
        assert_eq!(log.items[0].text, "change 1");
        assert!(log.more);
    }

    #[test]
    fn service_lines_are_no_change_list() {
        let body = "Automatic YAOPL (Yet Another Open PS2 Loader) build of v1.2.0-Beta-2370-33a2b19-yaopl from 33a2b19a.\n\nNot an official OPL build: see FORK.md for what differs from upstream OPL.\nSHA-256 checksums of the files are in SHA256SUMS.\n";
        assert!(changelog(body).items.is_empty());
        assert!(changelog("").items.is_empty());
        assert!(
            changelog("**Full Changelog**: https://github.com/o/r/compare/a...b")
                .items
                .is_empty()
        );
    }

    #[test]
    fn a_text_without_a_list_is_its_lines() {
        let log = changelog("New in OPL v1.1.0\nGCC 11 toolchain.\n");
        assert_eq!(texts(&log), ["New in OPL v1.1.0", "GCC 11 toolchain."]);
        let long = "x".repeat(500);
        let item = &changelog(&format!("- {long}")).items[0].text;
        assert_eq!(item.chars().count(), 200);
        assert!(item.ends_with('…'));
    }

    #[test]
    fn sums_lines() {
        let sha = "a".repeat(64);
        let text = format!(
            "{sha}  OPNPS2LD.ELF\n{}  *OPNPS2LD-v1.7z\nnot a line\n",
            "B".repeat(64)
        );
        assert_eq!(sums_line(&text, "OPNPS2LD.ELF"), Some(sha));
        assert_eq!(sums_line(&text, "OPNPS2LD-v1.7z"), Some("b".repeat(64)));
        assert_eq!(sums_line(&text, "OPNPS2LD.7z"), None);
    }
}
