# Comment Conventions

- Default to none. A comment earns its place for a non-obvious why or constraint.
- One line at the top of a file says what it holds: `//!` in Rust, a module
  docstring in Python.
- Before a function that needs it: one plain line starting with its name,
  Go-style (`// raise_memlock lifts ...`).
- If a comment is needed to explain what a function or variable does, fix the
  name instead.
- No adverbs ("only", "safely", "usually", "never", "already"), no tags.
- `// SAFETY:` on every `unsafe` block.
- Do not keep dead code in comments.
- Never delete or reword comments the user wrote, even when they break these
  rules; point it out at most.

Example:

```rust
//! Scaler turns syscall rates into replica counts, using a calibration and a target CPU limit.

// Confirm waits for `polls` consecutive polls asking for more replicas than run, so one noisy poll scales nothing.
pub struct Confirm {
```
