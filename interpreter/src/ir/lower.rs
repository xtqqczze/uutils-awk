// This file is part of the uutils awk package.
//
// For the full copyright and license information, please view the LICENSE
// files that was distributed with this source code.

use std::{borrow::Cow, mem::forget, ops::Deref};

use bumpalo::{Bump, collections::Vec};
use parser::{
    Atom, BinaryOperator, BinaryPlaceOperator, Body, Expr, ExprNode, Place, SimpleStatement,
    Statement, UnaryOperator, UnaryPlaceOperator, Variable,
};

use crate::{
    ir::{BinaryArg, Instruction, Label, MaybeImm, NonLocal, Reg, UnaryArg},
    types::Value,
    vm::{Consts, ExecMode, Interpreter, SymbolTable},
};

#[derive(Debug)]
pub struct Code<'arena> {
    pub arena: &'arena Bump,
    pub bc: Bytecode<'arena>,
    pub consts: Consts<'arena>,
    pub symbols: SymbolTable<'arena>,
    free_regs: Vec<'arena, Reg>,
    pub reg_pointer: u8,
}

#[must_use]
#[derive(Debug)]
#[repr(transparent)]
struct LinearReg(Reg);

#[must_use]
#[derive(Debug)]
enum Operand {
    Imm(MaybeImm),
    Reg(LinearReg),
}

impl<'a> Code<'a> {
    fn lower_body(&mut self, body: &Body) {
        for stmnt in &body.0 {
            self.lower_statement(stmnt);
        }
    }

    fn lower_statement(&mut self, stmnt: &Statement) {
        match stmnt {
            Statement::If { condition, then_body, else_body } => {
                let mut state = RegsState::new(self);
                let condition = self.lower_expr(condition);
                let label_then = self.following_instr(1);
                let if_label = self.bc.emit(Instruction::Branch((
                    condition.to_mi(),
                    label_then,
                    Label(0),
                )));
                condition.free(self);
                self.lower_body(then_body);
                let next = self.following_instr(0);
                self.bc.nth(if_label).set_label(next);

                if let Some(else_body) = else_body {
                    state.reg_pointer =
                        state.reg_pointer.checked_add(1).expect("register overflow");
                    self.bc.nth(if_label).push_start_label();
                    let end_label = self.bc.emit(Instruction::Jump(Label(0)));
                    state.scope_hwm(self, |c| c.lower_body(else_body));
                    let next = self.following_instr(0);
                    self.bc.nth(end_label).set_label(next);
                }
            }
            Statement::While { condition, then_body } => {
                let cond_label = self.following_instr(0);
                let condition = self.lower_expr(condition);
                let while_label = self.bc.emit(Instruction::Branch((
                    condition.to_mi(),
                    self.following_instr(1),
                    Label(0),
                )));
                condition.free(self);
                self.lower_body(then_body);
                self.bc.emit(Instruction::Jump(cond_label));
                let next = self.following_instr(0);
                self.bc.nth(while_label).set_label(next);
            }
            Statement::DoWhile { then_body, condition } => {
                let do_label = self.following_instr(0);
                self.lower_body(then_body);
                let condition = self.lower_expr(condition);
                self.bc.emit(Instruction::Branch((
                    condition.to_mi(),
                    do_label,
                    self.following_instr(1),
                )));
                condition.free(self);
            }
            Statement::For { init, condition, update, body } => {
                if let Some(SimpleStatement::Expression(expr)) = init {
                    let value = self.lower_expr(expr);
                    value.free(self);
                }
                let cond_label = self.following_instr(0);
                if let Some(condition) = condition {
                    let condition = self.lower_expr(condition);
                    let body_label = self.following_instr(1);
                    let while_label = self.bc.emit(Instruction::Branch((
                        condition.to_mi(),
                        body_label,
                        Label(0),
                    )));
                    condition.free(self);
                    self.lower_body(body);
                    if let Some(SimpleStatement::Expression(expr)) = update {
                        let value = self.lower_expr(expr);
                        value.free(self);
                    }
                    self.bc.emit(Instruction::Jump(cond_label));
                    let next = self.following_instr(0);
                    self.bc.nth(while_label).set_label(next);
                } else {
                    self.lower_body(body);
                    if let Some(SimpleStatement::Expression(expr)) = update {
                        let value = self.lower_expr(expr);
                        value.free(self);
                    }
                    self.bc.emit(Instruction::Jump(cond_label));
                }
            }
            Statement::Simple(SimpleStatement::Expression(expr)) => {
                let value = self.lower_expr(expr);
                value.free(self);
            }
            Statement::Simple(SimpleStatement::Command { name, args, redirection }) => {
                let (call_start, call_end, redir) = self.gen_call_convention(args, |this| {
                    redirection.as_ref().map(|(r, expr)| {
                        let redir_reg = this.alloc_reg();
                        this.lower_expr_into(expr, *redir_reg);
                        this.free_reg(redir_reg);
                        *r
                    })
                });
                self.bc.emit(Instruction::OutputCall((
                    call_start, call_end, *name, redir,
                )));
            }
            _ => todo!(),
        }
    }

