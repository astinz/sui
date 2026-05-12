// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::format::TraceEvent;
use crate::interface::{Tracer, Writer};

pub struct NopTracer;
impl Tracer for NopTracer {
    fn notify(
        &mut self,
        _event: &TraceEvent,
        _writer: &mut Writer<'_>,
        _stack: Option<&crate::format::TraceStack>,
    ) -> bool {
        // keep all events
        true
    }

    fn wants_effects(&self) -> bool {
        true
    }
}
