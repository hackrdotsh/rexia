# Program icons

`catalog.json` owns the 158 program identities, exact command aliases, flat tile
colors, source URLs, licenses, adaptation notes, and asset hashes.
[SOURCES.md](SOURCES.md) lists their provenance. Downloaded artwork is preserved
in `originals/`; adapted monochrome SVGs are beside this file.

Generate the compiled Rust catalog and source index offline:

```sh
python3 assets/programs/generate.py
python3 assets/programs/generate.py --check
```

Shell variants follow each pane's terminal foreground/background. Other tiles
use stable solid backgrounds and a contrasting foreground, including explicit
white Claude Code, co, and Buildkite marks. Codex uses the OpenAI mark. Pixi uses
the user-selected Prefix mark. Clang uses LLVM's dragon with user approval.
Delta is delta.dev, Sail is LakeSail, and Minimal is minimal.dev.

The six shell pictograms, Browser, Git Changes, Inspector, DBCLI, and Gojo are
functional symbols, not claims of distinct third-party logos. Gojo's branch and
DBCLI's database/prompt were explicitly requested. The supplied Sofka reference
is represented by Dashboard Icons' cat mark, not the project's cat photograph.

Git's logo is by Jason Long, licensed under
[CC BY 3.0](https://creativecommons.org/licenses/by/3.0/).
[Original and attribution](https://git-scm.com/community/logos).
Llama artwork is from [ggml-org/llama.brand](https://github.com/ggml-org/llama.brand);
its license and explicit nominative software-distribution permission are in
`licenses/llama.txt` and `licenses/llama-brand-usage.md`.

Collection and project notices are preserved in `licenses/`. Collection licenses
do not waive third-party trademark rights or separately license every brand.
Entries without a published asset license say so explicitly; public availability
is not a copyright grant. The marks identify running programs, not endorsement
or affiliation. No artwork from Rex's gated icon pack is included: its dinosaur
is adapted from the public website's app icon. Solid backgrounds and monochrome
adaptations are client styling, not a claim of compliance with all brand guides.