    fn lower_expr(&mut self, expr: &Expr) -> Operand {
        match expr {
            Expr::Leaf(atom) => self.lower_atom(atom),
            Expr::Node(_) => {
                let dest = self.alloc_reg();
                self.lower_expr_into(expr, *dest);
                Operand::Reg(dest)
            }
        }
    }

    fn lower_atom(&mut self, atom: &Atom) -> Operand {
        let dest = self.alloc_reg();
        match self.lower_atom_mi(atom, *dest) {
            MaybeImm::Reg(reg) => {
                debug_assert_eq!(reg, *dest);
                Operand::Reg(dest)
            }
            imm => {
                self.free_reg(dest);
                Operand::Imm(imm)
            }
        }
    }

    fn lower_atom_mi(&mut self, atom: &Atom, dest: Reg) -> MaybeImm {
        match atom {
            Atom::Variable(Variable::User(ident)) => {
                let src = self.symbols.register_user_var(ident, self.arena);
                MaybeImm::from_vs(self, dest, src)
            }
            Atom::Variable(var) => MaybeImm::from_is(self, dest, var),
            Atom::SmallInt(n) => MaybeImm::Imm(*n),
            Atom::Number(n) => MaybeImm::from_cnt(self, dest, Value::Float(*n)),
            atom @ (Atom::String(s) | Atom::TypedRegex(s)) => {
                let val = if matches!(atom, Atom::String(_)) {
                    Value::String
                } else {
                    Value::Regex
                };
                let buf = self.arena.alloc_slice_copy(s.as_ref());
                MaybeImm::from_cnt(self, dest, val(Cow::Borrowed(buf)))
            }
            Atom::Regex(r) => {
                let buf = &*self.arena.alloc_slice_copy(r.as_ref());
                let rhs = MaybeImm::from_cnt(self, dest, Value::Regex(buf.into()));
                self.bc
                    .emit(Instruction::Matches((dest, MaybeImm::Rec(0), rhs)));
                MaybeImm::Reg(dest)
            }
            _ => todo!(),
        }
    }

    fn lower_atom_into(&mut self, atom: &Atom, dest: Reg) {
        let mi = self.lower_atom_mi(atom, dest);
        match mi {
            MaybeImm::Reg(reg) if reg == dest => {}
            other => {
                self.bc.emit(Instruction::Copy((dest, other)));
            }
        }
    }

