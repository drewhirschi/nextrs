//! Island props extraction: parse a React `.tsx` component with oxc and
//! recover a serializable "props shape" for typed Rust bindings.
//!
//! This is the "alien import" half of the RSX design
//! (`docs/rsx-server-components.md`): TypeScript is the source of truth, and
//! the build generates `crate::client` bindings from each island's props
//! interface. oxc parses — it does not type-check — so island props must be
//! **explicitly annotated** with an interface/type-literal of serializable
//! shapes: primitives, string-literal unions, arrays, optionals, and nested
//! object literals. Anything else (generics, mapped types, `ReactNode`,
//! imports from other files) is a build error naming the file, prop, and
//! offending type. That strictness is deliberate: island props must survive
//! JSON serialization anyway.

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    Declaration, ExportDefaultDeclarationKind, Expression, Function, Program, PropertyKey,
    Statement, TSLiteral, TSSignature, TSType, TSTypeName,
};
use oxc_span::{GetSpan, SourceType};

/// A parsed island component: the default-exported React component and its
/// props shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IslandComponent {
    /// Component name (the default export's identifier, or the file stem for
    /// anonymous exports).
    pub name: String,
    /// Props in declaration order. Empty when the component takes no props.
    pub props: Vec<Prop>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prop {
    /// The TypeScript-side name (`initialFilter`).
    pub ts_name: String,
    pub optional: bool,
    pub ty: PropType,
}

/// The serializable-shape allowlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PropType {
    String,
    /// TS `number` — always `f64` on the Rust side (JSON has one number type).
    Number,
    Bool,
    /// A union of string literals: `'all' | 'active' | 'done'`.
    StringEnum(Vec<String>),
    Array(Box<PropType>),
    /// A nested object literal / in-file interface.
    Object(Vec<Prop>),
}

/// Parse a `.tsx` source. Returns:
/// - `Ok(Some(_))` — the file default-exports a component with supported props;
/// - `Ok(None)` — no default export (not an island root — plain module);
/// - `Err(msg)` — the file is an island but its props violate the allowlist,
///   or it doesn't parse. The message names file, prop, and offending type.
pub fn parse_island(source: &str, file_label: &str) -> Result<Option<IslandComponent>, String> {
    let allocator = Allocator::default();
    let parsed = oxc_parser::Parser::new(&allocator, source, SourceType::tsx()).parse();
    if !parsed.diagnostics.is_empty() {
        let first = &parsed.diagnostics[0];
        return Err(format!("{file_label}: failed to parse: {first}"));
    }
    let program = &parsed.program;

    let Some((func_name, params_first)) = find_default_export(program, file_label)? else {
        return Ok(None);
    };
    let name = func_name.unwrap_or_else(|| {
        std::path::Path::new(file_label)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Component")
            .to_string()
    });

    let Some(param) = params_first else {
        // No props parameter at all — a props-less island.
        return Ok(Some(IslandComponent { name, props: Vec::new() }));
    };

    let Some(annotation) = param else {
        return Err(format!(
            "{file_label}: island `{name}`'s props parameter has no type annotation — \
             island props must be explicitly annotated \
             (export default function {name}(props: {name}Props) {{ ... }})"
        ));
    };

    let props = props_of_type(annotation, program, source, file_label, &name, 0)?;
    Ok(Some(IslandComponent { name, props }))
}

/// Best-effort probe: the default-exported component's name, without
/// enforcing the props allowlist. Used to index candidate island files by
/// name — only components actually referenced from Rust get the full
/// (error-raising) `parse_island` treatment.
pub fn default_export_component_name(source: &str, file_label: &str) -> Option<String> {
    let allocator = Allocator::default();
    let parsed = oxc_parser::Parser::new(&allocator, source, SourceType::tsx()).parse();
    if !parsed.diagnostics.is_empty() {
        return None;
    }
    match find_default_export(&parsed.program, file_label) {
        Ok(Some((name, _))) => Some(name.unwrap_or_else(|| {
            std::path::Path::new(file_label)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Component")
                .to_string()
        })),
        _ => None,
    }
}

