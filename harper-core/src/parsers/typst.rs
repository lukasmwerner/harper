use itertools::Itertools;
use std::collections::VecDeque;
use typst_syntax::ast::{AstNode, Expr, Markup};

use super::{Parser, PlainEnglish};
use crate::{
    parsers::StrParser,
    patterns::{PatternExt, SequencePattern},
    ConjunctionData, Lrc, NounData, Punctuation, Span, Token, TokenKind, VecExt, WordMetadata,
};

/// A parser that wraps the [`PlainEnglish`] parser that allows one to parse
/// Typst files.
pub struct Typst;

/// Encapsulation of the translation between byte-based spans and char-based spans
#[derive(Debug, Clone, Copy)]
struct Offset<'a> {
    doc: &'a typst_syntax::Source,
    pub char: usize,
    pub byte: usize,
}

impl<'a> Offset<'a> {
    pub fn new(doc: &'a typst_syntax::Source) -> Self {
        Self {
            doc,
            char: 0,
            byte: 0,
        }
    }

    pub fn push_to(self, new_byte: usize) -> Self {
        assert!(new_byte >= self.byte);
        Self {
            char: self.doc.get(self.byte..new_byte).unwrap().chars().count(),
            byte: new_byte,
            ..self
        }
    }

    pub fn push_by(self, relative_bytes: usize) -> Self {
        let new_byte = self.byte + relative_bytes;
        Self {
            char: self.doc.get(self.byte..new_byte).unwrap().chars().count(),
            byte: new_byte,
            ..self
        }
    }

    pub fn push_to_span(self, span: typst_syntax::Span) -> Self {
        let new_byte = self.doc.range(span).unwrap().start;
        assert!(new_byte >= self.byte);

        self.push_to(new_byte)
    }
}

macro_rules! constant_token {
    ($doc:ident, $a:expr, $kind:expr, $offset:expr) => {{
        let start_char_loc = $offset.push_to($doc.range($a.span()).unwrap().start).char;
        let end_char_loc = $offset.push_to($doc.range($a.span()).unwrap().end).char;

        Some(vec![Token {
            span: Span {
                start: start_char_loc,
                end: end_char_loc,
            },
            kind: $kind,
        }])
    }};
}

macro_rules! merge_expr {
    ($($inner:expr),*) => {
        Some(
            [$($inner),*]
                .into_iter()
                .flatten()
                .flatten()
                .collect_vec(),
        )
    };
}

fn parse_english(
    str: impl Into<String>,
    parser: &mut PlainEnglish,
    offset: Offset,
) -> Option<Vec<Token>> {
    Some(
        parser
            .parse_str(str.into())
            .into_iter()
            .map(|mut t| {
                t.span.push_by(offset.char);
                t
            })
            .collect_vec(),
    )
}

fn parse_dict(
    dict: &mut dyn Iterator<Item = typst_syntax::ast::DictItem>,
    doc: &typst_syntax::Source,
    parser: &mut PlainEnglish,
    offset: Offset,
) -> Option<Vec<Token>> {
    Some(
        dict.filter_map(|di| match di {
            typst_syntax::ast::DictItem::Named(named) => merge_expr!(
                constant_token!(
                    doc,
                    named.name(),
                    TokenKind::Word(WordMetadata::default()),
                    offset
                ),
                parse_expr(
                    named.expr(),
                    doc,
                    parser,
                    offset.push_to_span(named.expr().span())
                )
            ),
            typst_syntax::ast::DictItem::Keyed(keyed) => merge_expr!(
                parse_expr(
                    keyed.key(),
                    doc,
                    parser,
                    offset.push_to_span(keyed.key().span())
                ),
                parse_expr(
                    keyed.expr(),
                    doc,
                    parser,
                    offset.push_to_span(keyed.expr().span())
                )
            ),
            typst_syntax::ast::DictItem::Spread(spread) => spread.sink_ident().map_or_else(
                || {
                    spread.sink_expr().and_then(|expr| {
                        parse_expr(expr, doc, parser, offset.push_to_span(expr.span()))
                    })
                },
                |ident| {
                    constant_token!(doc, ident, TokenKind::Word(WordMetadata::default()), offset)
                },
            ),
        })
        .flatten()
        .collect(),
    )
}

