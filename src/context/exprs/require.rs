use crate::bin_op::BinOp;
use crate::AnalyzerLike;
use crate::Concrete;
use crate::ConcreteNode;
use ethers_core::types::I256;

use crate::{
    ContextBuilder, ContextNode, ContextVar, ContextVarNode, Node, Op, Range, TmpConstruction,
};
use solang_parser::pt::{Expression, Loc};

impl<T> Require for T where T: BinOp + Sized + AnalyzerLike {}
pub trait Require: AnalyzerLike + BinOp + Sized {
    /// Updates the range bounds for the variables passed into the require function. If the lefthand side is a temporary value,
    /// it will recursively update the range bounds for the underlying variable
    fn require(
        &mut self,
        (lhs_cvar, new_lhs): (ContextVarNode, ContextVarNode),
        (rhs_cvar, new_rhs): (ContextVarNode, ContextVarNode),
        ctx: ContextNode,
        loc: Loc,
        lhs_range_fn: fn(Range, Range) -> Range,
        inverse_lhs_range_fn: fn(Range, Range) -> Range,
        rhs_range_fn: fn(Range, Range) -> Range,
    ) {
        if let Some(lhs_range) = new_lhs.underlying(self).ty.range(self) {
            if let Some(rhs_range) = new_rhs.underlying(self).ty.range(self) {
                let new_lhs_range = lhs_range_fn(lhs_range.clone(), rhs_range.clone());
                new_lhs.set_range_min(self, new_lhs_range.min);
                new_lhs.set_range_max(self, new_lhs_range.max);

                let new_rhs_range = rhs_range_fn(rhs_range, lhs_range.clone());
                new_rhs.set_range_min(self, new_rhs_range.min);
                new_rhs.set_range_max(self, new_rhs_range.max);
            }
        }

        if let Some(tmp) = lhs_cvar.tmp_of(self) {
            self.range_recursion(tmp, lhs_range_fn, inverse_lhs_range_fn, rhs_cvar, ctx, loc)
        }
    }

    /// Handles a require expression
    fn handle_require(&mut self, inputs: &Vec<Expression>, ctx: ContextNode) {
        match inputs.get(0).expect("No lhs input for require statement") {
            Expression::Less(loc, lhs, rhs) => {
                let lhs_cvar = ContextVarNode::from(self.parse_ctx_expr(lhs, ctx)[0]);
                let rhs_cvar = ContextVarNode::from(self.parse_ctx_expr(rhs, ctx)[0]);

                let new_lhs = self.advance_var(lhs_cvar, *loc);
                let new_rhs = self.advance_var(rhs_cvar, *loc);

                self.require(
                    (lhs_cvar, new_lhs),
                    (rhs_cvar, new_rhs),
                    ctx,
                    *loc,
                    Range::lt,
                    Range::gte,
                    Range::gt,
                );
            }
            Expression::More(loc, lhs, rhs) => {
                let lhs_cvar = ContextVarNode::from(self.parse_ctx_expr(lhs, ctx)[0]);
                let rhs_cvar = ContextVarNode::from(self.parse_ctx_expr(rhs, ctx)[0]);

                let new_lhs = self.advance_var(lhs_cvar, *loc);
                let new_rhs = self.advance_var(rhs_cvar, *loc);

                self.require(
                    (lhs_cvar, new_lhs),
                    (rhs_cvar, new_rhs),
                    ctx,
                    *loc,
                    Range::gt,
                    Range::lte,
                    Range::lt,
                );
            }
            Expression::MoreEqual(loc, lhs, rhs) => {
                let lhs_cvar = ContextVarNode::from(self.parse_ctx_expr(lhs, ctx)[0]);
                let rhs_cvar = ContextVarNode::from(self.parse_ctx_expr(rhs, ctx)[0]);

                let new_lhs = self.advance_var(lhs_cvar, *loc);
                let new_rhs = self.advance_var(rhs_cvar, *loc);

                self.require(
                    (lhs_cvar, new_lhs),
                    (rhs_cvar, new_rhs),
                    ctx,
                    *loc,
                    Range::gte,
                    Range::lte,
                    Range::lte,
                );
            }
            Expression::LessEqual(loc, lhs, rhs) => {
                let lhs_cvar = ContextVarNode::from(self.parse_ctx_expr(lhs, ctx)[0]);
                let rhs_cvar = ContextVarNode::from(self.parse_ctx_expr(rhs, ctx)[0]);

                let new_lhs = self.advance_var(lhs_cvar, *loc);
                let new_rhs = self.advance_var(rhs_cvar, *loc);

                self.require(
                    (lhs_cvar, new_lhs),
                    (rhs_cvar, new_rhs),
                    ctx,
                    *loc,
                    Range::lte,
                    Range::gte,
                    Range::gte,
                );
            }
            _ => unreachable!("Require expr with noncomparator"),
        }
    }

