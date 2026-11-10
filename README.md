# Stitchr

Combine PNG, JPEG, WebP, and GIF images into grids, stacks, freeform arrangements, or diagonal split comparisons, with solid or gradient backgrounds, borders, and labels.

Stitchr is a local image-composition CLI for macOS and Linux, built for both people and AI agents. Agents get a machine-readable argument schema, layout inspection before rendering, and versioned JSON results with stable error codes.

![Light and dark screenshots with labels and white borders on an orange-to-purple gradient](examples/17-before-after.png)

**Explore:** [Examples](examples/README.md) · [Runnable cookbook](docs/examples/README.md) · [Agent usage guide](docs/agents.md) · [Machine-readable examples](docs/examples/cases.json)

## Install the CLI

You need Git, a current stable [Rust toolchain](https://rustup.rs/), and a native C linker. On macOS, install Xcode Command Line Tools with `xcode-select --install`; on Debian/Ubuntu, install `build-essential`.

```sh
cargo install --git https://github.com/igorfil/Stitchr stitchr-cli --locked
stitchr --help
```

Cargo installs `stitchr` into `~/.cargo/bin`; make sure that directory is on your `PATH`. To install from a local checkout instead (for example, to run the examples against the included sample images):

```sh
git clone https://github.com/igorfil/Stitchr.git
cd Stitchr
cargo install --path cli --locked
```

## Usage

```sh
stitchr before.png after.png --layout horizontal --spacing 24 -o combined.png
```

The [examples](examples/README.md) show every option with its command and output image:

| Feature | Options | Examples |
| --- | --- | --- |
| Layouts | `--layout grid\|horizontal\|vertical\|freeform\|split`, `--columns`, `--position` | [Layouts](examples/README.md#1-layouts) |
| Spacing and alignment | `--spacing`, `--alignment`, `--divider` | [Spacing and alignment](examples/README.md#2-spacing-and-alignment) |
| Backgrounds | `--background`, `--background-gradient`, `--gradient-kind`, `--gradient-angle` | [Backgrounds](examples/README.md#3-backgrounds) |
| Resizing | `--fit-height`, `--fit-width`, `--fit cover\|contain`, `--max-width`, `--max-height` | [Agent guide](docs/agents.md#composition-rules) |
| Borders and labels | `--label`, `--border-width`, `--border-color`, `--label-size`, `--label-position inside\|outside`, `--label-color`, `--font` | [Borders and labels](examples/README.md#4-borders-and-labels) |
| Boxes and arrows | `--box X Y W H`, `--arrow X1 Y1 X2 Y2`, `--annotation-color`, `--annotation-width` | [Agent guide](docs/agents.md#composition-rules) |
| Combined recipes | Single image with a title, before/after, labeled grid | [Putting it together](examples/README.md#5-putting-it-together) |
| Output and inspection | `-o file.png\|.jpg\|.webp\|-`, `--format`, `--jpeg-quality`, `--print-spec`, `--spec`, `--json` | [Output and inspection](examples/README.md#6-output-and-inspection) |

Run `stitchr --help` for the full option reference, or `stitchr --help-json` (also `--schema`) for the same reference as JSON.

### Use from an agent

Add `--json` to receive versioned success records and actionable error codes on stdout:

```sh
stitchr --json docs/examples/light.png docs/examples/dark.png \
  --layout split --output comparison.png
```

```json
{"format":"png","height":400,"ok":true,"output":"comparison.png","output_abs":"/home/me/comparison.png","scale":1.0,"schema_version":1,"warnings":[],"width":640}
```

Errors use `{"schema_version":1,"ok":false,"error":{"code":"...","message":"..."}}` and a nonzero exit code. `--json` also wraps spec/schema queries and keeps help and argument errors machine-readable. Options that have no effect (such as `--spacing` with `--layout split`) and labels that overflow their image are reported as `warnings`; add `--strict` to make them errors. The [agent guide](docs/agents.md) documents mode combinations, exit codes, recovery, and compatibility rules.

### Output behavior

- The output extension selects PNG (`.png`), JPEG (`.jpg`, `.jpeg`), or lossless WebP (`.webp`). `--format png|jpeg|webp` overrides the extension. Existing output files are overwritten, but Stitchr refuses to overwrite one of its inputs unless you pass `--force`. Use `--no-clobber` to never overwrite, and `--mkdir` to create missing parent directories.
- `--jpeg-quality` sets JPEG quality from 1 to 100 (default 75). It is ignored for PNG.
- Use `-` as an input to read one image from stdin, and `-o -` with `--format` to write the image to stdout: `cat a.png | stitchr - b.png -o - --format png > out.png`. `-o -` cannot be combined with `--json`. Use `./-` for a file literally named `-`.
- Inputs can be PNG, JPEG, WebP, or GIF; the format is detected from the file content, and animated images use their first frame. Output is PNG, JPEG, or WebP.
- Images keep their original pixel dimensions, after applying EXIF orientation, unless you pass a fit option. Images are only ever shrunk: `--fit-height min` gives a horizontal stack equal heights, and `--max-width 1200` shrinks the images so the whole output fits while spacing, borders, and labels keep their size.
- Colors accept names, `'#RRGGBB'`, and `'rgb(r,g,b)'`; quote colors containing shell metacharacters.
- Output is opaque: transparent input pixels blend onto the selected background.
- The canvas buffer is limited to 512 MiB; oversized compositions return an error. Input images and intermediate buffers require additional memory.

## Development

The optional Nix development shell provides Cargo, Rust, rustfmt, and Clippy:

```sh
nix develop
```

Run the Rust checks from the repository root:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

The workspace tests execute every command in [the cookbook manifest](docs/examples/cases.json), checking dimensions, formats, representative pixels, and checked-in output images.

GitHub Actions runs these checks on macOS and Linux.

### Repository structure

- **`core/`** — image loading, layout planning, rendering, and export.
- **`cli/`** — command-line interface and integration tests.

## License

Licensed under [BSD-3-Clause](LICENSE).
