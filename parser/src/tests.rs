// This file is part of the uutils awk package.
//
// For the full copyright and license information, please view the LICENSE
// files that was distributed with this source code.

use bumpalo::Bump;

use crate::{Ast, Lexer, Parser};

fn parse<'a>(source: &'a str, arena: &'a Bump) -> super::Result<&'a Ast<'a>> {
    let parser = arena.alloc(Parser::new(arena));
    parser.parse_top(&mut Lexer::new(source.as_bytes(), arena), true)
}

// Behold! The Holy Macro to rule them all.
macro_rules! test_parser {
    (
        $code:expr => {
            $(loads:      $loads:expr,)?
            $(begin:      $begin:expr,)?
            $(end:        $end:expr,)?
            $(begin_file: $begin_file:expr,)?
            $(end_file:   $end_file:expr,)?
            $(rules:      $rules:expr,)?
            $(concurrent: $concurrent:expr,)?
            $(functions:  $functions:expr,)?
        }
    ) => {
        let arena = Bump::new();
        let code = parse($code, &arena).unwrap();

        #[allow(unused_mut, unused_assignments)]
        let _ = {
            use ::std::{option::Option, primitive::str, assert_eq, format};

            let mut loads:      &[&str]                         = &[];
            let mut end:        &[&str]                         = &[];
            let mut begin:      &[&str]                         = &[];
            let mut begin_file: &[&str]                         = &[];
            let mut end_file:   &[&str]                         = &[];
            let mut rules:      &[(Option<&str>, Option<&str>)] = &[];
            let mut concurrent: &[(Option<&str>, Option<&str>)] = &[];
            let mut functions:  &[(&str, &[&str], &str)]        = &[];

            $(loads      = &$loads;)?
            $(end        = &$end;)?
            $(begin      = &$begin;)?
            $(begin_file = &$begin_file;)?
            $(end_file   = &$end_file;)?
            $(rules      = &$rules;)?
            $(concurrent = &$concurrent;)?
            $(functions  = &$functions;)?

            test_parser!(
                @internal check |(a, b)| assert_eq!(a.as_bytes(), b.as_ref());
                loads => code.loads
            );
            test_parser!(
                @internal munch check_for_each code;
                |(&a, b)| assert_eq!(a, &format!("{b:?}"));
                begin, end, begin_file, end_file
            );
            test_parser!(
                @internal munch check_for_each code;
                |((e_pattern, e_actions), b)| {
                    assert_eq!(
                        *e_pattern,
                        b.pattern.as_ref().map(|x| format!("{x:?}")).as_deref()
                    );
                    assert_eq!(
                        *e_actions,
                        b.actions.as_ref().map(|x| format!("{x:?}")).as_deref()
                    );
                };
                rules, concurrent
            );
            test_parser!(
                @internal check |((e_name, e_args, e_body), (name, fun))| {
                    assert_eq!(e_name, &format!("{name:?}"));
                    test_parser!(@internal check
                        |(&a, b)| assert_eq!(a, &format!("{b:?}"));
                        e_args => fun.args
                    );
                    assert_eq!(*e_body, format!("{:?}", fun.body));
                };
                functions => code.functions
            );
        };
    };
    (is_err!($($code:expr),*)) => {
        let arena = Bump::new();
        assert!([$($code),*].into_iter().all(|e| parse(e, &arena).is_err()));
    };
    (@internal check $lambda:expr; $a:expr => $b:expr) => {
        assert_eq!($a.len(), $b.len());
        $a.into_iter().zip(&$b).for_each($lambda);
    };
    (@internal check_for_each $code:ident; $lambda:expr; $a:ident) => {
        test_parser!(@internal check $lambda; $a => $code.$a);
    };
    (@internal munch $method:ident $code:ident; $lambda:expr; $arg:ident, $($rest:tt)*) => {
        test_parser!(@internal $method $code; $lambda; $arg);
        test_parser!(@internal munch $method $code; $lambda; $($rest)*);
    };
    (@internal munch $method:ident $code:ident; $lambda:expr; $arg:ident) => {
        test_parser!(@internal $method $code; $lambda; $arg);
    };
    (@internal munch $method:ident $code:ident; $lambda:expr;) => {};
}

