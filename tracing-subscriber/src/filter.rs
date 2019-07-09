
use crate::layer::{Ctx, Layer};
use tracing_core::{subscriber::{Subscriber, Interest}, Metadata};
use std::marker::PhantomData;

pub trait Filter<S>
where
    Self: 'static,
    S: Subscriber,
{
    fn callsite_enabled(&self, metadata: &'static Metadata<'static>) -> Interest {
        if self.enabled(metadata, &Ctx::none()) {
            Interest::always()
        } else {
            Interest::never()
        }
    }

    fn enabled(&self, metadata: &Metadata, ctx: &Ctx<S>) -> bool;
}

pub trait FilterExt<S>
where
    Self: Filter<S>,
    S: Subscriber,
{
    fn or<B>(self, b: B) -> Or<Self, B>
    where
        Self: Sized,
        B: Filter<S>,
    {
        Or { a: self, b }
    }

    fn and<B>(self, b: B) -> And<Self, B>
    where
        Self: Sized,
        B: Filter<S>,
    {
        And { a: self, b }
    }
}

#[derive(Clone, Debug)]
pub struct EnabledFn<F, S> {
    f: F,
    _s: PhantomData<fn(S)>,
}

#[derive(Clone, Debug)]
pub struct InterestFn<F, S> {
    f: F,
    _s: PhantomData<fn(S)>,
}

#[derive(Clone, Debug)]
pub struct Or<A, B> {
    a: A,
    b: B,
}

#[derive(Clone, Debug)]
pub struct And<A, B> {
    a: A,
    b: B,
}

pub fn enabled_fn<F, S>(f: F) -> EnabledFn<F, S>
where
    F: Fn(&Metadata) -> bool + 'static,
{
    EnabledFn::from(f)
}

pub fn callsite_fn<F, S>(f: F) -> InterestFn<F, S>
where
    F: Fn(&Metadata) -> Interest + 'static,
{
    InterestFn::from(f)
}

// === impl Layer ===

impl<F, S> Layer<S> for F
where
    F: Filter<S>,
    S: Subscriber,
{
    fn register_callsite(&self, metadata: &'static Metadata<'static>, prev: Interest) -> Interest {
        let my_interest = self.callsite_enabled(metadata);
        if my_interest.is_always() {
            prev
        } else {
            my_interest
        }
    }

    fn enabled(&self, metadata: &Metadata, prev: bool, ctx: Ctx<S>) -> bool {
        Filter::enabled(self, metadata, &ctx) && prev
    }
}

// === impl EnabledFn ===

impl<F, S> Filter<S> for EnabledFn<F, S>
where
    F: Fn(&Metadata) -> bool + 'static,
    S: Subscriber,
{
    fn enabled(&self, metadata: &Metadata, _: &Ctx<S>) -> bool {
        (self.f)(metadata)
    }
}

impl<F, S> From<F> for EnabledFn<F, S>
where
    F: Fn(&Metadata) -> bool + 'static,
{
    fn from(f: F) -> Self {
        Self {
            f,
            _s: PhantomData,
        }
    }
}

// === impl InterestFn ===

impl<F, S> Filter<S> for InterestFn<F, S>
where
    F: Fn(&Metadata) -> Interest + 'static,
    S: Subscriber,
{
    fn enabled(&self, metadata: &Metadata, _: &Ctx<S>) -> bool {
        let my_interest = (self.f)(metadata);
        my_interest.is_always() || my_interest.is_sometimes()
    }

    fn callsite_enabled(&self, metadata: &'static Metadata<'static>) -> Interest {
        (self.f)(metadata)
    }
}

impl<F, S> From<F> for InterestFn<F, S>
where
    F: Fn(&'static Metadata<'static>) -> Interest + 'static,
{
    fn from(f: F) -> Self {
        Self {
            f,
            _s: PhantomData,
        }
    }
}

// === impl FilterExt ===

impl<F, S> FilterExt<S> for F
where
    F: Filter<S>,
    S: Subscriber,
{}

// === impl And ===

impl<A, B, S> Filter<S> for And<A, B>
where
    A: Filter<S>,
    B: Filter<S>,
    S: Subscriber,
{
    fn enabled(&self, metadata: &Metadata, ctx: &Ctx<S>) -> bool {
        self.a.enabled(metadata, ctx) && self.b.enabled(metadata, ctx)
    }

    fn callsite_enabled(&self, metadata: &'static Metadata<'static>) -> Interest {
        let a = self.a.callsite_enabled(metadata);
        let b = self.b.callsite_enabled(metadata);
        match () {
            _ if a.is_sometimes() && b.is_sometimes() => Interest::sometimes(),
            _ if a.is_always() && !b.is_never() => Interest::always(),
            _ if b.is_always() && !a.is_never() => Interest::always(),
            _ => Interest::never(),
        }
    }
}

// === impl Or ===

impl<A, B, S> Filter<S> for Or<A, B>
where
    A: Filter<S>,
    B: Filter<S>,
    S: Subscriber,
{
    fn enabled(&self, metadata: &Metadata, ctx: &Ctx<S>) -> bool {
        self.a.enabled(metadata, ctx) || self.b.enabled(metadata, ctx)
    }

    fn callsite_enabled(&self, metadata: &'static Metadata<'static>) -> Interest {
        let a = self.a.callsite_enabled(metadata);
        let b = self.b.callsite_enabled(metadata);
        match () {
            _ if a.is_always() || b.is_always() => Interest::always(),
            _ if a.is_sometimes() || b.is_sometimes() => Interest::sometimes(),
            _ => Interest::never(),
        }
    }
}
