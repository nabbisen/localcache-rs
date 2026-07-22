use super::*;

fn assert_matches(pattern: &str, matching: &[&str], rejected: &[&str]) {
    let glob = compile(pattern).unwrap();
    for candidate in matching {
        assert!(
            glob.matches(candidate),
            "{pattern:?} should match {candidate:?}"
        );
    }
    for candidate in rejected {
        assert!(
            !glob.matches(candidate),
            "{pattern:?} should not match {candidate:?}"
        );
    }
}

fn message(error: LocalFileCacheError) -> String {
    match error {
        LocalFileCacheError::UnsupportedFeature(message) => message,
        other => panic!("unexpected error: {other:?}"),
    }
}

fn reference_match(pattern: &str, candidate: &str) -> bool {
    let pattern = pattern.chars().collect::<Vec<_>>();
    let candidate = candidate.chars().collect::<Vec<_>>();
    let mut table = vec![vec![false; candidate.len() + 1]; pattern.len() + 1];
    table[0][0] = true;
    for pattern_index in 1..=pattern.len() {
        if pattern[pattern_index - 1] == '*' {
            table[pattern_index][0] = table[pattern_index - 1][0];
        }
        for candidate_index in 1..=candidate.len() {
            table[pattern_index][candidate_index] = match pattern[pattern_index - 1] {
                '*' => {
                    table[pattern_index - 1][candidate_index]
                        || table[pattern_index][candidate_index - 1]
                }
                '?' => table[pattern_index - 1][candidate_index - 1],
                literal => {
                    literal == candidate[candidate_index - 1]
                        && table[pattern_index - 1][candidate_index - 1]
                }
            };
        }
    }
    table[pattern.len()][candidate.len()]
}

fn reference_expand(pattern: &str) -> Result<Vec<String>, ()> {
    let mut depth = 0_i32;
    for ch in pattern.chars() {
        match ch {
            '{' => depth += 1,
            '}' if depth == 0 => return Err(()),
            '}' => depth -= 1,
            _ => {}
        }
    }
    if depth != 0 {
        return Err(());
    }

    let Some(open) = pattern.find('{') else {
        return Ok(vec![pattern.to_owned()]);
    };
    let mut nested = 0_i32;
    let mut close = None;
    for (offset, ch) in pattern[open..].char_indices() {
        match ch {
            '{' => nested += 1,
            '}' => {
                nested -= 1;
                if nested == 0 {
                    close = Some(open + offset);
                    break;
                }
            }
            _ => {}
        }
    }
    let close = close.ok_or(())?;
    let prefix = &pattern[..open];
    let inner = &pattern[open + 1..close];
    let suffix = &pattern[close + 1..];
    let mut output = Vec::new();
    for alternative in reference_split_group(inner) {
        output.extend(reference_expand(&format!("{prefix}{alternative}{suffix}"))?);
    }
    Ok(output)
}

fn reference_split_group(group: &str) -> Vec<&str> {
    let mut alternatives = Vec::new();
    let mut depth = 0_i32;
    let mut start = 0;
    for (index, ch) in group.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => depth -= 1,
            ',' if depth == 0 => {
                alternatives.push(&group[start..index]);
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }
    alternatives.push(&group[start..]);
    alternatives
}

#[test]
fn scalar_wildcards_and_literals_match_without_byte_slicing() {
    assert_matches("", &[""], &["a"]);
    assert_matches("*", &["", "é", "東京", "🙂🙂"], &[]);
    assert_matches("?", &["é", "東", "🙂"], &["", "e\u{301}", "🙂🙂"]);
    assert_matches("?*é", &["🙂é", "a東京é"], &["é", "🙂e\u{301}"]);
    assert_matches("a**?c", &["abc", "a🙂c", "a東京c"], &["ac", "a🙂"]);
    assert_matches("[x]", &["[x]"], &["x", "[y]"]);
    assert_matches("a,b", &["a,b"], &["a", "b"]);
}

#[test]
fn brace_groups_support_nesting_empty_and_cartesian_products() {
    let glob = compile("{pre,{mid,post}}_{x,y}.{txt,}").unwrap();
    assert_eq!(glob.alternatives.len(), 12);
    assert!(glob.matches("pre_x.txt"));
    assert!(glob.matches("mid_y."));
    assert!(glob.matches("post_x.txt"));
    assert!(!glob.matches("other_x.txt"));

    assert_matches("a{b}c", &["abc"], &["ac"]);
    assert_matches("{,a}", &["", "a"], &["aa"]);
}