fn parse_pattern(
    pat: typst_syntax::ast::Pattern,
    doc: &typst_syntax::Source,
    parser: &mut PlainEnglish,
    offset: Offset,
) -> Option<Vec<Token>> {
    match pat {
        typst_syntax::ast::Pattern::Normal(expr) => {
            parse_expr(expr, doc, parser, offset.push_to_span(expr.span()))
        }
        typst_syntax::ast::Pattern::Placeholder(underscore) => {
            constant_token!(doc, underscore, TokenKind::Unlintable, offset)
        }
        typst_syntax::ast::Pattern::Parenthesized(parenthesized) => merge_expr!(
            parse_expr(
                parenthesized.expr(),
                doc,
                parser,
                offset.push_to_span(parenthesized.expr().span())
            ),
            parse_pattern(
                parenthesized.pattern(),
                doc,
                parser,
                offset.push_to_span(parenthesized.pattern().span())
            )
        ),
        typst_syntax::ast::Pattern::Destructuring(destructuring) => Some(
            destructuring
                .items()
                .filter_map(|item| match item {
                    typst_syntax::ast::DestructuringItem::Pattern(pattern) => {
                        parse_pattern(pattern, doc, parser, offset.push_to_span(pattern.span()))
                    }
                    typst_syntax::ast::DestructuringItem::Named(named) => merge_expr!(
                        constant_token!(
                            doc,
                            named.name(),
                            TokenKind::Word(WordMetadata::default()),
                            offset
                        ),
                        parse_pattern(
                            named.pattern(),
                            doc,
                            parser,
                            offset.push_to_span(named.pattern().span())
                        )
                    ),
                    typst_syntax::ast::DestructuringItem::Spread(spread) => {
                        spread.sink_ident().map_or_else(
                            || {
                                spread.sink_expr().and_then(|expr| {
                                    parse_expr(expr, doc, parser, offset.push_to_span(expr.span()))
                                })
                            },
                            |ident| {
                                constant_token!(
                                    doc,
                                    ident,
                                    TokenKind::Word(WordMetadata::default()),
                                    offset
                                )
                            },
                        )
                    }
                })
                .flatten()
                .collect(),
        ),
    }
}

