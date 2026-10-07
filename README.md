# TOML plug-in for NSIS

![License](https://img.shields.io/github/license/nsis-community/plugin-toml?color=blue&style=for-the-badge)
![Release](https://img.shields.io/github/v/release/nsis-community/plugin-toml?style=for-the-badge)
![CI](https://img.shields.io/github/actions/workflow/status/nsis-community/plugin-toml/ci.yml?style=for-the-badge)

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
