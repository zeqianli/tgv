# Installation

## Stable release

- cargo: `cargo install tgv --locked`
- brew: `brew tap zeqianli/tgv && brew install tgv`
- bioconda: `conda install bioconda::tgv`
- Pre-built binaries: [GitHub Releases](https://github.com/zeqianli/tgv/releases/)

You may see a warning message on MacOS:

> Apple could not verify "tgv" is free of malware that may harm your Mac or compromise your privacy.

This binary is not yet [signed with an Apple developer account that costs $99/year](https://github.com/archimatetool/archi/issues/555#issuecomment-554965144). Don't worry, the program is open-source and safe. To continue, modify the `Privacy & Security` setting: https:

Similarly, MacOS would raise a warning here. See the solution above.

## Latest development branch

```bash
git clone https://github.com/zeqianli/tgv.git
cd tgv

cargo install --path crates/tgv --locked
```