    fn lower_expr_into(&mut self, expr: &Expr, dest: Reg) {
        match expr {
            Expr::Leaf(atom) => self.lower_atom_into(atom, dest),
            Expr::Node(node) => match node.as_ref() {
                ExprNode::UnaryOperation(op, expr) => {
                    let src = self.lower_expr(expr);
                    self.bc.emit(Instruction::from((*op, (dest, src.to_mi()))));
                    src.free(self);
                }
                ExprNode::BinaryOperation(op, lhs, rhs) => {
                    let lhs = self.lower_expr(lhs);
                    let rhs = self.lower_expr(rhs);
                    self.bc
                        .emit(Instruction::from((*op, (dest, lhs.to_mi(), rhs.to_mi()))));
                    lhs.free(self);
                    rhs.free(self);
                }
                ExprNode::Ternary(cond, true_then, false_then) => {
                    let cond = self.lower_expr(cond);
                    let branch = self.bc.emit(Instruction::Branch((
                        cond.to_mi(),
                        self.following_instr(1),
                        Label(0),
                    )));
                    cond.free(self);

                    let mut state = RegsState::new(self);
                    state = state
                        .scope(self, |c| {
                            c.lower_expr_into(true_then, dest);
                        })
                        .0;

                    let jump = self.bc.emit(Instruction::Jump(Label(0)));
                    let label = self.following_instr(0);

                    state.scope_hwm(self, |c| {
                        c.lower_expr_into(false_then, dest);
                    });

                    self.bc.nth(branch).set_label(label);
                    let next = self.following_instr(0);
                    self.bc.nth(jump).set_label(next);
                }
                ExprNode::BinaryPlaceOperation(op, place, expr) => {
                    let val = self.lower_expr(expr);

                    let second_op = match op {
                        BinaryPlaceOperator::Assignment => {
                            self.store_place(place, dest, val.to_mi());
                            val.free(self);
                            return;
                        }
                        BinaryPlaceOperator::AddAssign => Instruction::Add,
                        BinaryPlaceOperator::SubAssign => Instruction::Subtract,
                        BinaryPlaceOperator::MulAssign => Instruction::Multiply,
                        BinaryPlaceOperator::DivAssign => Instruction::Divide,
                        BinaryPlaceOperator::PowAssign => Instruction::Raise,
                        BinaryPlaceOperator::ModAssign => Instruction::Modulo,
                    };
                    let lhs_reg = self.alloc_reg();
                    let lhs = self.load_place(*lhs_reg, place);

                    self.bc.emit(second_op((dest, lhs, val.to_mi())));
                    self.store_place(place, dest, dest.into());

                    self.free_reg(lhs_reg);
                    val.free(self);
                }
                ExprNode::UnaryPlaceOperation(op, place) => {
                    // Note: val may alias with dest.
                    let val = self.load_place(dest, place);

                    let second_op = match op {
                        UnaryPlaceOperator::IncrementL | UnaryPlaceOperator::IncrementR => {
                            Instruction::Add
                        }
                        UnaryPlaceOperator::DecrementL | UnaryPlaceOperator::DecrementR => {
                            Instruction::Subtract
                        }
                    };
                    match op {
                        UnaryPlaceOperator::IncrementL | UnaryPlaceOperator::DecrementL => {
                            self.bc.emit(second_op((dest, val, MaybeImm::Imm(1))));
                            self.store_place(place, dest, dest.into());
                        }
                        UnaryPlaceOperator::IncrementR | UnaryPlaceOperator::DecrementR => {
                            self.bc
                                .emit(Instruction::Add((dest, val, MaybeImm::Imm(0))));
                            let tmp = self.alloc_reg();
                            self.bc.emit(second_op((*tmp, val, MaybeImm::Imm(1))));
                            self.store_place(place, *tmp, (*tmp).into());
                            self.free_reg(tmp);
                        }
                    }
                }
                _ => todo!(),
            },
        }
    }

    fn load_place(&mut self, dest: Reg, place: &Place<'_>) -> MaybeImm {
        match place {
            Place::Record(_) => {
                todo!()
            }
            Place::Variable(Variable::User(ident)) => {
                let src = self.symbols.register_user_var(ident, self.arena);
                MaybeImm::from_vs(self, dest, src)
            }
            Place::Variable(var) => MaybeImm::from_is(self, dest, var),
            Place::Index(Variable::User(ident), index) => {
                let src = self.symbols.register_user_var(ident, self.arena);
                let (start, end, _) = self.gen_call_convention(index, |_| ());
                self.bc
                    .emit(Instruction::LoadUserArray((dest, start, end, src)));
                MaybeImm::Reg(dest)
            }
            Place::Index(var, index) => {
                let (start, end, _) = self.gen_call_convention(index, |_| ());
                self.bc.emit(Instruction::LoadBuiltinArray((
                    dest,
                    start,
                    end,
                    var_index(var),
                )));
                MaybeImm::Reg(dest)
            }
            Place::ChainedIndex(_, _) => todo!(),
        }
    }

