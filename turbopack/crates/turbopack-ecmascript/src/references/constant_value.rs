use anyhow::Result;
use bincode::{Decode, Encode};
use swc_core::{
    common::{DUMMY_SP, FileName, SourceMap, sync::Lrc},
    ecma::{
        ast::{
            ArrayLit, EsVersion, Expr, KeyValueProp, Lit, ObjectLit, Prop, PropName, Regex, Str,
        },
        parser::{Syntax, parse_file_as_expr},
        visit::fields::PropField,
    },
    quote,
};
use turbo_rcstr::RcStr;
use turbo_tasks::{NonLocalValue, Vc, debug::ValueDebugFormat};
use turbo_tasks_fs::FileSystemPath;
use turbopack_core::{chunk::ChunkingContext, compile_time_info::CompileTimeDefineValue};

use crate::{
    ast_path_trie::{AstPathId, AstPathTrie},
    code_gen::{CodeGen, CodeGeneration},
    create_visitor,
    references::as_abs_path,
    runtime_functions::TURBOPACK_RESOLVE_ABSOLUTE_PATH,
};

#[derive(Clone, Debug, PartialEq, Eq, Hash, ValueDebugFormat, NonLocalValue, Encode, Decode)]
pub struct ConstantValueCodeGen {
    value: CompileTimeDefineValue,
    path: AstPathId,
}

impl ConstantValueCodeGen {
    pub fn new(value: CompileTimeDefineValue, path: AstPathId) -> Self {
        ConstantValueCodeGen { value, path }
    }
    pub async fn code_generation(
        &self,
        trie: &AstPathTrie,
        _chunking_context: Vc<Box<dyn ChunkingContext>>,
    ) -> Result<CodeGeneration> {
        Ok(expression_code_generation(
            trie,
            self.path,
            value_to_expr(&self.value),
        ))
    }
}

/// Resolve Node source globals relative to the runtime root instead of the virtual filesystem.
#[derive(Clone, Debug, PartialEq, Eq, Hash, ValueDebugFormat, NonLocalValue, Encode, Decode)]
pub struct InputRelativeCodeGen {
    source_path: FileSystemPath,
    path: AstPathId,
}

impl InputRelativeCodeGen {
    pub fn new(source_path: FileSystemPath, path: AstPathId) -> Self {
        Self { source_path, path }
    }

    pub async fn code_generation(
        &self,
        trie: &AstPathTrie,
        chunking_context: Vc<Box<dyn ChunkingContext>>,
    ) -> Result<CodeGeneration> {
        let root_path = chunking_context.root_path().await?;
        let value = match root_path.get_relative_path_to(&self.source_path) {
            Some(relative) => quote!(
                "$resolve($path)" as Expr,
                resolve: Expr = TURBOPACK_RESOLVE_ABSOLUTE_PATH.into(),
                path: Expr = relative.trim_start_matches("./").into()
            ),
            // Additional filesystems do not have a known runtime root.
            None => value_to_expr(&as_abs_path(self.source_path.clone()).into()),
        };
        Ok(expression_code_generation(trie, self.path, value))
    }
}

impl From<InputRelativeCodeGen> for CodeGen {
    fn from(value: InputRelativeCodeGen) -> Self {
        CodeGen::InputRelativeCodeGen(value)
    }
}

fn expression_code_generation(trie: &AstPathTrie, path: AstPathId, value: Expr) -> CodeGeneration {
    let mut visitors = Vec::new();

    if matches!(
        trie.get(path),
        Some(swc_core::ecma::visit::AstParentKind::Prop(
            PropField::Shorthand
        ))
    ) {
        let ast_path = trie.parent_or_root(path);
        visitors.push(create_visitor!(
            exact,
            trie,
            ast_path,
            visit_mut_prop,
            |prop: &mut Prop| {
                if let Prop::Shorthand(ident) = prop {
                    *prop = Prop::KeyValue(KeyValueProp {
                        key: PropName::Ident(ident.clone().into()),
                        value: value.clone().into(),
                    });
                }
            }
        ));
    } else {
        visitors.push(create_visitor!(
            trie,
            path,
            visit_mut_expr,
            |expr: &mut Expr| {
                *expr = value.clone();
            }
        ));
    }

    CodeGeneration::visitors(visitors)
}

