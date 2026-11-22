## Summary

<!-- What does this change do, and why? Link related issues (e.g. "Closes #12"). -->

## Security

<!-- Security comes first (docs/adr/0012). Can program output, pasted text or files reach new code paths? Are new buffers bounded? New dependencies? Write "No impact" with a reason if none applies. -->

## Testing

<!-- How was this verified? Include the platforms and shells you tested on. Bug fixes: name the test that fails without the fix. -->

## Performance

<!-- Does this touch reading, parsing or drawing? If so, include release-build measurements. Otherwise write "Not on a hot path". -->

## Checklist

- [ ] No new way for program output to act without the user's consent, and every new buffer is bounded
- [ ] Tests cover the change; bug fixes include a regression test
- [ ] `cargo fmt --all --check` passes
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes
- [ ] `cargo test --workspace` passes
- [ ] Documentation updated where relevant