    /// Recursively updates the range for a
    fn range_recursion(
        &mut self,
        tmp_construction: TmpConstruction,
        lhs_range_fn: fn(Range, Range) -> Range,
        inverse_lhs_range_fn: fn(Range, Range) -> Range,
        rhs_cvar: ContextVarNode,
        ctx: ContextNode,
        loc: Loc,
    ) {
        // handle lhs
        if !tmp_construction.lhs.is_const(self) {
            let adjusted_gt_rhs = ContextVarNode::from(
                self.op(
                    loc,
                    rhs_cvar,
                    tmp_construction.rhs,
                    ctx,
                    tmp_construction.op.inverse(),
                    false,
                )[0],
            );
            let new_underlying_lhs = self.advance_var(tmp_construction.lhs, loc);
            if let Some(lhs_range) = new_underlying_lhs.underlying(self).ty.range(self) {
                if let Some(rhs_range) = adjusted_gt_rhs.underlying(self).ty.range(self) {
                    let new_lhs_range = lhs_range_fn(lhs_range, rhs_range.clone());
                    new_underlying_lhs.set_range_min(self, new_lhs_range.min);
                    new_underlying_lhs.set_range_max(self, new_lhs_range.max);

                    if let Some(tmp) = new_underlying_lhs.tmp_of(self) {
                        self.range_recursion(
                            tmp,
                            lhs_range_fn,
                            inverse_lhs_range_fn,
                            adjusted_gt_rhs,
                            ctx,
                            loc,
                        );
                    }
                }
            }
        }

        // handle rhs
        if !tmp_construction.rhs.is_const(self) {
            let (needs_inverse, adjusted_gt_rhs) = match tmp_construction.op {
                Op::Sub => {
                    let concrete = ConcreteNode(
                        self.add_node(Node::Concrete(Concrete::Int(256, I256::from(-1i32))))
                            .index(),
                    );
                    let lhs_cvar = ContextVar::new_from_concrete(loc, concrete, self);
                    let tmp_lhs = ContextVarNode::from(self.add_node(Node::ContextVar(lhs_cvar)));
                    let tmp_rhs = ContextVarNode::from(
                        self.op(loc, rhs_cvar, tmp_lhs, ctx, Op::Mul, false)[0],
                    );
                    let new_rhs = ContextVarNode::from(
                        self.op(
                            loc,
                            tmp_rhs,
                            tmp_construction.lhs,
                            ctx,
                            tmp_construction.op.inverse(),
                            false,
                        )[0],
                    );
                    (true, new_rhs)
                }
                Op::Add => {
                    let new_rhs = ContextVarNode::from(
                        self.op(
                            loc,
                            rhs_cvar,
                            tmp_construction.lhs,
                            ctx,
                            tmp_construction.op.inverse(),
                            false,
                        )[0],
                    );
                    (false, new_rhs)
                }
                e => panic!("here {:?}", e),
            };

            let new_underlying_rhs = self.advance_var(tmp_construction.rhs, loc);
            if let Some(lhs_range) = new_underlying_rhs.underlying(self).ty.range(self) {
                if let Some(rhs_range) = adjusted_gt_rhs.underlying(self).ty.range(self) {
                    let new_lhs_range = if needs_inverse {
                        inverse_lhs_range_fn(lhs_range.clone(), rhs_range.clone())
                    } else {
                        lhs_range_fn(lhs_range.clone(), rhs_range.clone())
                    };

                    new_underlying_rhs.set_range_min(self, new_lhs_range.min);
                    new_underlying_rhs.set_range_max(self, new_lhs_range.max);
                    if let Some(tmp) = new_underlying_rhs.tmp_of(self) {
                        self.range_recursion(
                            tmp,
                            lhs_range_fn,
                            inverse_lhs_range_fn,
                            adjusted_gt_rhs,
                            ctx,
                            loc,
                        );
                    }
                }
            }
        }
    }
}