#[test]
fn test_parser_meta_holy_macro() {
    let source = "
        @load \"lib_foo.1\";
        @load \"lib_bar.so\";

        BEGIN { print 1 + 1 }
        BEGIN { 2 + 2 == 4\nprint \"foo\" }
        { if (a) print 2; }
        $0 == \"lisp would be proud\";
        function foo(a, b) { print a ? b : c }
    ";
    test_parser!(source => {
        loads: ["lib_foo.1", "lib_bar.so"],
        begin: [
            "(body (Print (Add 1 1)))",
            "(body (Eq (Add 2 2) 4) (Print \"foo\"))"
        ],
        rules: [
            (None, Some("(body (if awk::a (body (Print 2))))")),
            (Some("(Eq (Record 0) \"lisp would be proud\")"), None),
        ],
        functions: [
            (
                "awk::foo",
                &["awk::a", "awk::b"],
                "(body (Print (?: awk::a awk::b awk::c)))"
            )
        ],
    });
}

#[test]
fn test_parser_valid_patterns() {
    let source = "
        BEGIN { print }
        END { print }
        BEGINFILE { print }
        ENDFILE { print }
        $0 == 1 && /x/ { print }
        /abc/ { print }
        !$0, x::a ? b : c { print }
        awk;
        1 + 1 \n { print }
        { print }
    ";
    const BODY: &str = "(body (Print))";
    test_parser!(source => {
        begin: [BODY],
        end: [BODY],
        begin_file: [BODY],
        end_file: [BODY],
        rules: [
            (Some("(And (Eq (Record 0) 1) /x/)"), Some(BODY)),
            (Some("/abc/"), Some(BODY)),
            (
                Some("(Range (Negation (Record 0)) (?: x::a awk::b awk::c))"),
                Some(BODY)
            ),
            (Some("awk::awk"), None),
            (Some("(Add 1 1)"), None),
            (None, Some(BODY)),
            (None, Some(BODY)),
        ],
    });
}

#[test]
fn test_parser_invalid_patterns() {
    test_parser!(is_err!("BEGIN", "END", "BEGINFILE", "ENDFILE", "print 1;"));
}

#[test]
fn test_parser_non_assoc() {
    test_parser!(is_err!(
        "a == b == c",
        "a != b != c",
        "a > b > c",
        "a < b < c",
        "a >= b >= c",
        "a <= b <= c"
    ));
}

#[test]
fn test_parser_relaxed_assignments() {
    let source = "
        { 1 + 0 && x = 1 }
        { y = @/a/ ? b : c }
        { 1 + 0 || z = @/a/ ? b : c }
        { 1 + 0 || z = /a/ && b || c }
    ";
    test_parser!(
        source => {
            rules: [
                (None, Some("(body (And (Add 1 0) (Assignment awk::x 1)))")),
                (None, Some("(body (?: (Assignment awk::y @/a/) awk::b awk::c))")),
                (None, Some("(body (?: (Or (Add 1 0) (Assignment awk::z @/a/)) awk::b awk::c))")),
                (
                    None,
                    Some("(body (Or (Add 1 0) (Assignment awk::z (Or (And /a/ awk::b) awk::c))))"),
                ),
            ],
        }
    );
}

#[test]
fn test_parser_inc_dec() {
    let source = r#"
        { ++a $0-- }
        { --a[2] ++$(1 + 1) }
        { a++ a["x"]-- }
        { --a $"a"++ }
    "#;
    test_parser!(source => {
        rules: [
            (None, Some("(body (Concat (IncrementL awk::a) (DecrementR (Record 0))))")),
            (
                None,
                Some("(body (Concat (DecrementL (ArrayAccess awk::a 2)) (IncrementL (Record (Add 1 1)))))")
            ),
            (None, Some("(body (Concat (IncrementR awk::a) (DecrementR (ArrayAccess awk::a \"x\"))))")),
            (None, Some("(body (Concat (DecrementL awk::a) (IncrementR (Record \"a\"))))")),
        ],
    });
    // these should parse as (Cat (--L (++R $0)) a), or otherwise error out.
    // FIXME: not treated as errors yet.
    // test_parser!(is_err!("{ $0++ --a }", "{ ++$0 ++a }"));
}
