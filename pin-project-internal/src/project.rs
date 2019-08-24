use proc_macro2::{Span, TokenStream};
use quote::ToTokens;
use syn::{
    parse::Nothing,
    visit_mut::{self, VisitMut},
    *,
};

use crate::utils::{proj_generics, proj_ident, proj_lifetime_name, VecExt, DEFAULT_LIFETIME_NAME};

/// The attribute name.
const NAME: &str = "project";

pub(super) fn attribute(input: TokenStream) -> TokenStream {
    parse(input).unwrap_or_else(|e| e.to_compile_error())
}

fn parse(input: TokenStream) -> Result<TokenStream> {
    let mut stmt = syn::parse2(input)?;
    match &mut stmt {
        Stmt::Expr(Expr::Match(expr)) | Stmt::Semi(Expr::Match(expr), _) => {
            Context::default().replace_expr_match(expr)
        }
        Stmt::Local(local) => Context::default().replace_local(local),
        Stmt::Item(Item::Fn(ItemFn { block, .. })) => Dummy.visit_block_mut(block),
        Stmt::Item(Item::Impl(item)) => replace_item_impl(item),
        _ => {}
    }

    Ok(stmt.into_token_stream())
}

#[derive(Default)]
struct Context {
    register: Option<(Ident, usize)>,
    replaced: bool,
}

impl Context {
    fn update(&mut self, ident: &Ident, len: usize) {
        if self.register.is_none() {
            self.register = Some((ident.clone(), len));
        }
    }

    fn compare_paths(&self, ident: &Ident, len: usize) -> bool {
        match &self.register {
            Some((i, l)) => *l == len && ident == i,
            None => false,
        }
    }

    fn replace_local(&mut self, local: &mut Local) {
        // We need to use two 'if let' expressions
        // here, since we can't pattern-match through
        // a Box
        if let Some((_, expr)) = &mut local.init {
            if let Expr::Match(expr) = &mut **expr {
                self.replace_expr_match(expr);
            }
        }

        // TODO: If `self.replaced` is `true` and `local.pat` is a replaceable pattern,
        // submit an error and suggest splitting the initializer into separate let bindings.
        self.replace_pat(&mut local.pat);
    }

    fn replace_expr_match(&mut self, expr: &mut ExprMatch) {
        expr.arms.iter_mut().for_each(|Arm { pat, .. }| self.replace_pat(pat))
    }

    fn replace_pat(&mut self, pat: &mut Pat) {
        match pat {
            Pat::Ident(PatIdent { subpat: Some((_, pat)), .. })
            | Pat::Reference(PatReference { pat, .. })
            | Pat::Box(PatBox { pat, .. })
            | Pat::Type(PatType { pat, .. }) => self.replace_pat(pat),

            Pat::Struct(PatStruct { path, .. })
            | Pat::TupleStruct(PatTupleStruct { path, .. })
            | Pat::Path(PatPath { qself: None, path, .. }) => self.replace_path(path),

            Pat::Or(PatOr { cases, .. }) => cases.iter_mut().for_each(|pat| self.replace_pat(pat)),
            _ => {}
        }
    }

    fn replace_path(&mut self, path: &mut Path) {
        let len = match path.segments.len() {
            // 1: struct
            // 2: enum
            len @ 1 | len @ 2 => len,
            // other path
            _ => return,
        };

        if self.register.is_none() || self.compare_paths(&path.segments[0].ident, len) {
            self.update(&path.segments[0].ident, len);
            self.replaced = true;
            replace_ident(&mut path.segments[0].ident);
        }
    }
}

fn replace_item_impl(item: &mut ItemImpl) {
    let PathSegment { ident, arguments } = match &mut *item.self_ty {
        Type::Path(TypePath { qself: None, path }) => path.segments.last_mut().unwrap(),
        _ => return,
    };

    replace_ident(ident);

    let mut lifetime_name = String::from(DEFAULT_LIFETIME_NAME);
    proj_lifetime_name(&mut lifetime_name, &item.generics.params);
    item.items
        .iter_mut()
        .filter_map(|i| if let ImplItem::Method(i) = i { Some(i) } else { None })
        .for_each(|item| proj_lifetime_name(&mut lifetime_name, &item.sig.generics.params));
    let lifetime = Lifetime::new(&lifetime_name, Span::call_site());

    proj_generics(&mut item.generics, syn::parse_quote!(#lifetime));

    match arguments {
        PathArguments::None => {
            *arguments = PathArguments::AngleBracketed(syn::parse_quote!(<#lifetime>));
        }
        PathArguments::AngleBracketed(args) => {
            args.args.insert(0, syn::parse_quote!(#lifetime));
        }
        PathArguments::Parenthesized(_) => unreachable!(),
    }
}

fn replace_ident(ident: &mut Ident) {
    *ident = proj_ident(ident);
}

// =================================================================================================
// visitor

struct Dummy;

impl VisitMut for Dummy {
    fn visit_stmt_mut(&mut self, node: &mut Stmt) {
        visit_mut::visit_stmt_mut(self, node);

        let attr = match node {
            Stmt::Expr(Expr::Match(expr)) | Stmt::Semi(Expr::Match(expr), _) => {
                expr.attrs.find_remove(NAME)
            }
            Stmt::Local(local) => local.attrs.find_remove(NAME),
            _ => return,
        };

        if let Some(attr) = attr {
            let res = syn::parse2::<Nothing>(attr.tokens).map(|_| match node {
                Stmt::Expr(Expr::Match(expr)) | Stmt::Semi(Expr::Match(expr), _) => {
                    Context::default().replace_expr_match(expr)
                }
                Stmt::Local(local) => Context::default().replace_local(local),
                _ => unreachable!(),
            });

            if let Err(e) = res {
                *node = Stmt::Expr(syn::parse2(e.to_compile_error()).unwrap())
            }
        }
    }

    fn visit_item_mut(&mut self, _: &mut Item) {
        // Do not recurse into nested items.
    }
}
