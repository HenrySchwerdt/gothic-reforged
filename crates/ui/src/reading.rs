//! Text flow of the readable documents (`gCHUDViewBook2`, `gCHUDViewLetter2`): wrapping a text
//! into lines and spreading the lines over book pages.
//!
//! A book shows two columns per spread (left text first, then right text, then the next
//! spread); a letter is one scrolling column. Widths are estimated from the character count
//! (the renderer does the final layout), which is accurate enough to decide where to flip.
use crate::dialogmenu::approx_text_width;

/// Wrap `text` into lines no wider than `width` pixels at font size `px`. Paragraph breaks
/// (`\n`, and the literal `\n` sequence the string tables use) are kept; an empty line is
/// kept as a blank line.
pub fn wrap(text: &str, width: f32, px: f32) -> Vec<String> {
    let text = text.replace("\\n", "\n").replace('\r', "");
    let mut out = Vec::new();
    for para in text.split('\n') {
        if para.trim().is_empty() {
            out.push(String::new());
            continue;
        }
        let mut line = String::new();
        for word in para.split_whitespace() {
            let candidate = if line.is_empty() {
                word.to_string()
            } else {
                format!("{line} {word}")
            };
            if !line.is_empty() && approx_text_width(&candidate, px) > width {
                out.push(std::mem::take(&mut line));
                line = word.to_string();
            } else {
                line = candidate;
            }
        }
        out.push(line);
    }
    out
}

/// Split wrapped lines into pages of `lines_per_page`; a page never starts with a blank line.
pub fn paginate(lines: &[String], lines_per_page: usize) -> Vec<Vec<String>> {
    let per = lines_per_page.max(1);
    let mut pages: Vec<Vec<String>> = Vec::new();
    let mut cur: Vec<String> = Vec::new();
    for l in lines {
        if cur.is_empty() && l.is_empty() {
            continue;
        }
        cur.push(l.clone());
        if cur.len() == per {
            pages.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        pages.push(cur);
    }
    pages
}

/// The pages of a book: the left text fills the pages first, the right text continues after
/// it, so a spread shows pages `2n` and `2n + 1`.
pub fn book_pages(
    left: &str,
    right: &str,
    width: f32,
    px: f32,
    lines_per_page: usize,
) -> Vec<Vec<String>> {
    let mut pages = paginate(&wrap(left, width, px), lines_per_page);
    // the right-hand text starts on the right-hand page of a spread
    if pages.len() % 2 == 1 && !right.trim().is_empty() {
        pages.push(Vec::new());
        let at = pages.len() - 1;
        let mut r = paginate(&wrap(right, width, px), lines_per_page);
        if !r.is_empty() {
            pages[at] = r.remove(0);
        }
        pages.extend(r);
    } else {
        pages.extend(paginate(&wrap(right, width, px), lines_per_page));
    }
    pages
}

/// Number of spreads for `pages` pages.
pub fn spreads(pages: usize) -> usize {
    pages.div_ceil(2).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_breaks_at_width_and_keeps_paragraphs() {
        // 10 px font => 5 px per char; width 50 => 10 chars per line
        let lines = wrap("aaaa bbbb cccc\\n\ndddd", 50.0, 10.0);
        assert_eq!(lines, ["aaaa bbbb", "cccc", "", "dddd"]);
    }

    #[test]
    fn long_word_stays_on_its_own_line() {
        let lines = wrap("x supercalifragilistic y", 50.0, 10.0);
        assert_eq!(lines, ["x", "supercalifragilistic", "y"]);
    }

    #[test]
    fn pages_do_not_start_with_blank_lines_and_fill_to_the_limit() {
        let lines: Vec<String> = ["a", "b", "", "c", "d"].map(String::from).to_vec();
        let p = paginate(&lines, 2);
        assert_eq!(p, vec![vec!["a", "b"], vec!["c", "d"]]);
        assert!(paginate(&[], 3).is_empty());
    }

    #[test]
    fn right_text_starts_on_a_right_hand_page() {
        // left text fills exactly one page => the right text goes on page 1 of the spread
        let pages = book_pages("aaaa bbbb", "cccc", 50.0, 10.0, 5);
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0], ["aaaa bbbb"]);
        assert_eq!(pages[1], ["cccc"]);
        assert_eq!(spreads(pages.len()), 1);
        assert_eq!(spreads(3), 2);
        assert_eq!(spreads(0), 1);
    }
}