fn parse_expr(
    ex: typst_syntax::ast::Expr,
    doc: &typst_syntax::Source,
    parser: &mut PlainEnglish,
    offset: Offset,
) -> Option<Vec<Token>> {
    macro_rules! constant_token {
        ($a:expr, $kind:expr) => {{
            let start_char_loc = offset.push_to(doc.range($a.span()).unwrap().start).char;
            let end_char_loc = offset.push_to(doc.range($a.span()).unwrap().end).char;

            Some(vec![Token {
                span: Span {
                    start: start_char_loc,
                    end: end_char_loc,
                },
                kind: $kind,
            }])
        }};
    }
    let mut nested_env = |exprs: &mut dyn Iterator<Item = typst_syntax::ast::Expr>,
                          offset: Offset| {
        Some(
            exprs
                .filter_map(|e| parse_expr(e, doc, parser, offset))
                .flatten()
                .collect_vec(),
        )
    };

    match ex {
        Expr::Text(text) => parse_english(text.get(), parser, offset.push_to_span(text.span())),
        Expr::Space(a) => constant_token!(a, TokenKind::Space(1)),
        Expr::Linebreak(a) => constant_token!(a, TokenKind::Newline(1)),
        Expr::Parbreak(a) => constant_token!(a, TokenKind::ParagraphBreak),
        Expr::Escape(a) => constant_token!(a, TokenKind::Unlintable),
        Expr::Shorthand(a) => constant_token!(a, TokenKind::Unlintable),
        Expr::SmartQuote(quote) => {
            if quote.double() {
                constant_token!(
                    quote,
                    TokenKind::Punctuation(Punctuation::Quote(crate::Quote { twin_loc: None }))
                )
            } else {
                constant_token!(quote, TokenKind::Punctuation(Punctuation::Apostrophe))
            }
        }
        Expr::Strong(strong) => nested_env(
            &mut strong.body().exprs(),
            offset.push_to_span(strong.span()),
        ),
        Expr::Emph(emph) => nested_env(&mut emph.body().exprs(), offset.push_to_span(emph.span())),
        Expr::Raw(a) => constant_token!(a, TokenKind::Unlintable),
        Expr::Link(a) => constant_token!(a, TokenKind::Url),
        Expr::Label(label) => parse_english(label.get(), parser, offset.push_to_span(label.span())),
        Expr::Ref(a) => {
            constant_token!(a, TokenKind::Word(WordMetadata::default()))
        }
        Expr::Heading(heading) => nested_env(
            &mut heading.body().exprs(),
            offset.push_to_span(heading.span()),
        ),
        Expr::List(list_item) => nested_env(
            &mut list_item.body().exprs(),
            offset.push_to_span(list_item.span()),
        ),
        Expr::Enum(enum_item) => nested_env(
            &mut enum_item.body().exprs(),
            offset.push_to_span(enum_item.span()),
        ),
        Expr::Term(term_item) => nested_env(
            &mut term_item
                .term()
                .exprs()
                .chain(term_item.description().exprs()),
            offset.push_to_span(term_item.span()),
        ),
        Expr::Equation(a) => constant_token!(a, TokenKind::Unlintable),
        Expr::Math(_) => panic!("Unexpected math outside equation environment."),
        Expr::MathIdent(_) => panic!("Unexpected math outside equation environment."),
        Expr::MathShorthand(_) => panic!("Unexpected math outside equation environment."),
        Expr::MathAlignPoint(_) => panic!("Unexpected math outside equation environment."),
        Expr::MathDelimited(_) => panic!("Unexpected math outside equation environment."),
        Expr::MathAttach(_) => panic!("Unexpected math outside equation environment."),
        Expr::MathPrimes(_) => panic!("Unexpected math outside equation environment."),
        Expr::MathFrac(_) => panic!("Unexpected math outside equation environment."),
        Expr::MathRoot(_) => panic!("Unexpected math outside equation environment."),
        Expr::Ident(a) => constant_token!(a, TokenKind::Word(WordMetadata::default())),
        Expr::None(a) => constant_token!(a, TokenKind::Word(WordMetadata::default())),
        Expr::Auto(a) => constant_token!(a, TokenKind::Word(WordMetadata::default())),
        Expr::Bool(a) => constant_token!(a, TokenKind::Word(WordMetadata::default())),
        Expr::Int(int) => {
            constant_token!(int, TokenKind::Number((int.get() as f64).into(), None))
        }
        Expr::Float(float) => {
            constant_token!(float, TokenKind::Number(float.get().into(), None))
        }
        Expr::Numeric(a) => constant_token!(a, TokenKind::Unlintable),
        Expr::Str(text) => {
            let offset = doc.range(text.span()).unwrap().start + 1;
            let text = text.to_untyped().text();
            Some(
                parser
                    .parse_str(&text[1..text.len() - 1])
                    .into_iter()
                    .map(|mut t| {
                        t.span.push_by(offset);
                        t
                    })
                    .collect_vec(),
            )
        }
        Expr::Code(a) => constant_token!(a, TokenKind::Unlintable),
        Expr::Content(content_block) => nested_env(
            &mut content_block.body().exprs(),
            offset.push_to_span(content_block.span()),
        ),
        Expr::Parenthesized(parenthesized) => parse_expr(
            parenthesized.expr(),
            doc,
            parser,
            offset.push_to_span(parenthesized.span()),
        ),
        Expr::Array(array) => Some(
            array
                .items()
                .filter_map(|i| {
                    if let typst_syntax::ast::ArrayItem::Pos(e) = i {
                        parse_expr(e, doc, parser, offset.push_to_span(array.span()))
                    } else {
                        None
                    }
                })
                .flatten()
                .collect_vec(),
        ),
        Expr::Dict(a) => parse_dict(&mut a.items(), doc, parser, offset.push_to_span(a.span())),
        Expr::Unary(a) => constant_token!(a, TokenKind::Unlintable),
        Expr::Binary(a) => constant_token!(a, TokenKind::Unlintable),
        Expr::FieldAccess(field_access) => merge_expr!(
            parse_expr(
                field_access.target(),
                doc,
                parser,
                offset.push_to_span(field_access.span())
            ),
            constant_token!(
                field_access.field(),
                TokenKind::Word(WordMetadata::default())
            )
        ),
        Expr::FuncCall(a) => constant_token!(a, TokenKind::Unlintable),
        Expr::Closure(a) => constant_token!(a, TokenKind::Unlintable),
        Expr::Let(let_binding) => merge_expr!(
            match let_binding.kind() {
                typst_syntax::ast::LetBindingKind::Normal(pattern) =>
                    parse_pattern(pattern, doc, parser, offset.push_to_span(pattern.span())),
                typst_syntax::ast::LetBindingKind::Closure(ident) =>
                    constant_token!(ident, TokenKind::Word(WordMetadata::default())),
            },
            let_binding.init().and_then(|e| parse_expr(
                e,
                doc,
                parser,
                offset.push_to_span(e.span())
            ))
        ),
        Expr::DestructAssign(destruct_assignment) => parse_expr(
            destruct_assignment.value(),
            doc,
            parser,
            offset.push_to_span(destruct_assignment.span()),
        ),
        Expr::Set(set_rule) => merge_expr!(
            parse_expr(
                set_rule.target(),
                doc,
                parser,
                offset.push_to_span(set_rule.target().span())
            ),
            parse_expr(
                set_rule.condition()?,
                doc,
                parser,
                offset.push_to_span(set_rule.condition()?.span())
            )
        ),
        Expr::Show(show_rule) => merge_expr!(
            parse_expr(
                show_rule.transform(),
                doc,
                parser,
                offset.push_to_span(show_rule.transform().span())
            ),
            parse_expr(
                show_rule.selector()?,
                doc,
                parser,
                offset.push_to_span(show_rule.selector()?.span())
            )
        ),
        Expr::Contextual(contextual) => parse_expr(
            contextual.body(),
            doc,
            parser,
            offset.push_to_span(contextual.span()),
        ),
        Expr::Conditional(conditional) => merge_expr!(
            parse_expr(
                conditional.condition(),
                doc,
                parser,
                offset.push_to_span(conditional.condition().span())
            ),
            parse_expr(
                conditional.if_body(),
                doc,
                parser,
                offset.push_to_span(conditional.if_body().span())
            ),
            parse_expr(
                conditional.else_body()?,
                doc,
                parser,
                offset.push_to_span(conditional.else_body()?.span())
            )
        ),
        Expr::While(while_loop) => merge_expr!(
            parse_expr(
                while_loop.condition(),
                doc,
                parser,
                offset.push_to_span(while_loop.condition().span())
            ),
            parse_expr(
                while_loop.body(),
                doc,
                parser,
                offset.push_to_span(while_loop.body().span())
            )
        ),
        Expr::For(for_loop) => merge_expr!(
            parse_expr(
                for_loop.iterable(),
                doc,
                parser,
                offset.push_to_span(for_loop.iterable().span())
            ),
            parse_expr(
                for_loop.body(),
                doc,
                parser,
                offset.push_to_span(for_loop.body().span())
            )
        ),
        Expr::Import(module_import) => {
            merge_expr!(
                parse_expr(
                    module_import.source(),
                    doc,
                    parser,
                    offset.push_to_span(module_import.source().span())
                ),
                constant_token!(
                    module_import.new_name()?,
                    TokenKind::Word(WordMetadata::default())
                )
            )
        }
        Expr::Include(a) => constant_token!(a, TokenKind::Unlintable),
        Expr::Break(a) => constant_token!(a, TokenKind::Unlintable),
        Expr::Continue(a) => constant_token!(a, TokenKind::Unlintable),
        Expr::Return(a) => constant_token!(a, TokenKind::Unlintable),
    }
}