impl From<ConstantValueCodeGen> for CodeGen {
    fn from(val: ConstantValueCodeGen) -> Self {
        CodeGen::ConstantValueCodeGen(val)
    }
}

fn value_to_expr(value: &CompileTimeDefineValue) -> Expr {
    match value {
        CompileTimeDefineValue::Undefined => {
            quote!("(\"TURBOPACK compile-time value\", void 0)" as Expr)
        }
        CompileTimeDefineValue::Null => {
            quote!("(\"TURBOPACK compile-time value\", null)" as Expr)
        }
        CompileTimeDefineValue::Bool(true) => {
            quote!("(\"TURBOPACK compile-time value\", true)" as Expr)
        }
        CompileTimeDefineValue::Bool(false) => {
            quote!("(\"TURBOPACK compile-time value\", false)" as Expr)
        }
        CompileTimeDefineValue::Number(n) => {
            quote!("(\"TURBOPACK compile-time value\", $e)" as Expr, e: Expr = n
                .as_f64()
                .expect("unreachable: serde-json has arbitrary_precision disabled")
                .into())
        }
        CompileTimeDefineValue::String(s) => {
            quote!("(\"TURBOPACK compile-time value\", $e)" as Expr, e: Expr = s.as_str().into())
        }
        CompileTimeDefineValue::BigInt(n) => {
            quote!("(\"TURBOPACK compile-time value\", $e)" as Expr, e: Expr = Expr::Lit(Lit::BigInt(n.as_ref().clone().into())))
        }
        CompileTimeDefineValue::Regex(pattern, flags) => {
            quote!("(\"TURBOPACK compile-time value\", $e)" as Expr, e: Expr = Expr::Lit(Lit::Regex(Regex {
               span: DUMMY_SP,
               exp: pattern.as_str().into(),
               flags: flags.as_str().into(),
            })))
        }
        CompileTimeDefineValue::Array(a) => {
            quote!("(\"TURBOPACK compile-time value\", $e)" as Expr, e: Expr = Expr::Array(ArrayLit {
                span: DUMMY_SP,
                elems: a.iter().map(|i| Some(value_to_expr(i).into())).collect(),
            }))
        }
        CompileTimeDefineValue::Object(m) => {
            quote!("(\"TURBOPACK compile-time value\", $e)" as Expr, e: Expr = Expr::Object(ObjectLit {
                span: DUMMY_SP,
                props: m
                    .iter()
                    .map(|(k, v)| {
                        swc_core::ecma::ast::PropOrSpread::Prop(
                            Prop::KeyValue(KeyValueProp {
                                key: PropName::Str(Str::from(k.as_str())),
                                value: value_to_expr(v).into(),
                            })
                            .into(),
                        )
                    })
                    .collect(),
            }))
        }
        CompileTimeDefineValue::Evaluate(s) => parse_single_expr_lit(s),
    }
}

pub(crate) fn parse_single_expr_lit(expr_lit: &RcStr) -> Expr {
    let cm = Lrc::new(SourceMap::default());
    let fm = cm.new_source_file(FileName::Anon.into(), expr_lit.clone());
    parse_file_as_expr(
        &fm,
        Syntax::Es(Default::default()),
        EsVersion::latest(),
        None,
        &mut vec![],
    )
    .map_or(
        quote!("(\"Failed parsed TURBOPACK compile-time value\", $s)" as Expr, s: Expr = expr_lit.as_str().into()),
        |expr| quote!("(\"TURBOPACK compile-time value\", $e)" as Expr, e: Expr = *expr),
    )
}
