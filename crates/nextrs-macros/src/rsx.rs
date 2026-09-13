//! The `rsx!` macro: JSX-shaped templates for Rust server components.
//!
//! Parses with [`rstml`] (the parser under Leptos's `view!`) and expands to a
//! single `String` builder that returns `::nextrs::rsx::Rsx`. Static text and
//! attributes are escaped at compile time; expression holes go through the
//! `Render`/`AttrValue` traits at runtime.
//!
//! Element / component split follows JSX: lowercase tags are HTML elements,
//! uppercase-first names are components — plain Rust functions returning
//! `Rsx`, called as `Name(NameProps { ... })` (or `Name()` with no
//! attributes). Generated island bindings (`crate::client::*`) have exactly
//! that shape. Components take no children in v1: islands are leaves
//! (docs/rsx-server-components.md).

use proc_macro2::TokenStream;
use quote::{ToTokens, quote};
use rstml::node::{
    KVAttributeValue, KeyedAttribute, Node, NodeAttribute, NodeBlock, NodeElement, NodeName,
};
use syn::spanned::Spanned;

/// HTML void elements — no closing tag, self-closing in source is idiomatic.
const VOID_ELEMENTS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track",
    "wbr",
];

pub(crate) fn rsx_impl(input: TokenStream) -> TokenStream {
    let nodes = match rstml::parse2(input) {
        Ok(nodes) => nodes,
        Err(e) => return e.to_compile_error(),
    };

    let mut cx = Codegen::default();
    for node in &nodes {
        cx.node(node);
    }
    cx.flush();

    if let Some(err) = cx.errors.into_iter().reduce(|mut a, b| {
        a.combine(b);
        a
    }) {
        return err.to_compile_error();
    }

    let stmts = cx.stmts;
    quote! {{
        let mut __nx_out = ::std::string::String::new();
        #(#stmts)*
        ::nextrs::rsx::Rsx::from_raw(__nx_out)
    }}
}

#[derive(Default)]
struct Codegen {
    stmts: Vec<TokenStream>,
    /// Pending compile-time-known output, coalesced into one `push_str`.
    buf: String,
    errors: Vec<syn::Error>,
}