thread_local! {
    static WORD_APOSTROPHE_WORD: Lrc<SequencePattern> = Lrc::new(SequencePattern::default()
                .then_any_word()
                .then_apostrophe()
                .then_any_word());
}

impl Parser for Typst {
    fn parse(&mut self, source: &[char]) -> Vec<Token> {
        let mut english_parser = PlainEnglish;

        let source_str: String = source.iter().collect();
        let typst_document = typst_syntax::Source::detached(source_str);
        let typst_tree = Markup::from_untyped(typst_document.root())
            .expect("Unable to create typst document from parsed tree!");

        // NOTE: the range spits out __byte__ indices, not char indices.
        // This is why we keep track above.
        let mut tokens = typst_tree
            .exprs()
            .filter_map(|ex| {
                parse_expr(
                    ex,
                    &typst_document,
                    &mut english_parser,
                    Offset::new(&typst_document),
                )
            })
            .flatten()
            .collect_vec();

        // Consolidate conjunctions
        let mut to_remove = VecDeque::default();
        for tok_span in WORD_APOSTROPHE_WORD
            .with(|v| v.clone())
            .find_all_matches(&tokens, source)
        {
            let start_tok = &tokens[tok_span.start];
            let end_tok = &tokens[tok_span.end - 1];
            let char_span = Span::new(start_tok.span.start, end_tok.span.end);

            if let TokenKind::Word(metadata) = start_tok.kind {
                tokens[tok_span.start].kind =
                    TokenKind::Word(if end_tok.span.get_content(source) == ['s'] {
                        WordMetadata {
                            noun: Some(NounData {
                                is_possessive: Some(true),
                                ..metadata.noun.unwrap_or_default()
                            }),
                            conjunction: None,
                            ..metadata
                        }
                    } else {
                        WordMetadata {
                            noun: metadata.noun.map(|noun| NounData {
                                is_possessive: Some(false),
                                ..noun
                            }),
                            conjunction: Some(ConjunctionData {}),
                            ..metadata
                        }
                    });

                tokens[tok_span.start].span = char_span;
                to_remove.extend(tok_span.start + 1..tok_span.end);
            } else {
                panic!("Apostrophe consolidation does not start with Word Token!")
            }
        }
        tokens.remove_indices(to_remove.into_iter().sorted().unique().collect());

        tokens
    }
}

