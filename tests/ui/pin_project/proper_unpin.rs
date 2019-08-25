// compile-fail

#![deny(warnings, unsafe_code)]

use pin_project::{pin_project, pinned_drop};
use std::pin::Pin;

struct Inner<T> {
    val: T
}

#[pin_project]
struct Foo<T, U> {
    #[pin]
    inner: Inner<T>,
    other: U
}

fn is_unpin<T: Unpin>() {}

fn bar<T, U>() {
    is_unpin::<Foo<T, U>>(); //~ ERROR E0277
}

fn main() {}
