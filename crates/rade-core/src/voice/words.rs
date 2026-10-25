//! Which words of a revised Voice Draft differ from the one before (D11):
//! a word-level longest common subsequence.

use std::ops::Range;

/// Whitespace-separated words of `s` with their byte ranges.
fn words(s: &str) -> Vec<(Range<usize>, &str)> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, c) in s.char_indices() {
        match (c.is_whitespace(), start) {
            (false, None) => start = Some(i),
            (true, Some(st)) => {
                out.push((st..i, &s[st..i]));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(st) = start {
        out.push((st..s.len(), &s[st..]));
    }
    out
}

/// Byte ranges in `new` of the words that are not part of the longest common
/// word sequence with `old`. Neighbouring changed words on one line are one
/// range.
pub fn changed_spans(old: &str, new: &str) -> Vec<Range<usize>> {
    let a = words(old);
    let b = words(new);
    // lcs[i][j] = length of the LCS of a[i..] and b[j..].
    let mut lcs = vec![vec![0u32; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            lcs[i][j] = if a[i].1 == b[j].1 {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let mut kept = vec![false; b.len()];
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        if a[i].1 == b[j].1 {
            kept[j] = true;
            i += 1;
            j += 1;
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    let mut out: Vec<Range<usize>> = Vec::new();
    for (k, (r, _)) in b.iter().enumerate() {
        if kept[k] {
            continue;
        }
        match out.last_mut() {
            Some(last) if k > 0 && !kept[k - 1] && !new[last.end..r.start].contains('\n') => {
                last.end = r.end;
            }
            _ => out.push(r.clone()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marked<'a>(old: &str, new: &'a str) -> Vec<&'a str> {
        changed_spans(old, new)
            .into_iter()
            .map(|r| &new[r])
            .collect()
    }

    #[test]
    fn r124_h_only_the_changed_words_are_marked() {
        assert_eq!(
            marked(
                "Fix A in x.ts.\n- add a test",
                "Fix A in x.ts.\n- keep the API"
            ),
            ["keep the API"]
        );
        assert_eq!(marked("a b c", "a b c"), Vec::<&str>::new());
        assert_eq!(marked("", "all new"), ["all new"]);
        assert_eq!(
            marked("a b c d", "a c d"),
            Vec::<&str>::new(),
            "deleted words leave nothing to mark"
        );
        // Two separate changes, and none across a line break.
        assert_eq!(marked("a b c d e", "a X c Y e"), ["X", "Y"]);
        assert_eq!(marked("a\nb", "X\nY"), ["X", "Y"]);
        // Byte ranges hold for non-ASCII text.
        let new = "sửa cái này rồi";
        assert_eq!(marked("sửa cái đó", new), ["này rồi"]);
    }
}