#[cfg(test)]
mod tests {
    use itertools::Itertools;
    use ordered_float::OrderedFloat;

    use super::Typst;
    use crate::{parsers::StrParser, NounData, Punctuation, TokenKind, WordMetadata};

    #[test]
    fn conjunction() {
        let source = "doesn't";

        let tokens = Typst.parse_str(source);
        let token_kinds = tokens.iter().map(|t| t.kind).collect_vec();
        dbg!(&token_kinds);

        assert_eq!(token_kinds.len(), 1);
        assert!(token_kinds.into_iter().all(|t| t.is_conjunction()))
    }

    #[test]
    fn possessive() {
        let source = "person's";

        let tokens = Typst.parse_str(source);
        let token_kinds = tokens.iter().map(|t| t.kind).collect_vec();
        dbg!(&token_kinds);

        assert_eq!(token_kinds.len(), 1);
        assert!(token_kinds.into_iter().all(|t| {
            matches!(
                t,
                TokenKind::Word(WordMetadata {
                    noun: Some(NounData {
                        is_possessive: Some(true),
                        ..
                    }),
                    ..
                })
            )
        }))
    }

    #[test]
    fn number() {
        let source = "12 is larger than 11, but much less than 11!";

        let tokens = Typst.parse_str(source);
        let token_kinds = tokens.iter().map(|t| t.kind).collect_vec();
        dbg!(&token_kinds);

        assert!(matches!(
            token_kinds.as_slice(),
            &[
                TokenKind::Number(OrderedFloat(12.0), None),
                TokenKind::Space(1),
                TokenKind::Word(_),
                TokenKind::Space(1),
                TokenKind::Word(_),
                TokenKind::Space(1),
                TokenKind::Word(_),
                TokenKind::Space(1),
                TokenKind::Number(OrderedFloat(11.0), None),
                TokenKind::Punctuation(Punctuation::Comma),
                TokenKind::Space(1),
                TokenKind::Word(_),
                TokenKind::Space(1),
                TokenKind::Word(_),
                TokenKind::Space(1),
                TokenKind::Word(_),
                TokenKind::Space(1),
                TokenKind::Word(_),
                TokenKind::Space(1),
                TokenKind::Number(OrderedFloat(11.0), None),
                TokenKind::Punctuation(Punctuation::Bang),
            ]
        ))
    }

