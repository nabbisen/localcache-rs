use super::*;

#[test]
fn truncation_is_scalar_safe_at_every_boundary() {
    for value in ["ascii", "éclair", "e\u{301}cole", "東京駅", "🙂🙃🙂"] {
        assert_eq!(truncate(value, 0), "");
        assert_eq!(truncate(value, 1), "…");
        assert_eq!(truncate(value, value.chars().count()), value);
        assert_eq!(truncate(value, value.chars().count() + 1), value);
    }

    assert_eq!(truncate("éclair", 4), "…air");
    assert_eq!(truncate("e\u{301}cole", 4), "…ole");
    assert_eq!(truncate("東京駅", 2), "…駅");
    assert_eq!(truncate("🙂🙃🙂", 2), "…🙂");
}

#[test]
fn bounded_truncation_corpus_preserves_a_valid_suffix() {
    let atoms = ['a', 'é', '\u{301}', '東', '🙂'];
    for first in atoms {
        for second in atoms {
            for third in atoms {
                let value = [first, second, third].iter().collect::<String>();
                for max in 0..=5 {
                    let output = truncate(&value, max);
                    assert!(output.chars().count() <= max);
                    if max >= value.chars().count() {
                        assert_eq!(output, value);
                    } else if max > 1 {
                        let suffix = value.chars().skip(value.chars().count() - (max - 1));
                        assert_eq!(
                            output.chars().skip(1).collect::<String>(),
                            suffix.collect::<String>()
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn seconds_formatting_is_total_and_exact() {
    assert_eq!(format_unix_seconds(0), "1970-01-01 00:00:00");
    assert_eq!(format_unix_seconds(-1), "1969-12-31 23:59:59");
    assert_eq!(format_unix_seconds(1_704_067_200), "2024-01-01 00:00:00");
    assert_eq!(
        format_unix_seconds(i64::MIN),
        "-292277022657-01-27 08:29:52"
    );
    assert_eq!(format_unix_seconds(i64::MAX), "292277026596-12-04 15:30:07");
}

#[test]
fn nanoseconds_formatting_uses_euclidean_fraction_and_exact_units() {
    assert_eq!(format_unix_nanoseconds(0), "1970-01-01 00:00:00.000000000");
    assert_eq!(format_unix_nanoseconds(1), "1970-01-01 00:00:00.000000001");
    assert_eq!(format_unix_nanoseconds(-1), "1969-12-31 23:59:59.999999999");
    assert_eq!(
        format_unix_nanoseconds(i64::MIN),
        "1677-09-21 00:12:43.145224192"
    );
    assert_eq!(
        format_unix_nanoseconds(i64::MAX),
        "2262-04-11 23:47:16.854775807"
    );
}
