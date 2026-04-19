# Installation

## Stable release

### cargo

Prerequisite: rust
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Add Rust to your path (or restart your terminal):
source "$HOME/.cargo/env"
```

Install:
```bash
cargo install tgv
```


### homebrew

```bash
brew tap zeqianli/tgv
brew install tgv

# Test
tgv
```

You will see a warning message on MacOS:

> Apple could not verify "tgv" is free of malware that may harm your Mac or compromise your privacy.

This is because the binary is not yet [signed with an Apple developer account that costs $99/year](https://github.com/archimatetool/archi/issues/555#issuecomment-554965144). I may do this one day. Don't worry, the program is open-source and safe :)

To continue, modify the `Privacy & Security` setting: https://support.apple.com/en-us/102445

### Bioconda

Set up bioconda if you haven't: https://bioconda.github.io/

```bash
conda install bioconda::tgv
```

### Pre-built binaries

Pre-built binaries are found in [Github Releases](https://github.com/zeqianli/tgv/releases/). They are not tested on all operating systems. Please report issues or try another installation method.

Optional: Add to the system PATH:

```bash
# Pick a version at the release page
VERSION=____

# Linux: tgv-x86_64-linux-musl.tar.gz; MacOS: tgv-aarch64-apple-darwin.tar.gz
FILENAME=____

curl -L https://github.com/zeqianli/tgv/releases/download/${VERSION}/${FILENAME}
tar -xzf tgv-aarch64-apple-darwin.tar.gz
sudo mv tgv /usr/local/bin/

# Test
tgv --version
```

Similarly, MacOS would raise a warning here. See the solution above.


## Latest development branch

```bash
git clone https://github.com/zeqianli/tgv.git
cd tgv

cargo install --path .
```
