# CI workflow (parked)

The GitHub Actions workflow lives here instead of `.github/workflows/` because
the CLI token that created this repo lacked the `workflow` OAuth scope.

To activate it:

```bash
gh auth refresh -h github.com -s workflow
git mv ci/ci.yml .github/workflows/ci.yml
git commit -m 'ci: activate workflow' && git push
```
