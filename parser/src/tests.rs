use std::fmt::Debug;

use bumpalo::Bump;

use crate::{Ast, Lexer, Parser};

fn parse<'a, T: Debug>(
    source: &'a str,
    arena: &'a Bump,
    selector: impl for<'b> FnOnce(&'b Ast<'a>) -> &'b T,
) -> super::Result<String> {
    let mut parser = Parser::new(arena);
    parser
        .parse_top(&mut Lexer::new(source.as_bytes(), arena), true)
        .map(|x| format!("{:?}", selector(x)))
}

#[test]
fn test_if() {
    let arena = Bump::new();
    parse("{ if (x == 2) print y; }", &arena, |x| x).unwrap();
}