    fn store_place(&mut self, place: &Place<'_>, dest: Reg, src: MaybeImm) {
        match place {
            Place::Record(expr) => {
                let rec = self.lower_expr(expr);
                self.bc
                    .emit(Instruction::StoreRecord((dest, rec.to_mi(), src)));
                rec.free(self);
            }
            Place::Variable(Variable::User(ident)) => {
                self.bc.emit(Instruction::StoreUserScalar((
                    dest,
                    src,
                    self.symbols.register_user_var(ident, self.arena),
                )));
            }
            Place::Variable(var) => {
                self.bc
                    .emit(Instruction::StoreBuiltinScalar((dest, src, var_index(var))));
            }
            Place::Index(Variable::User(ident), index) => {
                let place = self.symbols.register_user_var(ident, self.arena);
                let (start, end, _) = self.gen_call_convention(index, |_| ());
                self.bc
                    .emit(Instruction::StoreUserArray((dest, start, end, place)));
            }
            Place::Index(var, index) => {
                let (start, end, _) = self.gen_call_convention(index, |_| ());
                self.bc.emit(Instruction::StoreBuiltinArray((
                    dest,
                    start,
                    end,
                    var_index(var),
                )));
            }
            Place::ChainedIndex(_, _) => todo!(),
        }
    }

    fn alloc_reg(&mut self) -> LinearReg {
        self.free_regs.pop().map(LinearReg).unwrap_or_else(|| {
            let current = self.reg_pointer;
            self.reg_pointer = self.reg_pointer.checked_add(1).expect("register overflow");
            LinearReg(Reg(current))
        })
    }

    fn gen_call_convention<T>(
        &mut self,
        args: &[Expr<'_>],
        extra: impl FnOnce(&mut Code) -> T,
    ) -> (Reg, Reg, T) {
        RegsState::new(self)
            .scope(self, |this| {
                let call_start = this.reg_pointer;
                // TODO: Nicer error reporting.
                let args_len: u8 = args.len().try_into().expect("too many call args");
                let call_end = call_start.checked_add(args_len).expect("register overflow");

                this.reg_pointer = call_end;
                for (i, arg) in args.iter().enumerate() {
                    let offset = i as u8;
                    let reg = Reg(call_start.checked_add(offset).expect("register overflow"));
                    this.lower_expr_into(arg, reg);
                }
                (Reg(call_start), Reg(call_end), extra(this))
            })
            .1
    }

    fn free_reg(&mut self, reg: LinearReg) {
        self.free_regs.push(reg.into_inner());
    }

    fn register_const(&mut self, value: Value<'a>) -> NonLocal {
        NonLocal(self.consts.0.insert_full(value).0 as u16)
    }

    fn following_instr(&self, nth: u16) -> Label {
        Label(self.bc.len() + nth)
    }
}

#[derive(Debug, Clone)]
pub struct Bytecode<'a> {
    pub code: Vec<'a, Instruction>,
}

#[derive(Clone, Debug)]
struct RegsState {
    reg_pointer: u8,
    n_free_regs: usize,
}

impl<'a> Bytecode<'a> {
    fn new_in(bump: &'a Bump) -> Self {
        Self { code: Vec::with_capacity_in(64, bump) }
    }

    #[inline(always)]
    fn emit(&mut self, code: Instruction) -> Label {
        self.code.push(code);
        Label((self.code.len() - 1) as u16)
    }

    fn len(&self) -> u16 {
        self.code.len() as u16
    }

    fn nth(&mut self, label: Label) -> &mut Instruction {
        &mut self.code[label.0 as usize]
    }
}

impl RegsState {
    fn new(code: &Code) -> Self {
        Self {
            reg_pointer: code.reg_pointer,
            n_free_regs: code.free_regs.len(),
        }
    }
    fn scope<T>(self, code: &mut Code, f: impl FnOnce(&mut Code) -> T) -> (Self, T) {
        let ret = f(code);
        let old = code.reg_pointer;
        code.reg_pointer = self.reg_pointer;
        code.free_regs.truncate(self.n_free_regs);
        (Self { reg_pointer: old, ..self }, ret)
    }
    fn scope_hwm<T>(self, code: &mut Code, f: impl FnOnce(&mut Code) -> T) {
        f(code);
        code.reg_pointer = code.reg_pointer.max(self.reg_pointer);
        code.free_regs.truncate(self.n_free_regs);
    }
}

