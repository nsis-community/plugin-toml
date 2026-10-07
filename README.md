# TOML plug-in for NSIS

[![License: MIT](https://img.shields.io/github/license/nsis-community/plugin-toml)](LICENSE)
[![Release](https://img.shields.io/github/v/release/nsis-community/plugin-toml)](https://github.com/nsis-community/plugin-toml/releases)
[![CI](https://github.com/nsis-community/plugin-toml/actions/workflows/ci.yml/badge.svg)](https://github.com/nsis-community/plugin-toml/actions/workflows/ci.yml)

Read and write TOML 1.0 files from NSIS scripts, keeping comments, spacing and key order intact.

> [!NOTE]
> **Looking for the usage guide?** Commands, paths, recipes and caveats are in [Docs/TOML/README.md](Docs/TOML/README.md).

## Installation

Download the installer or archive from the [Releases page](https://github.com/nsis-community/plugin-toml/releases).

If you downloaded the zip archive, extract it into your NSIS folder: it adds `TOML.dll` to `Plugins/<variant>/` and `TOML.nsh` to `Include/`.

The plug-in works as soon as the DLL is in place. For `${TomlForEach}` loops, add `!include "TOML.nsh"` to your script.

## Building

Needs [mise](https://mise.jdx.dev). `mise run checks` runs formatting, lints and tests, and `mise run build` writes both variants to `dist/Plugins/`. `mise run smoke` builds the smoke installer and runs it under Wine.

## License

[MIT](LICENSE)