#[test]
fn malformed_braces_have_one_stable_error() {
    for pattern in ["}", "a}b", "{", "a{b", "{a,{b,c}", "{a,b}}"] {
        assert_eq!(message(compile(pattern).unwrap_err()), MALFORMED_MESSAGE);
    }
}

#[test]
fn safety_boundaries_are_checked_before_growth() {
    assert!(compile(&"a".repeat(MAX_PATTERN_BYTES)).is_ok());
    assert_eq!(
        message(compile(&"a".repeat(MAX_PATTERN_BYTES + 1)).unwrap_err()),
        SAFETY_MESSAGE
    );
    assert_eq!(message(compile("a\0b").unwrap_err()), SAFETY_MESSAGE);

    let depth_32 = format!("{}x{}", "{".repeat(32), "}".repeat(32));
    assert!(compile(&depth_32).is_ok());
    let depth_33 = format!("{}x{}", "{".repeat(33), "}".repeat(33));
    assert_eq!(message(compile(&depth_33).unwrap_err()), SAFETY_MESSAGE);

    let alternatives_256 = format!("{{{}}}", vec!["x"; 256].join(","));
    assert_eq!(compile(&alternatives_256).unwrap().alternatives.len(), 256);
    let alternatives_257 = format!("{{{}}}", vec!["x"; 257].join(","));
    assert_eq!(
        message(compile(&alternatives_257).unwrap_err()),
        SAFETY_MESSAGE
    );

    let sixteen = vec!["x"; 16].join(",");
    assert_eq!(
        compile(&format!("{{{sixteen}}}{{{sixteen}}}"))
            .unwrap()
            .alternatives
            .len(),
        256
    );
    let seventeen = vec!["x"; 17].join(",");
    assert_eq!(
        message(compile(&format!("{{{seventeen}}}{{{sixteen}}}")).unwrap_err()),
        SAFETY_MESSAGE
    );
}

#[test]
fn many_sequential_groups_do_not_recurse_by_group_count() {
    let pattern = "{x}".repeat(2_000);
    let candidate = "x".repeat(2_000);
    let glob = compile(&pattern).unwrap();
    assert!(glob.matches(&candidate));
}

#[test]
fn sqlite_translation_uses_the_same_expanded_tokens() {
    let glob = compile("{a,[b]}?*").unwrap();
    assert_eq!(glob.sqlite_alternatives(), &["a?*", "[[]b]?*"]);
}

#[test]
fn bounded_pattern_and_candidate_corpus_never_panics() {
    let atoms = ["a", "*", "?", "{", "}", "é", "\u{301}", "東", "🙂", ","];
    for first in atoms {
        for second in atoms {
            for third in atoms {
                let pattern = format!("{first}{second}{third}");
                let compiled = std::panic::catch_unwind(|| compile(&pattern))
                    .expect("glob compilation must not panic");
                let reference = reference_expand(&pattern);
                match compiled {
                    Ok(glob) => {
                        let alternatives = reference.expect("accepted brace syntax must expand");
                        for candidate in ["", "a", "é", "e\u{301}", "東京", "🙂a", "a,b"] {
                            let actual = std::panic::catch_unwind(|| glob.matches(candidate))
                                .expect("glob matching must not panic");
                            let expected = alternatives
                                .iter()
                                .any(|alternative| reference_match(alternative, candidate));
                            assert_eq!(actual, expected, "{pattern:?} vs {candidate:?}");
                        }
                    }
                    Err(error) => {
                        assert!(reference.is_err(), "reference accepted {pattern:?}");
                        assert!(matches!(
                            message(error).as_str(),
                            MALFORMED_MESSAGE | SAFETY_MESSAGE
                        ));
                    }
                }
            }
        }
    }
}

#[test]
fn long_candidates_and_many_stars_complete_iteratively() {
    let pattern = format!("{}z", "*?".repeat(128));
    let candidate = format!("{}z", "🙂".repeat(8_192));
    assert!(compile(&pattern).unwrap().matches(&candidate));
}
