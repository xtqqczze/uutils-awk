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
use parser::{Command, Redirection};

#[derive(Clone, Copy, Debug)]
#[repr(transparent)]
pub struct NonLocal(pub u16);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct Reg(pub u8);

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
    LoadUserScalar(MemArg),
    LoadUserArray(MemArgRange),
    LoadUserMDimArray(MemArgRange),
    LoadBuiltinScalar(MemArg),
    LoadBuiltinArray(MemArgRange),
    LoadConst(MemArg),
    StoreUserScalar(MemArgImm),
    StoreBuiltinScalar(MemArgImm),
    StoreUserArray(MemArgRange),
    StoreUserMDimArray(MemArgRange),
    StoreBuiltinArray(MemArgRange),
    StoreRecord(BinaryArg),
    IntrinsicCall(CallArgs),
    OutputCall(OutputCallArgs),
    UserCall(IndCallArgs),
    IndirectCall(CallArgs),
    Jump(JumpArg),
    Return(RetArg),
    Branch(BranchArg),
}

const _: () = const { assert!(size_of::<Instruction>() <= 8) };

pub type UnaryArg = (Reg, MaybeImm);
pub type BinaryArg = (Reg, MaybeImm, MaybeImm);
pub type MemArgImm = (Reg, MaybeImm, NonLocal);
pub type MemArgRange = (Reg, Reg, Reg, NonLocal);
pub type MemArg = (Reg, NonLocal);
pub type JumpArg = Label;
pub type RetArg = MaybeImm;
pub type BranchArg = (MaybeImm, Label, Label);
pub type CallArgs = (Reg, Reg, NonLocal);
pub type OutputCallArgs = (Reg, Reg, Command, Option<Redirection>);
pub type IndCallArgs = (Reg, Reg, Reg);

#[repr(u8)]
#[derive(Clone, Copy, Debug)]
pub enum MaybeImm {
    Reg(Reg),
    Rec(i8),
    Imm(i8),
    ImmCnt(u8),
    ImmUserVar(u8),
    ImmBuiltinVar(u8),
}

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
            Self::LoadUserScalar((dest, src)) => {
                write!(f, "{dest} <- {op} user[{src}]")
            }
            Self::StoreUserScalar((dest, imm, src)) => {
                write!(f, "{dest} <- {op} user[{src}], {imm}")
            }
            Self::LoadUserArray((dest, start, end, src))
            | Self::StoreUserArray((dest, start, end, src)) => {
                write!(f, "{dest} <- {op} user[{start}..{end}], {src}")
            }
            Self::LoadUserMDimArray((dest, start, end, src))
            | Self::StoreUserMDimArray((dest, start, end, src)) => {
                write!(f, "{dest} <- {op} user[{src}], {start}..{end}")
            }
            Self::LoadConst((dest, src)) => {
                write!(f, "{dest} <- {op} mem[{src}]")
            }
            Self::StoreBuiltinScalar((dest, imm, src)) => {
                write!(f, "{dest} <- {op} intrinsic[{src}], {imm}")
            }
            Self::LoadBuiltinScalar((dest, src)) => {
                write!(f, "{dest} <- {op} intrinsic[{src}]")
            }
            Self::StoreRecord((dest, src, imm)) => {
                write!(f, "{dest} <- {op} {src}, {imm}")
            }
            Self::LoadBuiltinArray((dest, start, end, src))
            | Self::StoreBuiltinArray((dest, start, end, src)) => {
                write!(f, "{dest} <- {op} intrinsic[{src}], {start}..{end}")
            }
            Self::Branch((cond, label_then, label_else)) => {
                write!(f, "{op} {cond}, {label_then}, {label_else}")
            }
            Self::Jump(label) => {
                write!(f, "{op} {label}")
            }
            Self::Return(src) => {
                write!(f, "{op} {src}")
            }
            Self::IntrinsicCall((dest, code, args)) | Self::IndirectCall((dest, code, args)) => {
                write!(f, "{dest} <- {op} {code}, {args}")
            }
            Self::OutputCall((start, end, call, Some(redir))) => {
                write!(f, "{call}{redir:?} {start}..{end}")
            }
            Self::OutputCall((start, end, call, None)) => {
                write!(f, "{call} {start}..{end}")
            }
            Self::UserCall((dest, src, args)) => {
                write!(f, "{dest} <- {op} {src}, {args}")
            }
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
            Self::LoadUserScalar(_) => "vsload",
            Self::LoadBuiltinScalar(_) => "isload",
            Self::LoadUserArray(_) => "vaload",
            Self::LoadUserMDimArray(_) => "vvload",
            Self::LoadBuiltinArray(_) => "iaload",
            Self::LoadConst(_) => "cload",
            Self::StoreUserScalar(_) => "vsstore",
            Self::StoreRecord(_) => "rsstore",
            Self::StoreBuiltinScalar(_) => "isstore",
            Self::StoreUserArray(_) => "vastore",
            Self::StoreUserMDimArray(_) => "vvstore",
            Self::StoreBuiltinArray(_) => "iastore",
            Self::Copy(_) => "cpy",
            Self::IntrinsicCall(_) => "icall",
            Self::UserCall(_) => "ucall",
            Self::IndirectCall(_) => "vcall",
            Self::OutputCall(_) => "out",
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
        <_ as Display>::fmt(&self.0, f)
    }
}

impl Display for ArgCount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        <_ as Display>::fmt(&self.0, f)
    }
}

impl Display for MaybeImm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Reg(reg) => <_ as Display>::fmt(reg, f),
            Self::Rec(rec) => write!(f, "rec[{rec}]"),
            Self::Imm(imm) => write!(f, "imm({imm})"),
            Self::ImmCnt(imm) => write!(f, "mem[{imm}]"),
            Self::ImmUserVar(imm) => write!(f, "user[{imm}]"),
            Self::ImmBuiltinVar(imm) => write!(f, "intrinsic[{imm}]"),
        }
    }
}
