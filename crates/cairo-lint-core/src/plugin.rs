use cairo_lang_defs::ids::{FunctionWithBodyId, ModuleId, ModuleItemId};
use cairo_lang_defs::plugin::PluginDiagnostic;
use cairo_lang_semantic::db::SemanticGroup;
use cairo_lang_semantic::plugin::{AnalyzerPlugin, PluginSuite};
use cairo_lang_semantic::Expr;
use cairo_lang_syntax::node::ast::{ElseClause, Expr as AstExpr, ExprBinary, ExprIf};
use cairo_lang_syntax::node::kind::SyntaxKind;
use cairo_lang_syntax::node::{TypedStablePtr, TypedSyntaxNode};

use crate::lints::ifs::*;
use crate::lints::{
    bool_comparison, breaks, double_comparison, double_parens, duplicate_underscore_args, loops, single_match,
};

pub fn cairo_lint_plugin_suite() -> PluginSuite {
    let mut suite = PluginSuite::default();
    suite.add_analyzer_plugin::<CairoLint>();
    suite
}
#[derive(Debug, Default)]
pub struct CairoLint;

#[derive(Debug, PartialEq)]
pub enum CairoLintKind {
    DestructMatch,
    MatchForEquality,
    DoubleComparison,
    DoubleParens,
    EquatableIfLet,
    BreakUnit,
    BoolComparison,
    CollapsibleIfElse,
    DuplicateUnderscoreArgs,
    LoopMatchPopFront,
    Unknown,
}

pub fn diagnostic_kind_from_message(message: &str) -> CairoLintKind {
    match message {
        single_match::DESTRUCT_MATCH => CairoLintKind::DestructMatch,
        single_match::MATCH_FOR_EQUALITY => CairoLintKind::MatchForEquality,
        double_parens::DOUBLE_PARENS => CairoLintKind::DoubleParens,
        double_comparison::SIMPLIFIABLE_COMPARISON => CairoLintKind::DoubleComparison,
        double_comparison::REDUNDANT_COMPARISON => CairoLintKind::DoubleComparison,
        double_comparison::CONTRADICTORY_COMPARISON => CairoLintKind::DoubleComparison,
        breaks::BREAK_UNIT => CairoLintKind::BreakUnit,
        equatable_if_let::EQUATABLE_IF_LET => CairoLintKind::EquatableIfLet,
        bool_comparison::BOOL_COMPARISON => CairoLintKind::BoolComparison,
        collapsible_if_else::COLLAPSIBLE_IF_ELSE => CairoLintKind::CollapsibleIfElse,
        duplicate_underscore_args::DUPLICATE_UNDERSCORE_ARGS => CairoLintKind::DuplicateUnderscoreArgs,
        loops::LOOP_MATCH_POP_FRONT => CairoLintKind::LoopMatchPopFront,
        _ => CairoLintKind::Unknown,
    }
}

impl AnalyzerPlugin for CairoLint {
    fn diagnostics(&self, db: &dyn SemanticGroup, module_id: ModuleId) -> Vec<PluginDiagnostic> {
        let mut diags = Vec::new();
        let syntax_db = db.upcast();
        let Ok(items) = db.module_items(module_id) else {
            return diags;
        };
        for item in &*items {
            let function_nodes = match item {
                ModuleItemId::Constant(constant_id) => {
                    constant_id.stable_ptr(db.upcast()).lookup(syntax_db).as_syntax_node()
                }
                ModuleItemId::FreeFunction(free_function_id) => {
                    let func_id = FunctionWithBodyId::Free(*free_function_id);
                    duplicate_underscore_args::check_duplicate_underscore_args(
                        db.function_with_body_signature(func_id).unwrap().params,
                        &mut diags,
                    );
                    let Ok(function_body) = db.function_body(func_id) else {
                        continue;
                    };
                    for (_expression_id, expression) in &function_body.arenas.exprs {
                        match &expression {
                            Expr::Match(expr_match) => {
                                single_match::check_single_match(db, expr_match, &mut diags, &function_body.arenas)
                            }
                            Expr::Loop(expr_loop) => {
                                loops::check_loop_match_pop_front(db, expr_loop, &mut diags, &function_body.arenas)
                            }
                            _ => (),
                        };
                    }
                    free_function_id.stable_ptr(db.upcast()).lookup(syntax_db).as_syntax_node()
                }
                ModuleItemId::Impl(impl_id) => {
                    let impl_functions = db.impl_functions(*impl_id);
                    let Ok(functions) = impl_functions else {
                        continue;
                    };
                    for (_fn_name, fn_id) in functions.iter() {
                        let Ok(function_body) = db.function_body(FunctionWithBodyId::Impl(*fn_id)) else {
                            continue;
                        };
                        for (_expression_id, expression) in &function_body.arenas.exprs {
                            match &expression {
                                Expr::Match(expr_match) => {
                                    single_match::check_single_match(db, expr_match, &mut diags, &function_body.arenas)
                                }
                                Expr::Loop(expr_loop) => {
                                    loops::check_loop_match_pop_front(db, expr_loop, &mut diags, &function_body.arenas)
                                }
                                _ => (),
                            };
                        }
                    }
                    impl_id.stable_ptr(db.upcast()).lookup(syntax_db).as_syntax_node()
                }
                _ => continue,
            }
            .descendants(syntax_db);

            for node in function_nodes {
                match node.kind(syntax_db) {
                    SyntaxKind::ExprParenthesized => double_parens::check_double_parens(
                        db.upcast(),
                        &AstExpr::from_syntax_node(db.upcast(), node),
                        &mut diags,
                    ),
                    SyntaxKind::StatementBreak => breaks::check_break(db.upcast(), node, &mut diags),
                    SyntaxKind::ExprIf => equatable_if_let::check_equatable_if_let(
                        db.upcast(),
                        &ExprIf::from_syntax_node(db.upcast(), node),
                        &mut diags,
                    ),
                    SyntaxKind::ExprBinary => {
                        let expr_binary = ExprBinary::from_syntax_node(db.upcast(), node);
                        bool_comparison::check_bool_comparison(db.upcast(), &expr_binary, &mut diags);
                        double_comparison::check_double_comparison(db.upcast(), &expr_binary, &mut diags);
                    }
                    SyntaxKind::ElseClause => {
                        collapsible_if_else::check_collapsible_if_else(
                            db.upcast(),
                            &ElseClause::from_syntax_node(db.upcast(), node),
                            &mut diags,
                        );
                    }
                    _ => continue,
                }
            }
        }
        diags
    }
}