/// Locate the default-exported component. Returns
/// `(component_name, Some(first_param_annotation))` where the inner Option is
/// `None` when the function takes no parameters, and the annotation is `None`
/// when the first parameter is unannotated.
#[allow(clippy::type_complexity)]
fn find_default_export<'a>(
    program: &'a Program<'a>,
    file_label: &str,
) -> Result<Option<(Option<String>, Option<Option<&'a TSType<'a>>>)>, String> {
    for stmt in &program.body {
        let Statement::ExportDefaultDeclaration(export) = stmt else {
            continue;
        };
        return match &export.declaration {
            ExportDefaultDeclarationKind::FunctionDeclaration(f) => {
                Ok(Some(function_shape(f)))
            }
            ExportDefaultDeclarationKind::ArrowFunctionExpression(arrow) => {
                let first = arrow.params.items.first().map(|p| {
                    p.type_annotation.as_ref().map(|t| &t.type_annotation)
                });
                Ok(Some((None, first)))
            }
            ExportDefaultDeclarationKind::Identifier(ident) => {
                let target = ident.name.as_str();
                for stmt in &program.body {
                    if let Some(shape) = named_function(stmt, target) {
                        return Ok(Some(shape));
                    }
                }
                Err(format!(
                    "{file_label}: `export default {target}` — couldn't find \
                     `function {target}` or `const {target} = (...) => ...` in this file"
                ))
            }
            other => Err(format!(
                "{file_label}: unsupported default export ({}) — islands default-export \
                 a function component",
                kind_name(other)
            )),
        };
    }
    Ok(None)
}

fn function_shape<'a>(
    f: &'a Function<'a>,
) -> (Option<String>, Option<Option<&'a TSType<'a>>>) {
    let name = f.id.as_ref().map(|id| id.name.to_string());
    let first = f
        .params
        .items
        .first()
        .map(|p| p.type_annotation.as_ref().map(|t| &t.type_annotation));
    (name, first)
}

/// Match `function Name(...)`, `const Name = (...) => ...`, and their
/// `export`-prefixed forms, for `export default Name` resolution.
fn named_function<'a>(
    stmt: &'a Statement<'a>,
    target: &str,
) -> Option<(Option<String>, Option<Option<&'a TSType<'a>>>)> {
    let decl: &Declaration = match stmt {
        Statement::FunctionDeclaration(f) => {
            if f.id.as_ref().is_some_and(|id| id.name == target) {
                return Some(function_shape(f));
            }
            return None;
        }
        Statement::VariableDeclaration(v) => {
            for d in &v.declarations {
                let is_target = d
                    .id
                    .get_binding_identifier()
                    .is_some_and(|id| id.name == target);
                if !is_target {
                    continue;
                }
                if let Some(Expression::ArrowFunctionExpression(arrow)) = &d.init {
                    let first = arrow.params.items.first().map(|p| {
                        p.type_annotation.as_ref().map(|t| &t.type_annotation)
                    });
                    return Some((Some(target.to_string()), first));
                }
            }
            return None;
        }
        Statement::ExportNamedDeclaration(e) => e.declaration.as_ref()?,
        _ => return None,
    };
    match decl {
        Declaration::FunctionDeclaration(f)
            if f.id.as_ref().is_some_and(|id| id.name == target) =>
        {
            Some(function_shape(f))
        }
        _ => None,
    }
}

/// Resolve a props annotation to a flat prop list: either an inline type
/// literal or a named interface/type alias declared in the same file.
fn props_of_type(
    ty: &TSType<'_>,
    program: &Program<'_>,
    source: &str,
    file_label: &str,
    component: &str,
    depth: usize,
) -> Result<Vec<Prop>, String> {
    if depth > 16 {
        return Err(format!(
            "{file_label}: island `{component}`: props type nests too deep (cycle?)"
        ));
    }
    match ty {
        TSType::TSTypeLiteral(lit) => {
            signatures_to_props(&lit.members, program, source, file_label, component, depth)
        }
        TSType::TSTypeReference(r) => {
            let TSTypeName::IdentifierReference(ident) = &r.type_name else {
                return Err(unsupported(source, ty.span(), file_label, component, ""));
            };
            let name = ident.name.as_str();
            let members = find_type_decl(program, name).ok_or_else(|| {
                format!(
                    "{file_label}: island `{component}`: props type `{name}` isn't declared \
                     in this file — cross-file and imported props types aren't supported yet; \
                     declare the interface next to the component"
                )
            })?;
            match members {
                TypeDecl::Interface(sigs) => {
                    signatures_to_props(sigs, program, source, file_label, component, depth + 1)
                }
                TypeDecl::Alias(aliased) => {
                    props_of_type(aliased, program, source, file_label, component, depth + 1)
                }
            }
        }
        _ => Err(unsupported(source, ty.span(), file_label, component, "props annotation")),
    }
}

