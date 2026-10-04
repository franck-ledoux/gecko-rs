# Writing documentation

The site is [MkDocs](https://www.mkdocs.org/) with the
[Material](https://squidfunk.github.io/mkdocs-material/) theme, matching
GECKO's.

```bash
pip install -r requirements-docs.txt
./docs.sh            # build everything into site/
mkdocs serve         # live preview on http://127.0.0.1:8000
```

`docs.sh` builds the Markdown **and** the rustdoc reference, putting the latter
under `site/api/` where [the API page](api.md) links to it.

## Strict by default

`docs.sh` passes `--strict`, so a warning fails the build. In practice that
means a link to a page that does not exist, or a page that is not in the
navigation. Both are worth failing on: a dead link in documentation is worse
than no link, because it costs the reader the time to find out.

Raw HTML is left alone by MkDocs, which is how the API page links into the
rustdoc site without `--strict` objecting to a path it cannot resolve.

## Formatting

Markdown is formatted with `mdformat`, as in GECKO:

```bash
mdformat docs README.md          # rewrite
mdformat --check docs README.md  # verify, as CI does
```

## What goes where

- **`index.md`** — what the project is and the two ways in. Someone who reads
  only this page should know whether it is for them.
- **`user-guide/`** — how to use it. No reasoning about the implementation.
- **`developer-guide/`** — how it is put together and why. This is where a
  decision and its cost belong, so the next person does not have to rediscover
  them.

The thing worth writing down is rarely what the code does — that is in the
code. It is the alternative that was rejected, and what it would have cost.

## Publishing

The CI builds the site on every push, which is what keeps it honest. It is not
deployed yet: GitHub Pages on a private repository needs a paid plan, so the
deploy job is written and waits for the repository to go public — the condition
is `!github.event.repository.private`, so nothing has to be switched on by hand.