pub fn test_interpreter(stmnt: &Body<'_>) -> String {
    let bump = Bump::with_capacity(16384);
    let mut c = Code {
        arena: &bump,
        bc: Bytecode::new_in(&bump),
        consts: Consts::new_in(&bump),
        symbols: SymbolTable::new_in(&bump),
        reg_pointer: 0,
        free_regs: Vec::new_in(&bump),
    };
    c.lower_body(stmnt);
    let mut vm = Interpreter::new(ExecMode::Uu, c);
    vm.run();
    vm.to_string()
}

impl From<(UnaryOperator, UnaryArg)> for Instruction {
    fn from((op, args): (UnaryOperator, UnaryArg)) -> Self {
        match op {
            UnaryOperator::Record => Self::Record(args),
            UnaryOperator::Negation => Self::Negation(args),
            UnaryOperator::ToInt => Self::ToInt(args),
            UnaryOperator::Negative => Self::Negative(args),
        }
    }
}

impl From<(BinaryOperator, BinaryArg)> for Instruction {
    fn from((op, args): (BinaryOperator, BinaryArg)) -> Self {
        match op {
            BinaryOperator::Concat => Self::Concat(args),
            BinaryOperator::Eq => Self::Eq(args),
            BinaryOperator::NEq => Self::NEq(args),
            BinaryOperator::Gt => Self::Gt(args),
            BinaryOperator::Lt => Self::Lt(args),
            BinaryOperator::LtE => Self::LtE(args),
            BinaryOperator::GtE => Self::GtE(args),
            BinaryOperator::And => Self::And(args),
            BinaryOperator::Or => Self::Or(args),
            BinaryOperator::Matches => Self::Matches(args),
            BinaryOperator::MatchesNot => Self::MatchesNot(args),
            BinaryOperator::Add => Self::Add(args),
            BinaryOperator::Subtract => Self::Subtract(args),
            BinaryOperator::Multiply => Self::Multiply(args),
            BinaryOperator::Divide => Self::Divide(args),
            BinaryOperator::Raise => Self::Raise(args),
            BinaryOperator::Modulo => Self::Modulo(args),
        }
    }
}

// trait Fold<T> {
//     type Args;
//     fn fold(&self, args: Self::Args) -> T;
// }

impl LinearReg {
    fn into_inner(self) -> Reg {
        let inner = self.0;
        forget(self);
        inner
    }
}

impl Operand {
    fn to_mi(&self) -> MaybeImm {
        match self {
            &Self::Imm(imm) => imm,
            Self::Reg(reg) => reg.0.into(),
        }
    }

    fn free(self, code: &mut Code) {
        if let Self::Reg(reg) = self {
            code.free_reg(reg);
        }
    }
}

impl MaybeImm {
    fn from_vs(code: &mut Code<'_>, dest: Reg, src: NonLocal) -> Self {
        if let Ok(src) = u8::try_from(src.0) {
            Self::ImmUserVar(src)
        } else {
            code.bc.emit(Instruction::LoadUserScalar((dest, src)));
            Self::Reg(dest)
        }
    }

    fn from_is(code: &mut Code<'_>, dest: Reg, var: &Variable<'_>) -> Self {
        let src = var_index(var);
        if let Ok(src) = u8::try_from(src.0) {
            Self::ImmBuiltinVar(src)
        } else {
            code.bc.emit(Instruction::LoadBuiltinScalar((dest, src)));
            Self::Reg(dest)
        }
    }

    fn from_cnt<'a>(code: &mut Code<'a>, dest: Reg, value: Value<'a>) -> Self {
        let src = code.register_const(value);
        if let Ok(src) = u8::try_from(src.0) {
            Self::ImmCnt(src)
        } else {
            code.bc.emit(Instruction::LoadConst((dest, src)));
            Self::Reg(dest)
        }
    }
}

impl From<Reg> for MaybeImm {
    fn from(value: Reg) -> Self {
        Self::Reg(value)
    }
}

impl Deref for LinearReg {
    type Target = Reg;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

fn var_index(var: &Variable<'_>) -> NonLocal {
    // SAFETY: it is repr(u16).
    unsafe { *<*const Variable>::from(var).cast::<NonLocal>() }
}

#[cfg(debug_assertions)]
impl Drop for LinearReg {
    fn drop(&mut self) {
        debug_assert!(false, "Leaked register {}!", self.0);
    }
}