    #[test]
    fn math_unlintable() {
        let source = "$12 > 11$, $12 << 11!$";

        let tokens = Typst.parse_str(source);
        let token_kinds = tokens.iter().map(|t| t.kind).collect_vec();
        dbg!(&token_kinds);

        assert!(matches!(
            token_kinds.as_slice(),
            &[
                TokenKind::Unlintable,
                TokenKind::Punctuation(Punctuation::Comma),
                TokenKind::Space(1),
                TokenKind::Unlintable,
            ]
        ))
    }

    #[test]
    fn dict_parsing() {
        let source = r#"#let dict = (
                        name: "Typst",
                        born: 2019,
                      )"#;

        let tokens = Typst.parse_str(source);
        let token_kinds = tokens.iter().map(|t| t.kind).collect_vec();
        dbg!(&token_kinds);

        let charslice = source.chars().collect_vec();
        assert_eq!(tokens[2].span.get_content_string(&charslice), "Typst");

        assert!(matches!(
            token_kinds.as_slice(),
            &[
                TokenKind::Word(_),                            // Identifier
                TokenKind::Word(_),                            // Key 1
                TokenKind::Word(_),                            // Value 1
                TokenKind::Word(_),                            // Key 2
                TokenKind::Number(OrderedFloat(2019.0), None), // Value 2
            ]
        ))
    }

    #[test]
    fn str_parsing() {
        let source = r#"#let ident = "This is a string""#;

        let token_kinds = Typst.parse_str(source).iter().map(|t| t.kind).collect_vec();

        assert!(matches!(
            &token_kinds.as_slice(),
            &[
                TokenKind::Word(_), // Identifier
                TokenKind::Word(_), // This
                TokenKind::Space(1),
                TokenKind::Word(_), // Is
                TokenKind::Space(1),
                TokenKind::Word(_), // A
                TokenKind::Space(1),
                TokenKind::Word(_), // String
            ]
        ))
    }

    #[test]
    fn sentence() {
        let source = "This is a sentence, it is not interesting.";

        let tokens = Typst.parse_str(source);
        let token_kinds = tokens.iter().map(|t| t.kind).collect_vec();
        dbg!(&token_kinds);

        assert!(matches!(
            token_kinds.as_slice(),
            &[
                TokenKind::Word(_),
                TokenKind::Space(1),
                TokenKind::Word(_),
                TokenKind::Space(1),
                TokenKind::Word(_),
                TokenKind::Space(1),
                TokenKind::Word(_),
                TokenKind::Punctuation(Punctuation::Comma),
                TokenKind::Space(1),
                TokenKind::Word(_),
                TokenKind::Space(1),
                TokenKind::Word(_),
                TokenKind::Space(1),
                TokenKind::Word(_),
                TokenKind::Space(1),
                TokenKind::Word(_),
                TokenKind::Punctuation(Punctuation::Period),
            ]
        ))
    }

    #[test]
    fn smart_apostrophe_newline() {
        let source = r#"group’s
writing"#;

        let tokens = Typst.parse_str(source);
        let token_kinds = tokens.iter().map(|t| t.kind).collect_vec();
        dbg!(&token_kinds);

        let charslice = source.chars().collect_vec();
        assert_eq!(tokens[2].span.get_content_string(&charslice), "writing");

        assert!(matches!(
            token_kinds.as_slice(),
            &[
                TokenKind::Word(WordMetadata {
                    noun: Some(NounData {
                        is_possessive: Some(true),
                        ..
                    }),
                    ..
                }),
                TokenKind::Space(1),
                TokenKind::Word(_),
            ]
        ));
    }
}
