# Movy compatibility changes

This fork carries a small tracing and test-execution compatibility layer so
Movy and Peregrine can run Sui Move packages through a local executor while
still using this Sui checkout as the single source of Sui and Move crates.

The goal is not to fork Sui behavior broadly or recreate old VM internals. Movy
should use concrete types and functions already available from Sui wherever
possible. New Sui-side surface should only be added when the existing public
surface cannot provide the data Movy needs.

## Why this exists

Movy fuzzing needs three things from Sui that are not available from the stock
executor surface:

1. A pre-instruction trace event with the current program counter and opcode.
2. A concrete view of the VM operand stack before the instruction executes.
3. A test-style execution mode that can deploy and execute packages locally
   without requiring production-only verifier behavior.

Peregrine depends on Movy for fuzzing and also depends on Sui directly. Keeping
both pointed at this checkout avoids mixed Sui versions in the same Rust graph.

## Concrete trace stack boundary

This fork does not add compatibility crates such as `move-vm-types` or
`move-vm-stack`, and it does not synthesize old VM value APIs. Movy consumes the
concrete trace values that Sui already knows how to produce.

Added to `move-trace-format`:

- `TraceStack`
- `TraceStack::values`
- `TraceStack::last`
- `TraceStack::last_n`

`TraceStack` is an ordered snapshot of the VM operand stack before an
instruction executes. Its entries are `Option<TraceValue>` because the existing
Sui trace resolver can only expose values that have a valid trace
representation at that point. Missing values stay unavailable instead of being
replaced with placeholder or stub values.

Why: Sui already has a concrete `TraceValue`/`SerializableMoveValue` trace
format. The missing piece for Movy was a pre-instruction stack view, not a
separate VM value hierarchy.

## Trace API changes

Changed `move-trace-format` so a tracer receives an optional stack snapshot:

```rust
fn notify(
    &mut self,
    event: &TraceEvent,
    writer: &mut Writer<'_>,
    stack: Option<&TraceStack>,
) -> bool;
```

Also added:

- `Tracer::wants_effects`, defaulting to `true`
- `TraceEvent::BeforeInstruction`
- `ExtraInstructionInformation`
- `MoveTraceBuilder::before_instruction_with_stack`
- `MoveTraceBuilder::push_event_with_stack`

The existing built-in trace-format tracers were updated to the new method
signature. `move-vm-profiler` and `move-coverage` were updated so the new event
does not break existing trace consumers.

Why: Movy wants to inspect the VM before the instruction mutates the operand
stack. The old post-instruction trace is too late for some branch, comparison,
overflow, and type conversion checks.

## Runtime tracing changes

Changed `move-vm-runtime` tracing to build a concrete `TraceStack` from the
existing runtime trace-value resolver before executing each instruction. The
runtime now emits a `BeforeInstruction` event with:

- opcode name
- program counter
- remaining gas
- selected instruction metadata, such as branch offsets or local indexes
- the concrete trace stack snapshot

The runtime also honors `Tracer::wants_effects`. If a tracer does not need
effect events, the VM can skip some expensive trace value conversion work.

Why: Movy's fuzz feedback and Sui oracles are based on instruction-level
signals. Capturing a `TraceStack` gives Movy those signals through the existing
trace-value representation without exposing the full private runtime value
representation.

## Trace snapshot recursion guard

`move-vm-runtime` now bounds recursive root-location snapshot resolution while
building `TraceValue` snapshots for references. If a traced runtime location
chain is cyclic or deeper than `MAX_ROOT_LOCATION_SNAPSHOT_DEPTH`, the snapshot
resolver returns `None` for that value instead of recursing until the tracing
thread overflows its stack.

Why: Movy and Peregrine fuzz arbitrary public functions, so the tracer can see
reference-location shapes that normal trace consumers rarely exercise. A missing
trace value is already represented by `TraceStack` as `Option<TraceValue>`;
aborting the whole process is not acceptable for a fuzzing UI. This keeps the
full trace path concrete and preserves normal execution semantics while making
trace value materialization fail closed.

## Coverage changes

`move-coverage` now ignores `TraceEvent::BeforeInstruction` in the same class as
other non-coverage events.

Why: The new event is for fuzzing/tracing. It should not change source coverage
semantics or make coverage tools fail when reading traces produced by this fork.

## Test execution feature

Added a `testing` feature through `sui-execution` and the versioned execution
crates.

The feature is propagated to:

- `sui-adapter-*`
- `sui-verifier-*`
- `sui-move-natives-*` where applicable
- Move VM runtime testing features

When `testing` is enabled:

- Latest and v3 adapters install an in-memory test store extension.
- `TransactionContext::new` sets `test_only` from `cfg!(feature = "testing")`.
- Verifiers skip the production-only init-call and one-time-witness
  instantiation restrictions that block local test-style execution.

Why: Movy deploys and replays packages in a local executor, closer to a testing
environment than to a production validator path. This lets Movy fuzz ordinary
packages locally while keeping normal Sui execution behavior unchanged unless
the `testing` feature is explicitly enabled.

## Production behavior boundary

The intended boundary is:

- Normal Sui builds do not opt into `sui-execution/testing`.
- Verifier relaxations are behind `cfg!(feature = "testing")`.
- Test store insertion is behind `#[cfg(feature = "testing")]`.
- Tracing changes are only active when the Move VM tracing path is enabled.

Do not enable the `testing` feature in production binaries unless you
intentionally want test-mode verifier and native behavior.

## Rebase checklist

When rebasing this fork onto a newer Sui version:

1. Do not re-add stub or compatibility value crates. First check whether the
   target Sui version already exposes the concrete tracing data Movy needs.
2. Re-check `move_trace_format::format::TraceStack`; preserve the stack helpers
   Movy uses if upstream still lacks an equivalent pre-instruction stack view.
3. Re-check `move_trace_format::interface::Tracer`; preserve the stack argument
   and `wants_effects` behavior if Movy still depends on them.
4. Re-check the VM tracing path and make sure `BeforeInstruction` is emitted
   before operand stack mutation.
5. Re-check coverage and profiler trace consumers so they either handle or
   ignore `BeforeInstruction`.
6. Re-check all versioned `sui-execution` adapters and verifiers; Sui keeps
   multiple execution versions, and Movy can hit more than just `latest`.
7. Keep the test execution behavior feature-gated.

## Verification used with Movy and Peregrine

The compatibility layer was verified from the consumer side with:

```sh
cargo check -p movy --lib --no-default-features --features sui-fuzz
cargo check -p movy-fuzz --no-default-features
cargo tree -p movy-fuzz --no-default-features -e normal
cargo check -p peregrine-movy-fuzz-adapter
cargo check -p peregrine
cargo run -p peregrine -- --peregrine-movy-fuzz /Users/eieiron/dev/gm_contracts/savings_personal .
```

The direct Peregrine Movy fuzz helper run completed a 30 second public-function
fuzz campaign with 105 public targets and 1248 queue entries after the trace
snapshot recursion guard was added.

`cargo tree -p peregrine -i sui-types` was also used to confirm that Peregrine
and Movy both resolve `sui-types` from this local Sui checkout.
