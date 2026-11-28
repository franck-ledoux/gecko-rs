# `.gcad` fixtures

Made by `scripts/make_gcad_fixtures.py` with OpenCascade, so that the tests
reading them never need it:

```bash
uv run scripts/make_gcad_fixtures.py ~/path/to/manbo-leg
```

- `plate`, `cylinder`, `sphere`, `cone`: primitives built by the script.
- `B10`, `B47`: parts of the Mambo benchmark, imported from its
  `Basic/B10.step` and `Basic/B47.step`. Mambo is © its authors and
  distributed under the Apache License 2.0:
  https://gitlab.com/franck.ledoux/mambo.

The same OpenCascade and the same input give the same bytes: run the script
again and `git diff` shows nothing unless something changed.
