use elm_ast::builder::spanned;
use elm_ast::exposing::{ExposedItem, Exposing};
use elm_ast::expr::{CaseBranch, Expr};
use elm_ast::import::Import;
use elm_ast::node::Spanned;
use elm_ast::operator::InfixDirection;
use elm_ast::span::{Position, Span};
use elm_ast::type_annotation::{RecordField, TypeAnnotation};

fn line_span(line: u32) -> Span {
    let pos = Position {
        offset: 0,
        line,
        column: 1,
    };
    Span::new(pos, pos)
}

fn on_distinct_lines<T>(items: Vec<Spanned<T>>) -> Vec<Spanned<T>> {
    items
        .into_iter()
        .enumerate()
        .map(|(i, mut s)| {
            s.span = line_span((i + 1) as u32);
            s
        })
        .collect()
}

/// Qualified type reference, e.g. `Time.Posix`, `Decode.Decoder a`.
pub fn tqualified(
    module: &[&str],
    name: &str,
    args: Vec<Spanned<TypeAnnotation>>,
) -> Spanned<TypeAnnotation> {
    spanned(TypeAnnotation::Typed {
        module_name: module.iter().map(|s| s.to_string()).collect(),
        name: spanned(name.to_string()),
        args,
    })
}

/// Multi-line record type:
///
/// ```elm
/// { sqid : String
/// , name : String
/// }
/// ```
pub fn trecord(fields: Vec<(&str, Spanned<TypeAnnotation>)>) -> Spanned<TypeAnnotation> {
    let field_nodes: Vec<Spanned<RecordField>> = fields
        .into_iter()
        .map(|(name, type_annotation)| {
            spanned(RecordField {
                name: spanned(name.to_string()),
                type_annotation,
            })
        })
        .collect();
    spanned(TypeAnnotation::Record(on_distinct_lines(field_nodes)))
}

/// Left-associative pipeline `first |> s1 |> s2 |> ...` with each stage
/// on its own line.
pub fn pipeline_chain(first: Spanned<Expr>, steps: Vec<Spanned<Expr>>) -> Spanned<Expr> {
    let mut left = first;
    left.span = line_span(1);
    for (i, mut step) in steps.into_iter().enumerate() {
        step.span = line_span((i + 2) as u32);
        let op_span = Span::new(left.span.start, step.span.end);
        let op_expr = Expr::OperatorApplication {
            operator: "|>".to_string(),
            direction: InfixDirection::Left,
            left: Box::new(left),
            right: Box::new(step),
        };
        left = Spanned::new(op_span, op_expr);
    }
    left
}

/// List expression with each element on its own line.
pub fn list_multiline(elements: Vec<Spanned<Expr>>) -> Spanned<Expr> {
    spanned(Expr::List {
        elements: on_distinct_lines(elements),
        element_inline_comments: Vec::new(),
        trailing_comments: Vec::new(),
    })
}

/// Record access: `value.field`
pub fn record_access(record: Spanned<Expr>, field: &str) -> Spanned<Expr> {
    spanned(Expr::RecordAccess {
        record: Box::new(record),
        field: spanned(field.to_string()),
    })
}

/// Case expression. Useful for enum decoders.
pub fn case_of(
    expr: Spanned<Expr>,
    branches: Vec<(Spanned<elm_ast::pattern::Pattern>, Spanned<Expr>)>,
) -> Spanned<Expr> {
    spanned(Expr::CaseOf {
        expr: Box::new(expr),
        branches: branches
            .into_iter()
            .map(|(pattern, body)| CaseBranch { pattern, body })
            .collect(),
    })
}

/// `import Foo.Bar as B`
pub fn import_as(module_name: &[&str], alias: &str) -> Spanned<Import> {
    spanned(Import {
        module_name: spanned(module_name.iter().map(|s| s.to_string()).collect()),
        alias: Some(spanned(vec![alias.to_string()])),
        exposing: None,
    })
}

/// `import Foo.Bar as B exposing (Thing, otherThing)`
pub fn import_as_exposing(
    module_name: &[&str],
    alias: &str,
    exposed: Vec<&str>,
) -> Spanned<Import> {
    spanned(Import {
        module_name: spanned(module_name.iter().map(|s| s.to_string()).collect()),
        alias: Some(spanned(vec![alias.to_string()])),
        exposing: Some(spanned(Exposing::Explicit {
            items: exposed_items(exposed),
            trailing_comments: Vec::new(),
        })),
    })
}

/// `import Foo.Bar exposing (Thing, otherThing)`
pub fn import_exposing(module_name: &[&str], exposed: Vec<&str>) -> Spanned<Import> {
    spanned(Import {
        module_name: spanned(module_name.iter().map(|s| s.to_string()).collect()),
        alias: None,
        exposing: Some(spanned(Exposing::Explicit {
            items: exposed_items(exposed),
            trailing_comments: Vec::new(),
        })),
    })
}

fn exposed_items(exposed: Vec<&str>) -> Vec<Spanned<ExposedItem>> {
    exposed
        .into_iter()
        .map(|name| {
            let item = if name.chars().next().is_some_and(|c| c.is_uppercase()) {
                ExposedItem::TypeOrAlias(name.to_string())
            } else {
                ExposedItem::Function(name.to_string())
            };
            spanned(item)
        })
        .collect()
}
