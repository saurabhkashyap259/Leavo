## What and why

<!-- One or two sentences. Link the issue or spec requirement (FR-…). -->

## Checklist

- [ ] Tests were written first and failed before the change (constitution principle X)
- [ ] `cargo test --workspace`, clippy and fmt pass; web `npm test`, `tsc`, lint and `check:i18n` pass
- [ ] New or changed routes: OpenAPI regenerated, `http-api.md` updated, and the route added to
      `cross_org_matrix_test.rs` if it takes a resource id (a user of another org gets 404)
- [ ] New tables have row-level security; migrations are new files, not edits
- [ ] Interface text comes from `locales/en.json`
- [ ] Deviations from the spec are recorded in `research.md`
- [ ] Commits are signed off (`git commit -s`) and use Conventional Commit messages