enum TypeDecl<'a, 'b> {
    Interface(&'b oxc_allocator::Vec<'a, TSSignature<'a>>),
    Alias(&'b TSType<'a>),
}

fn find_type_decl<'a, 'b>(program: &'b Program<'a>, name: &str) -> Option<TypeDecl<'a, 'b>> {
    for stmt in &program.body {
        let decl: Option<&Declaration> = match stmt {
            Statement::TSInterfaceDeclaration(_) | Statement::TSTypeAliasDeclaration(_) => {
                stmt.as_declaration()
            }
            Statement::ExportNamedDeclaration(e) => e.declaration.as_ref(),
            _ => None,
        };
        match decl {
            Some(Declaration::TSInterfaceDeclaration(i)) if i.id.name == name => {
                return Some(TypeDecl::Interface(&i.body.body));
            }
            Some(Declaration::TSTypeAliasDeclaration(a)) if a.id.name == name => {
                return Some(TypeDecl::Alias(&a.type_annotation));
            }
            _ => {}
        }
    }
    None
}

fn signatures_to_props(
    signatures: &oxc_allocator::Vec<'_, TSSignature<'_>>,
    program: &Program<'_>,
    source: &str,
    file_label: &str,
    component: &str,
    depth: usize,
) -> Result<Vec<Prop>, String> {
    let mut props = Vec::new();
    for sig in signatures {
        let TSSignature::TSPropertySignature(p) = sig else {
            return Err(format!(
                "{file_label}: island `{component}`: only plain `name: Type` properties are \
                 supported in props (no methods, index signatures, or call signatures)"
            ));
        };
        let PropertyKey::StaticIdentifier(key) = &p.key else {
            return Err(format!(
                "{file_label}: island `{component}`: computed/quoted prop names aren't supported"
            ));
        };
        let ts_name = key.name.to_string();
        let Some(ann) = &p.type_annotation else {
            return Err(format!(
                "{file_label}: island `{component}`: prop `{ts_name}` has no type"
            ));
        };
        let ty = map_type(
            &ann.type_annotation,
            program,
            source,
            file_label,
            component,
            &ts_name,
            depth,
        )?;
        props.push(Prop { ts_name, optional: p.optional, ty });
    }
    Ok(props)
}

#[allow(clippy::too_many_arguments)]
fn map_type(
    ty: &TSType<'_>,
    program: &Program<'_>,
    source: &str,
    file_label: &str,
    component: &str,
    prop: &str,
    depth: usize,
) -> Result<PropType, String> {
    if depth > 16 {
        return Err(format!(
            "{file_label}: island `{component}`: prop `{prop}` nests too deep (cycle?)"
        ));
    }
    match ty {
        TSType::TSStringKeyword(_) => Ok(PropType::String),
        TSType::TSNumberKeyword(_) => Ok(PropType::Number),
        TSType::TSBooleanKeyword(_) => Ok(PropType::Bool),
        TSType::TSLiteralType(l) => match &l.literal {
            TSLiteral::StringLiteral(s) => Ok(PropType::StringEnum(vec![s.value.to_string()])),
            _ => Err(unsupported(source, ty.span(), file_label, component, prop)),
        },
        TSType::TSUnionType(u) => {
            let mut variants = Vec::new();
            for member in &u.types {
                match member {
                    TSType::TSLiteralType(l) => match &l.literal {
                        TSLiteral::StringLiteral(s) => variants.push(s.value.to_string()),
                        _ => {
                            return Err(unsupported(
                                source, member.span(), file_label, component, prop,
                            ));
                        }
                    },
                    _ => {
                        return Err(format!(
                            "{}: island `{component}`: prop `{prop}`: only unions of string \
                             literals are supported ('a' | 'b'), found `{}`",
                            file_label,
                            span_text(source, member.span()),
                        ));
                    }
                }
            }
            Ok(PropType::StringEnum(variants))
        }
        TSType::TSArrayType(a) => Ok(PropType::Array(Box::new(map_type(
            &a.element_type,
            program,
            source,
            file_label,
            component,
            prop,
            depth + 1,
        )?))),
        TSType::TSTypeLiteral(lit) => Ok(PropType::Object(signatures_to_props(
            &lit.members,
            program,
            source,
            file_label,
            component,
            depth + 1,
        )?)),
        TSType::TSTypeReference(r) => {
            let TSTypeName::IdentifierReference(ident) = &r.type_name else {
                return Err(unsupported(source, ty.span(), file_label, component, prop));
            };
            let name = ident.name.as_str();
            match find_type_decl(program, name) {
                Some(TypeDecl::Interface(sigs)) => Ok(PropType::Object(signatures_to_props(
                    sigs, program, source, file_label, component, depth + 1,
                )?)),
                Some(TypeDecl::Alias(aliased)) => map_type(
                    aliased, program, source, file_label, component, prop, depth + 1,
                ),
                None => Err(format!(
                    "{file_label}: island `{component}`: prop `{prop}` references `{name}`, \
                     which isn't declared in this file — cross-file and imported types aren't \
                     supported yet (nor library types like ReactNode); island props must be \
                     serializable shapes declared next to the component"
                )),
            }
        }
        _ => Err(unsupported(source, ty.span(), file_label, component, prop)),
    }
}

fn span_text(source: &str, span: oxc_span::Span) -> String {
    source
        .get(span.start as usize..span.end as usize)
        .unwrap_or("<?>")
        .trim()
        .to_string()
}

fn unsupported(
    source: &str,
    span: oxc_span::Span,
    file_label: &str,
    component: &str,
    prop: &str,
) -> String {
    let at = if prop.is_empty() { String::new() } else { format!(" prop `{prop}`:") };
    format!(
        "{file_label}: island `{component}`:{at} unsupported props type `{}` — supported shapes: \
         string, number, boolean, string-literal unions, arrays, optionals, and nested object \
         literals (the props must survive JSON serialization)",
        span_text(source, span),
    )
}

fn kind_name(kind: &ExportDefaultDeclarationKind<'_>) -> &'static str {
    match kind {
        ExportDefaultDeclarationKind::ClassDeclaration(_) => "a class",
        ExportDefaultDeclarationKind::TSInterfaceDeclaration(_) => "an interface",
        _ => "a non-function expression",
    }
}

// ---------------------------------------------------------------------------
// Rust bindings rendering
// ---------------------------------------------------------------------------

/// Convert `initialFilter` → `initial_filter` (and guard Rust keywords).
pub fn rust_field_name(ts_name: &str) -> String {
    let mut out = String::with_capacity(ts_name.len() + 4);
    for (i, ch) in ts_name.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    const KEYWORDS: &[&str] = &[
        "as", "async", "await", "box", "break", "const", "continue", "crate", "dyn", "else",
        "enum", "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod",
        "move", "mut", "pub", "ref", "return", "self", "static", "struct", "super", "trait",
        "true", "type", "unsafe", "use", "where", "while",
    ];
    if KEYWORDS.contains(&out.as_str()) {
        format!("r#{out}")
    } else {
        out
    }
}

/// `'not-found'` → `NotFound`; `'active'` → `Active`; leading digits get a
/// `V` prefix so the variant is a valid ident.
pub fn variant_name(literal: &str) -> String {
    let mut out = String::new();
    let mut upper_next = true;
    for ch in literal.chars() {
        if ch.is_ascii_alphanumeric() {
            if upper_next {
                out.push(ch.to_ascii_uppercase());
                upper_next = false;
            } else {
                out.push(ch);
            }
        } else {
            upper_next = true;
        }
    }
    if out.is_empty() || out.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        format!("V{out}")
    } else {
        out
    }
}

fn pascal(ts_name: &str) -> String {
    variant_name(ts_name)
}

/// Render the `crate::client` bindings module for a set of islands.
/// `islands` pairs each parsed component with its manifest id
/// (`TodoFilter-a91f3c`). The output is written to
/// `$OUT_DIR/nextrs_islands.rs` by `bundle_pages` and included by the app.
pub fn render_bindings(islands: &[(String, IslandComponent)]) -> String {
    // No inner `#![allow]` — this file is include!d inside the app's
    // `pub mod client { ... }`, where inner attributes are rejected; the
    // component fns carry their own outer allows instead.
    let mut out = String::from(
        "// @generated by nextrs from app .tsx islands — do not edit.\n\
         // Typed Rust bindings for React island components \
         (docs/rsx-server-components.md).\n\n",
    );
    for (island_id, component) in islands {
        render_component(&mut out, island_id, component);
    }
    out
}

fn render_component(out: &mut String, island_id: &str, component: &IslandComponent) {
    let name = &component.name;
    if component.props.is_empty() {
        out.push_str(&format!(
            "/// React island `{name}` (chunk `{island_id}`).\n\
             #[allow(non_snake_case)]\n\
             pub fn {name}() -> ::nextrs::rsx::Rsx {{\n    \
                 ::nextrs::rsx::Rsx::client_ref(\"{island_id}\", &::nextrs::serde_json::json!({{}}))\n\
             }}\n\n"
        ));
        return;
    }

    let mut aux = String::new();
    let props_ty = format!("{name}Props");
    out.push_str(&format!(
        "/// Props for the React island `{name}` — generated from its TypeScript \
         props annotation.\n\
         #[derive(Debug, Clone, ::nextrs::serde::Serialize)]\n\
         #[serde(crate = \"::nextrs::serde\")]\n\
         pub struct {props_ty} {{\n"
    ));
    for prop in &component.props {
        render_field(out, &mut aux, name, prop);
    }
    out.push_str("}\n\n");
    out.push_str(&aux);
    out.push_str(&format!(
        "/// React island `{name}` (chunk `{island_id}`). Renders a placeholder \
         that the client runtime hydrates.\n\
         #[allow(non_snake_case)]\n\
         pub fn {name}(props: {props_ty}) -> ::nextrs::rsx::Rsx {{\n    \
             ::nextrs::rsx::Rsx::client_ref(\"{island_id}\", &props)\n\
         }}\n\n"
    ));
}

/// Emit one struct field; nested enums/structs accumulate into `aux`.
fn render_field(out: &mut String, aux: &mut String, component: &str, prop: &Prop) {
    let field = rust_field_name(&prop.ts_name);
    let base_ty = rust_type(aux, component, &prop.ts_name, &prop.ty);
    let (ty, skip) = if prop.optional {
        (format!("Option<{base_ty}>"), true)
    } else {
        (base_ty, false)
    };
    let ts_name = &prop.ts_name;
    out.push_str(&format!("    #[serde(rename = \"{ts_name}\""));
    if skip {
        out.push_str(", skip_serializing_if = \"Option::is_none\"");
    }
    out.push_str(")]\n");
    out.push_str(&format!("    pub {field}: {ty},\n"));
}

fn rust_type(aux: &mut String, component: &str, ts_name: &str, ty: &PropType) -> String {
    match ty {
        PropType::String => "String".into(),
        PropType::Number => "f64".into(),
        PropType::Bool => "bool".into(),
        PropType::Array(inner) => {
            format!("Vec<{}>", rust_type(aux, component, ts_name, inner))
        }
        PropType::StringEnum(variants) => {
            let enum_name = format!("{component}{}", pascal(ts_name));
            aux.push_str(&format!(
                "/// `{}` — generated from a TypeScript string-literal union.\n\
                 #[derive(Debug, Clone, Copy, PartialEq, Eq, ::nextrs::serde::Serialize)]\n\
                 #[serde(crate = \"::nextrs::serde\")]\n\
                 pub enum {enum_name} {{\n",
                variants.join(" | "),
            ));
            for v in variants {
                aux.push_str(&format!(
                    "    #[serde(rename = \"{v}\")]\n    {},\n",
                    variant_name(v)
                ));
            }
            aux.push_str("}\n\n");
            enum_name
        }
        PropType::Object(props) => {
            let struct_name = format!("{component}{}", pascal(ts_name));
            let mut body = format!(
                "/// Nested props object `{ts_name}` of island `{component}`.\n\
                 #[derive(Debug, Clone, ::nextrs::serde::Serialize)]\n\
                 #[serde(crate = \"::nextrs::serde\")]\n\
                 pub struct {struct_name} {{\n"
            );
            let mut nested_aux = String::new();
            for p in props {
                render_field(&mut body, &mut nested_aux, &struct_name, p);
            }
            body.push_str("}\n\n");
            aux.push_str(&body);
            aux.push_str(&nested_aux);
            struct_name
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TODO_FILTER: &str = r#"
import { useState } from 'react';

export interface TodoFilterProps {
  initialFilter: 'all' | 'active' | 'done';
  counts: { all: number; active: number; done: number };
  label?: string;
  tags: string[];
}

export default function TodoFilter({ initialFilter, counts }: TodoFilterProps) {
  const [filter, setFilter] = useState(initialFilter);
  return <div>{filter}</div>;
}
"#;

    #[test]
    fn extracts_props_shape() {
        let island = parse_island(TODO_FILTER, "app/components/TodoFilter.tsx")
            .unwrap()
            .expect("is an island");
        assert_eq!(island.name, "TodoFilter");
        assert_eq!(island.props.len(), 4);
        assert_eq!(island.props[0].ts_name, "initialFilter");
        assert_eq!(
            island.props[0].ty,
            PropType::StringEnum(vec!["all".into(), "active".into(), "done".into()])
        );
        assert!(matches!(&island.props[1].ty, PropType::Object(p) if p.len() == 3));
        assert!(island.props[2].optional);
        assert_eq!(island.props[3].ty, PropType::Array(Box::new(PropType::String)));
    }

    #[test]
    fn no_default_export_is_not_an_island() {
        let src = "export function helper() { return 1; }";
        assert_eq!(parse_island(src, "app/lib.ts").unwrap(), None);
    }

    #[test]
    fn unannotated_props_is_an_error() {
        let src = "export default function X(props) { return <div/>; }";
        let err = parse_island(src, "app/X.tsx").unwrap_err();
        assert!(err.contains("no type annotation"), "{err}");
    }

    #[test]
    fn imported_type_is_a_clear_error() {
        let src = r#"
import type { Props } from './types';
export default function X(props: Props) { return <div/>; }
"#;
        let err = parse_island(src, "app/X.tsx").unwrap_err();
        assert!(err.contains("isn't declared in this file"), "{err}");
    }

    #[test]
    fn unsupported_type_names_the_prop() {
        let src = r#"
interface P { children: React.ReactNode; }
export default function X(props: P) { return <div/>; }
"#;
        let err = parse_island(src, "app/X.tsx").unwrap_err();
        assert!(err.contains('X') && err.contains("children"), "{err}");
    }

    #[test]
    fn default_export_identifier_and_arrow() {
        let src = r#"
type P = { n: number };
const Counter = (props: P) => <div>{props.n}</div>;
export default Counter;
"#;
        let island = parse_island(src, "app/Counter.tsx").unwrap().unwrap();
        assert_eq!(island.name, "Counter");
        assert_eq!(island.props, vec![Prop { ts_name: "n".into(), optional: false, ty: PropType::Number }]);
    }

    #[test]
    fn propless_component() {
        let src = "export default function Logo() { return <svg/>; }";
        let island = parse_island(src, "app/Logo.tsx").unwrap().unwrap();
        assert_eq!(island.name, "Logo");
        assert!(island.props.is_empty());
    }

    #[test]
    fn bindings_render() {
        let island = parse_island(TODO_FILTER, "app/components/TodoFilter.tsx")
            .unwrap()
            .unwrap();
        let code = render_bindings(&[("TodoFilter-abc123".into(), island)]);
        assert!(code.contains("pub struct TodoFilterProps"), "{code}");
        assert!(code.contains("pub enum TodoFilterInitialFilter"), "{code}");
        assert!(code.contains("#[serde(rename = \"initialFilter\")]"), "{code}");
        assert!(code.contains("pub initial_filter: TodoFilterInitialFilter"), "{code}");
        assert!(code.contains("pub struct TodoFilterCounts"), "{code}");
        assert!(code.contains("pub counts: TodoFilterCounts"), "{code}");
        assert!(
            code.contains("skip_serializing_if = \"Option::is_none\""),
            "{code}"
        );
        assert!(code.contains("pub tags: Vec<String>"), "{code}");
        assert!(code.contains("Rsx::client_ref(\"TodoFilter-abc123\", &props)"), "{code}");
    }

    #[test]
    fn name_conversions() {
        assert_eq!(rust_field_name("initialFilter"), "initial_filter");
        assert_eq!(rust_field_name("type"), "r#type");
        assert_eq!(variant_name("not-found"), "NotFound");
        assert_eq!(variant_name("404"), "V404");
    }
}
