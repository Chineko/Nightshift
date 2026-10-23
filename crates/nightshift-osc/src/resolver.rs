#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResolveResult {
    Resolved { parameter: String, score: u32 },
    Ambiguous { candidates: Vec<String>, score: u32 },
    Missing,
}

#[derive(Clone, Debug)]
pub struct SemanticQuery<'a> {
    pub required: &'a [&'a str],
    pub preferred: &'a [&'a str],
}

pub fn resolve_parameter<'a, I>(parameters: I, query: &SemanticQuery<'_>) -> ResolveResult
where
    I: IntoIterator<Item = &'a str>,
{
    let mut matches = parameters
        .into_iter()
        .filter_map(|parameter| {
            let normalized = normalize(parameter);
            let tokens = tokenize(parameter);

            let required_ok = query.required.iter().all(|needle| {
                let needle = normalize(needle);
                tokens.iter().any(|token| token == &needle) || normalized.contains(&needle)
            });
            if !required_ok {
                return None;
            }

            let required_score = query
                .required
                .iter()
                .map(|needle| score_term(&normalized, &tokens, needle, true))
                .sum::<u32>();
            let preferred_score = query
                .preferred
                .iter()
                .map(|needle| score_term(&normalized, &tokens, needle, false))
                .sum::<u32>();

            Some((parameter.to_string(), required_score + preferred_score))
        })
        .collect::<Vec<_>>();

    if matches.is_empty() {
        return ResolveResult::Missing;
    }

    matches.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let best_score = matches[0].1;
    let best = matches
        .into_iter()
        .take_while(|(_, score)| *score == best_score)
        .map(|(name, _)| name)
        .collect::<Vec<_>>();

    if best.len() == 1 {
        ResolveResult::Resolved {
            parameter: best.into_iter().next().expect("one best match"),
            score: best_score,
        }
    } else {
        ResolveResult::Ambiguous {
            candidates: best,
            score: best_score,
        }
    }
}

fn score_term(normalized: &str, tokens: &[String], needle: &str, required: bool) -> u32 {
    let needle = normalize(needle);
    if needle.is_empty() {
        return 0;
    }

    let weight = if required { 10 } else { 3 };
    if tokens.iter().any(|token| token == &needle) {
        weight * 3
    } else if normalized == needle {
        weight * 2
    } else if normalized.contains(&needle) {
        weight
    } else {
        0
    }
}

fn normalize(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn tokenize(value: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut previous_was_lower_or_digit = false;

    for ch in value.chars() {
        if !ch.is_ascii_alphanumeric() {
            if !current.is_empty() {
                tokens.push(current.to_ascii_lowercase());
                current.clear();
            }
            previous_was_lower_or_digit = false;
            continue;
        }

        let camel_boundary = ch.is_ascii_uppercase() && previous_was_lower_or_digit;
        if camel_boundary && !current.is_empty() {
            tokens.push(current.to_ascii_lowercase());
            current.clear();
        }

        previous_was_lower_or_digit = ch.is_ascii_lowercase() || ch.is_ascii_digit();
        current.push(ch);
    }

    if !current.is_empty() {
        tokens.push(current.to_ascii_lowercase());
    }

    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_across_separators_and_case() {
        let parameters = [
            "Other/Thing",
            "Go/Locomotion/FeetLock",
            "Some/Parameter",
        ];
        let result = resolve_parameter(
            parameters.iter().copied(),
            &SemanticQuery {
                required: &["feet", "lock"],
                preferred: &["go", "locomotion"],
            },
        );

        assert!(matches!(
            result,
            ResolveResult::Resolved { parameter, .. } if parameter == "Go/Locomotion/FeetLock"
        ));
    }

    #[test]
    fn prefers_token_matches_over_loose_substrings() {
        let parameters = ["Avatar/DefeetedLocksmith", "GoGo/Feet_Lock"];
        let result = resolve_parameter(
            parameters.iter().copied(),
            &SemanticQuery {
                required: &["feet", "lock"],
                preferred: &["gogo"],
            },
        );

        assert!(matches!(
            result,
            ResolveResult::Resolved { parameter, .. } if parameter == "GoGo/Feet_Lock"
        ));
    }

    #[test]
    fn reports_ambiguity_instead_of_guessing() {
        let parameters = ["GoGo/Prone_A", "GoGo/Prone_B"];
        let result = resolve_parameter(
            parameters.iter().copied(),
            &SemanticQuery {
                required: &["prone"],
                preferred: &["gogo"],
            },
        );

        assert!(matches!(
            result,
            ResolveResult::Ambiguous { candidates, .. } if candidates.len() == 2
        ));
    }

    #[test]
    fn missing_when_required_terms_are_absent() {
        let parameters = ["GoGo/Sit", "GoGo/Stand"];
        assert_eq!(
            resolve_parameter(
                parameters.iter().copied(),
                &SemanticQuery {
                    required: &["prone"],
                    preferred: &["gogo"],
                },
            ),
            ResolveResult::Missing
        );
    }
}