impl Codegen {
    fn flush(&mut self) {
        if !self.buf.is_empty() {
            let lit = std::mem::take(&mut self.buf);
            self.stmts.push(quote! { __nx_out.push_str(#lit); });
        }
    }

    fn err(&mut self, span: proc_macro2::Span, msg: &str) {
        self.errors.push(syn::Error::new(span, msg));
    }

    fn node(&mut self, node: &Node) {
        match node {
            Node::Element(el) => self.element(el),
            Node::Fragment(f) => {
                for child in &f.children {
                    self.node(child);
                }
            }
            Node::Text(t) => escape_text(&mut self.buf, &t.value_string()),
            Node::RawText(t) => escape_text(&mut self.buf, &t.to_string_best()),
            Node::Block(block) => match block {
                NodeBlock::ValidBlock(b) => {
                    self.flush();
                    self.stmts.push(quote! {
                        ::nextrs::rsx::Render::render_to(#b, &mut __nx_out);
                    });
                }
                NodeBlock::Invalid(inv) => self.err(inv.span(), "invalid block in rsx!"),
            },
            Node::Doctype(d) => {
                self.buf.push_str("<!DOCTYPE ");
                self.buf.push_str(&d.value.to_string_best());
                self.buf.push('>');
            }
            // Comments are for the source, not the response.
            Node::Comment(_) => {}
            Node::Custom(_) => unreachable!("no custom nodes configured"),
        }
    }

    fn element(&mut self, el: &NodeElement<rstml::Infallible>) {
        let name = el.open_tag.name.to_string();
        if is_component(&el.open_tag.name, &name) {
            self.component(el);
            return;
        }

        self.buf.push('<');
        self.buf.push_str(&name);
        for attr in el.attributes() {
            match attr {
                NodeAttribute::Attribute(ka) => self.html_attribute(ka),
                NodeAttribute::Block(b) => {
                    self.err(b.span(), "attribute spreads are not supported in rsx!")
                }
            }
        }
        self.buf.push('>');

        let void = VOID_ELEMENTS.contains(&name.as_str());
        if void {
            if !el.children.is_empty() {
                self.err(
                    el.open_tag.name.span(),
                    &format!("<{name}> is a void element and cannot have children"),
                );
            }
            return;
        }

        for child in &el.children {
            self.node(child);
        }
        self.buf.push_str("</");
        self.buf.push_str(&name);
        self.buf.push('>');
    }

    fn html_attribute(&mut self, ka: &KeyedAttribute) {
        let key = ka.key.to_string();
        match &ka.possible_value {
            rstml::node::KeyedAttributeValue::None => {
                // Bare attribute: `<input disabled>`.
                self.buf.push(' ');
                self.buf.push_str(&key);
            }
            rstml::node::KeyedAttributeValue::Value(v) => match &v.value {
                KVAttributeValue::Expr(expr) => {
                    // String/number literals render at compile time; bools and
                    // arbitrary expressions go through AttrValue (bool =
                    // presence/absence, Option = omit when None).
                    let static_lit = match expr {
                        syn::Expr::Lit(l) => match &l.lit {
                            syn::Lit::Str(s) => Some(s.value()),
                            syn::Lit::Int(i) => Some(i.base10_digits().to_string()),
                            syn::Lit::Float(f) => Some(f.base10_digits().to_string()),
                            _ => None,
                        },
                        _ => None,
                    };
                    if let Some(value) = static_lit {
                        self.buf.push(' ');
                        self.buf.push_str(&key);
                        self.buf.push_str("=\"");
                        escape_attr(&mut self.buf, &value);
                        self.buf.push('"');
                    } else {
                        self.flush();
                        self.stmts.push(quote! {
                            ::nextrs::rsx::AttrValue::render_attr_to(#expr, #key, &mut __nx_out);
                        });
                    }
                }
                KVAttributeValue::InvalidBraced(inv) => {
                    self.err(inv.span(), "invalid attribute value block");
                }
            },
            rstml::node::KeyedAttributeValue::Binding(b) => {
                self.err(b.span(), "fn-binding attributes are not supported in rsx!");
            }
        }
    }

    fn component(&mut self, el: &NodeElement<rstml::Infallible>) {
        let NodeName::Path(path) = &el.open_tag.name else {
            self.err(el.open_tag.name.span(), "invalid component name");
            return;
        };
        let fn_path = &path.path;

        if !el.children.is_empty() {
            self.err(
                el.open_tag.name.span(),
                "components take no children in rsx! — islands are leaves \
                 (pass server-rendered content as regular elements around the island)",
            );
            return;
        }

        let mut fields: Vec<TokenStream> = Vec::new();
        for attr in el.attributes() {
            let NodeAttribute::Attribute(ka) = attr else {
                self.err(attr.span(), "attribute spreads are not supported in rsx!");
                return;
            };
            let key = ka.key.to_string();
            let Ok(field) = syn::parse_str::<syn::Ident>(&key) else {
                self.err(
                    ka.key.span(),
                    &format!("`{key}` is not a valid prop name (props are snake_case idents)"),
                );
                return;
            };
            match &ka.possible_value {
                rstml::node::KeyedAttributeValue::None => {
                    fields.push(quote! { #field: true });
                }
                rstml::node::KeyedAttributeValue::Value(v) => match &v.value {
                    KVAttributeValue::Expr(expr) => {
                        // String literals get `.into()` so `title="x"` fills a
                        // `String` field; everything else passes through as-is.
                        if matches!(expr, syn::Expr::Lit(l) if matches!(l.lit, syn::Lit::Str(_))) {
                            fields.push(quote! { #field: ::core::convert::Into::into(#expr) });
                        } else {
                            fields.push(quote! { #field: #expr });
                        }
                    }
                    KVAttributeValue::InvalidBraced(inv) => {
                        self.err(inv.span(), "invalid prop value block");
                        return;
                    }
                },
                rstml::node::KeyedAttributeValue::Binding(b) => {
                    self.err(b.span(), "fn-binding props are not supported in rsx!");
                    return;
                }
            }
        }

        self.flush();
        let call = if fields.is_empty() {
            quote! { #fn_path() }
        } else {
            // `Name` -> `NameProps`, preserving any leading path segments.
            let mut props_path = fn_path.clone();
            let last = props_path.segments.last_mut().expect("non-empty path");
            last.ident = syn::Ident::new(&format!("{}Props", last.ident), last.ident.span());
            quote! { #fn_path(#props_path { #(#fields),* }) }
        };
        self.stmts.push(quote! {
            ::nextrs::rsx::Render::render_to(#call, &mut __nx_out);
        });
    }
}

/// A tag name is a component when it's a plain path starting with an
/// uppercase letter (`TodoFilter`, `client::TodoFilter`) — dashed names
/// (`my-widget`) are always HTML elements.
fn is_component(name: &NodeName, name_str: &str) -> bool {
    matches!(name, NodeName::Path(_))
        && name_str
            .rsplit("::")
            .next()
            .and_then(|last| last.chars().next())
            .is_some_and(|c| c.is_ascii_uppercase())
}

/// Compile-time copies of the runtime escapes in `nextrs::rsx` — static text
/// is escaped once here instead of on every request.
fn escape_text(out: &mut String, text: &str) {
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
    }
}

fn escape_attr(out: &mut String, value: &str) {
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '<' => out.push_str("&lt;"),
            _ => out.push(ch),
        }
    }
}
