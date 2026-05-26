// This file is part of the uutils awk package.
//
// For the full copyright and license information, please view the LICENSE
// files that was distributed with this source code.

//! This module contains the bytecode description, designed to be compact
//! for cache efficiency and isomorphic w.r.t Cranelift IR. Also, our bytecode
//! _is_ our IR; we lower the AST into it and can execute it right away, or do
//! an optimization or JIT pass. We don't do the hack Lua 5's VM does of
//! emitting bytecode without an intermediate AST because AWK contextual
//! shenanigans; _even_ if it was possible, good luck maintaining that.

pub mod lower;

use std::fmt::{Debug, Display};

pub use lower::test_interpreter;

#[derive(Clone, Copy, Debug)]
#[repr(transparent)]
pub struct NonLocal(pub u16);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct Reg(pub u16);

#[derive(Clone, Copy, Debug)]
#[repr(transparent)]
pub struct Label(pub u16);

#[derive(Clone, Copy, Debug)]
#[repr(transparent)]
pub struct ArgCount(u16);

#[repr(u8, C, align(8))]
#[derive(Clone, Copy, Debug)]
pub enum Instruction {
    // Unary operations
    Record(UnaryArg),
    Negation(UnaryArg),
    ToInt(UnaryArg),
    Negative(UnaryArg),
    Copy(UnaryArg),

    // Binary operations
    Eq(BinaryArg),
    NEq(BinaryArg),
    Gt(BinaryArg),
    Lt(BinaryArg),
    LtE(BinaryArg),
    GtE(BinaryArg),
    And(BinaryArg),
    Or(BinaryArg),
    Matches(BinaryArg),
    MatchesNot(BinaryArg),
    Add(BinaryArg),
    Subtract(BinaryArg),
    Multiply(BinaryArg),
    Divide(BinaryArg),
    Raise(BinaryArg),
    Modulo(BinaryArg),
    Concat(BinaryArg),

    // Intrinsic operations
    LoadUser(LoadStoreArg),
    LoadBultin(LoadStoreArg),
    LoadConst(LoadStoreArg),
    StoreUser(LoadStoreArg),
    StoreBuiltin(LoadStoreArg),
    IntrinsicCall(CallArgs),
    UserCall(IndCallArgs),
    IndirectCall(CallArgs),
    Jump(JumpArg),
    Return(RetArg),
    Branch(BranchArg),
}

const _: () = const { assert!(size_of::<Instruction>() <= 8) };

pub type UnaryArg = (Reg, Reg);
pub type BinaryArg = (Reg, Reg, Reg);
pub type LoadStoreArg = (Reg, NonLocal);
pub type JumpArg = Label;
pub type RetArg = Reg;
pub type BranchArg = (Reg, Label, Label);
pub type CallArgs = (Reg, NonLocal, ArgCount);
pub type IndCallArgs = (Reg, Reg, ArgCount);

impl Instruction {
    fn set_label(&mut self, label: Label) {
        match self {
            Self::Jump(lx) | Self::Branch((_, _, lx)) => *lx = label,
            _ => debug_assert!(false, "Incorrect label set!"),
        }
    }

    fn push_start_label(&mut self) {
        if let Self::Branch((_, label, _)) = self {
            label.0 += 1;
        } else {
            debug_assert!(false, "Incorrect label set!");
        }
    }
}

impl Display for Instruction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let op = self.display_name();
        match self {
            Self::Record((dest, data))
            | Self::Negation((dest, data))
            | Self::ToInt((dest, data))
            | Self::Negative((dest, data))
            | Self::Copy((dest, data)) => {
                write!(f, "{dest} <- {op} {data}")
            }
            Self::Eq((dest, lhs, rhs))
            | Self::NEq((dest, lhs, rhs))
            | Self::Gt((dest, lhs, rhs))
            | Self::Lt((dest, lhs, rhs))
            | Self::LtE((dest, lhs, rhs))
            | Self::GtE((dest, lhs, rhs))
            | Self::And((dest, lhs, rhs))
            | Self::Or((dest, lhs, rhs))
            | Self::Matches((dest, lhs, rhs))
            | Self::MatchesNot((dest, lhs, rhs))
            | Self::Add((dest, lhs, rhs))
            | Self::Subtract((dest, lhs, rhs))
            | Self::Multiply((dest, lhs, rhs))
            | Self::Divide((dest, lhs, rhs))
            | Self::Raise((dest, lhs, rhs))
            | Self::Concat((dest, lhs, rhs))
            | Self::Modulo((dest, lhs, rhs)) => {
                write!(f, "{dest} <- {op} {lhs}, {rhs}")
            }
            Self::LoadUser((dest, src)) | Self::StoreUser((dest, src)) => {
                write!(f, "{dest} <- {op} user[{src}]")
            }
            Self::LoadConst((dest, src)) => {
                write!(f, "{dest} <- {op} mem[{src}]")
            }
            Self::StoreBuiltin((dest, src)) | Self::LoadBultin((dest, src)) => {
                write!(f, "{dest} <- {op} intrinsic[{src}]")
            }
            Self::Branch((cond, label_then, label_else)) => {
                write!(f, "{op} {cond}, {label_then}, {label_else}")
            }
            Self::Jump(label) => {
                write!(f, "{op} {label}")
            }
            _ => todo!(),
        }
    }
}

impl Instruction {
    fn display_name(self) -> &'static str {
        match self {
            Self::Record(_) => "rec",
            Self::Negation(_) => "not",
            Self::ToInt(_) => "int",
            Self::Negative(_) => "neg",
            Self::Concat(_) => "cat",
            Self::Eq(_) => "eq",
            Self::NEq(_) => "neq",
            Self::Gt(_) => "gt",
            Self::Lt(_) => "lt",
            Self::LtE(_) => "le",
            Self::GtE(_) => "ge",
            Self::And(_) => "and",
            Self::Or(_) => "or",
            Self::Matches(_) => "mtch",
            Self::MatchesNot(_) => "nmtch",
            Self::Add(_) => "add",
            Self::Subtract(_) => "sub",
            Self::Multiply(_) => "mul",
            Self::Divide(_) => "div",
            Self::Raise(_) => "pow",
            Self::Modulo(_) => "mod",
            Self::LoadUser(_) => "vload",
            Self::LoadBultin(_) => "iload",
            Self::LoadConst(_) => "cload",
            Self::StoreUser(_) => "vstore",
            Self::StoreBuiltin(_) => "istore",
            Self::Copy(_) => "cpy",
            Self::IntrinsicCall(_) => "icall",
            Self::UserCall(_) => "ucall",
            Self::IndirectCall(_) => "vcall",
            Self::Jump(_) => "jmp",
            Self::Return(_) => "ret",
            Self::Branch(_) => "brif",
        }
    }
}

impl Display for Label {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        <_ as Display>::fmt(&self.0, f)
    }
}

impl Display for Reg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "r{}", self.0)
    }
}

impl Display for NonLocal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
