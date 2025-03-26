use super::{Lint, LintKind, PatternLinter};
use crate::Token;
use crate::char_string::char_string;
use crate::linting::Suggestion;
use crate::patterns::{
    All, AnyCapitalization, Invert, OwnedPatternExt, Pattern, SequencePattern, WordSet,
};

#[doc = "Corrects the misuse of `then` to `than`."]
pub struct ThenThan {
    pattern: Box<dyn Pattern>,
}

impl ThenThan {
    pub fn new() -> Self {
        Self {
            pattern: Box::new(All::new(vec![
                Box::new(
                    SequencePattern::default()
                        .then(WordSet::new(&["better", "other"]).or(Box::new(
                            |tok: &Token, _source: &[char]| tok.kind.is_adjective(),
                        )))
                        .then_whitespace()
                        .then_any_capitalization_of("then")
                        .then_whitespace()
                        .then(Invert::new(AnyCapitalization::new(char_string!("that")))),
                ),
                // Denotes exceptions to the rule.
                Box::new(Invert::new(WordSet::new(&["back", "this", "so", "but"]))),
            ])),
        }
    }
}

impl Default for ThenThan {
    fn default() -> Self {
        Self::new()
    }
}

impl PatternLinter for ThenThan {
    fn pattern(&self) -> &dyn Pattern {
        self.pattern.as_ref()
    }
    fn match_to_lint(&self, matched_tokens: &[Token], source: &[char]) -> Option<Lint> {
        let span = matched_tokens[2].span;
        let offending_text = span.get_content(source);

        Some(Lint {
            span,
            lint_kind: LintKind::Miscellaneous,
            suggestions: vec![Suggestion::replace_with_match_case(
                "than".chars().collect(),
                offending_text,
            )],
            message: "Did you mean `than`?".to_string(),
            priority: 31,
        })
    }
    fn description(&self) -> &'static str {
        "Corrects the misuse of `then` to `than`."
    }
}

#[cfg(test)]
mod tests {
    use super::ThenThan;
    use crate::linting::tests::{assert_lint_count, assert_suggestion_result};

    #[test]
    fn allows_back_then() {
        assert_lint_count("I was a gross kid back then.", ThenThan::default(), 0);
    }

    #[test]
    fn catches_shorter_then() {
        assert_suggestion_result(
            "One was shorter then the other.",
            ThenThan::default(),
            "One was shorter than the other.",
        );
    }

    #[test]
    fn catches_better_then() {
        assert_suggestion_result(
            "One was better then the other.",
            ThenThan::default(),
            "One was better than the other.",
        );
    }

    #[test]
    fn catches_longer_then() {
        assert_suggestion_result(
            "One was longer then the other.",
            ThenThan::default(),
            "One was longer than the other.",
        );
    }

    #[test]
    fn catches_less_then() {
        assert_suggestion_result(
            "I eat less then you.",
            ThenThan::default(),
            "I eat less than you.",
        );
    }

    #[test]
    fn catches_more_then() {
        assert_suggestion_result(
            "I eat more then you.",
            ThenThan::default(),
            "I eat more than you.",
        );
    }

    #[test]
    fn stronger_should_change() {
        assert_suggestion_result(
            "a chain is no stronger then its weakest link",
            ThenThan::default(),
            "a chain is no stronger than its weakest link",
        );
    }

    #[test]
    fn half_a_loaf_should_change() {
        assert_suggestion_result(
            "half a loaf is better then no bread",
            ThenThan::default(),
            "half a loaf is better than no bread",
        );
    }

    #[test]
    fn then_everyone_clapped_should_be_allowed() {
        assert_lint_count("and then everyone clapped", ThenThan::default(), 0);
    }

    #[test]
    fn crazier_than_rat_should_change() {
        assert_suggestion_result(
            "crazier then a shithouse rat",
            ThenThan::default(),
            "crazier than a shithouse rat",
        );
    }

    #[test]
    fn poke_in_eye_should_change() {
        assert_suggestion_result(
            "better then a poke in the eye with a sharp stick",
            ThenThan::default(),
            "better than a poke in the eye with a sharp stick",
        );
    }

    #[test]
    fn other_then_should_change() {
        assert_suggestion_result(
            "There was no one other then us at the campsite.",
            ThenThan::default(),
            "There was no one other than us at the campsite.",
        );
    }

    #[test]
    fn allows_and_then() {
        assert_lint_count("And then we left.", ThenThan::default(), 0);
    }

    #[test]
    fn allows_this_then() {
        assert_lint_count("Do this then that.", ThenThan::default(), 0);
    }

    #[test]
    fn allows_issue_720() {
        assert_lint_count(
            "And if just one of those is set incorrectly or it has the tiniest bit of dirt inside then that will wreak havoc with the engine's running ability.",
            ThenThan::default(),
            0,
        );
        assert_lint_count("So let's check it out then.", ThenThan::default(), 0);
        assert_lint_count(
            "And if just the tiniest bit of dirt gets inside then that will wreak havoc.",
            ThenThan::default(),
            0,
        );

        assert_lint_count(
            "He was always a top student in school but then his argument is that grades don't define intelligence.",
            ThenThan::default(),
            0,
        );
    }

    #[test]
    fn allows_issue_744() {
        assert_lint_count(
            "So then after talking about how he would, he didn't.",
            ThenThan::default(),
            0,
        );
    }

    #[test]
    fn issue_720_school_but_then_his() {
        assert_lint_count(
            "She loved the atmosphere of the school but then his argument is that it lacks proper resources for students.",
            ThenThan::default(),
            0,
        );
        assert_lint_count(
            "The teacher praised the efforts of the school but then his argument is that the curriculum needs to be updated.",
            ThenThan::default(),
            0,
        );
        assert_lint_count(
            "They were excited about the new program at school but then his argument is that it won't be effective without proper training.",
            ThenThan::default(),
            0,
        );
        assert_lint_count(
            "The community supported the school but then his argument is that funding is still a major issue.",
            ThenThan::default(),
            0,
        );
    }

    #[test]
    fn issue_720_so_then_these_resistors() {
        assert_lint_count(
            "So then these resistors are connected up in parallel to reduce the overall resistance.",
            ThenThan::default(),
            0,
        );
        assert_lint_count(
            "So then these resistors are connected up to ensure the current flows properly.",
            ThenThan::default(),
            0,
        );
        assert_lint_count(
            "So then these resistors are connected up to achieve the desired voltage drop.",
            ThenThan::default(),
            0,
        );
        assert_lint_count(
            "So then these resistors are connected up to demonstrate the principles of series and parallel circuits.",
            ThenThan::default(),
            0,
        );
        assert_lint_count(
            "So then these resistors are connected up to optimize the circuit's performance.",
            ThenThan::default(),
            0,
        );
    }

    #[test]
    fn issue_720_yes_so_then_sorry() {
        assert_lint_count(
            "Yes so then sorry you didn't receive the memo about the meeting changes.",
            ThenThan::default(),
            0,
        );
        assert_lint_count(
            "Yes so then sorry you had to wait so long for a response from our team.",
            ThenThan::default(),
            0,
        );
        assert_lint_count(
            "Yes so then sorry you felt left out during the discussion; we value your input.",
            ThenThan::default(),
            0,
        );
        assert_lint_count(
            "Yes so then sorry you missed the deadline; we can discuss an extension.",
            ThenThan::default(),
            0,
        );
        assert_lint_count(
            "Yes so then sorry you encountered issues with the software; let me help you troubleshoot.",
            ThenThan::default(),
            0,
        );
    }
}
