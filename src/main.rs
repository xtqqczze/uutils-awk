// This file is part of the uutils awk package.
//
// For the full copyright and license information, please view the LICENSE
// files that was distributed with this source code.

// static POSIX: bool = false;

mod utils;

use bumpalo::Bump;
use color_eyre::Result;
use parser::{Lexer, Parser};

use crate::utils::{ensure_consistent_panic, exit_err};

fn main() {
    if let Err(e) = ensure_consistent_panic(uu_main) {
        exit_err(e)
    }
}

#[tracing::instrument]
fn uu_main() -> Result<()> {
    let arg = std::env::args().nth(1).unwrap();

    let arena = Bump::with_capacity(4000); // 4KB minus metadata-ish
    let mut lex = Lexer::new(arg.as_bytes());
    let mut parser = Parser::new(&arena);
    let ast = dbg!(parser.parse(&mut lex, true).unwrap());
    let arena2 = Bump::new();
    arena2.alloc_with(|| ast.clone());
    dbg!(arena.chunk_capacity());

    // for token in lex {
    //     let Ok(x) = token else {
    //         return token.map(drop).map_err(color_eyre::Report::from);
    //     };
    //     println!("{x:?}");
    // }
    // exit_with(Interpreter.run())
    Ok(())
}
