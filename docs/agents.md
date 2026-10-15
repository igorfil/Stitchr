# Using Stitchr from an agent

Stitchr composes local PNG/JPEG files without network access or an interactive session. Pass arguments as an array and use `--json` for a machine-readable result.

- [Runnable cookbook](examples/README.md)
- [Machine-readable cookbook](examples/cases.json)
- [Installation](../README.md#install-the-cli)

## Recommended workflow

### 1. Discover the installed interface

```sh
stitchr --json --help-json
```

The response contains `schema`, including the installed program version, arguments, types, defaults, enums, arity, and examples. `--schema` is an alias for `--help-json`.

### 2. Choose inputs and layout

Use the cookbook to select a layout. Resolve inputs to local files and put them in the intended order. When passing an argument array, expand globs yourself; Stitchr does not expand `*.png`.

| Goal | Layout | Important options |
| --- | --- | --- |
| Contact sheet | `grid` | `--columns`, `--spacing`, `--alignment` |
| Side-by-side comparison | `horizontal` | `--spacing`, `--alignment` |
| Long vertical image | `vertical` | `--spacing`, `--alignment` |
| Explicit placement or overlap | `freeform` | One `--position X Y` per input |
| Diagonal comparison | `split` | Same-size inputs, optional `--divider` |

### 3. Inspect the composition

```sh
stitchr --json docs/examples/light.png docs/examples/dark.png \
  --layout split --divider 4 --background white --print-spec
```

Read `response.spec.width`, `height`, `layout`, `background_color`, and `images`. Each image has a name and a top-left position. Split images all have position `(0, 0)`; the renderer selects their diagonal regions.

`--print-spec` loads and decodes the inputs, but writes no image. It does not validate the export destination or allocate the canvas. Rendering can still fail because of memory limits or output permissions. The printed spec is an inspection format; there is no CLI option to read it back as a render request.

### 4. Render with the same inputs and layout options

```sh
stitchr --json docs/examples/light.png docs/examples/dark.png \
  --layout split --divider 4 --background white --output split.png
```

Success:

```json
{
  "schema_version": 1,
  "ok": true,
  "output": "split.png",
  "width": 640,
  "height": 400,
  "format": "png"
}
```

Check the process exit code and `ok`, then inspect the saved image when visual verification matters. `output` is the destination as supplied, not a resolved absolute path. Relative paths are relative to the process working directory.

## JSON contract, version 1

With `--json`, handled success and failure outcomes produce **one JSON object on stdout**, followed by a newline. No progress messages or human-readable diagnostics are mixed into stdout. Handled errors also use stdout; stderr is empty. Capture both streams so unexpected process/toolchain failures can still be diagnosed.

Every response has:

| Field | Type | Meaning |
| --- | --- | --- |
| `schema_version` | integer | Machine-interface version, currently `1` |
| `ok` | boolean | Whether the requested operation succeeded |

Successful response fields depend on the operation:

| Operation | Additional fields |
| --- | --- |
| Render | `output` (string), `width` and `height` (integers), `format` (`"png"` or `"jpeg"`) |
| `--print-spec` | `spec` (object) |
| `--help-json` / `--schema` | `schema` (object) |
| `--help` / `-h` | `help` (string) |
| `--version` / `-V` | `version` (program version string) |

`spec` contains `schema_version`, `images`, `layout`, `width`, `height`, and `background_color`. Coordinates and dimensions are integers in pixels; the RGB background has integer `red`, `green`, and `blue` channels in `0..255`.

Built-in help/version flags are handled during argument parsing. Otherwise, `--help-json` takes precedence over `--print-spec`, which takes precedence over rendering. Neither discovery nor spec inspection writes an output image.

Without `--json`, `--help-json` and `--print-spec` still print their original top-level objects, now with `schema_version: 1`. Other human-readable output is unchanged: successful renders print the output path, and failures print a diagnostic to stderr.

Place `--json` before `--`. Arguments following `--` are input paths, even if named `--json`. A separate `--json` flag is recognized for error reporting even when an earlier argument is invalid; `--json=true` is invalid syntax and returns `invalid_arguments`.

### Compatibility

The contract version is separate from the program version. Clients should check `schema_version`, ignore unknown additional fields, and handle unfamiliar error codes as generic failures. Removing fields, changing their types, or changing their meaning requires a new contract version. Object key order, whitespace, and human-readable error messages are not stable interfaces.

Missing or malformed JSON is a process/integration failure, not a successful composition.

## Errors and recovery

Example failure:

```json
{
  "schema_version": 1,
  "ok": false,
  "error": {
    "code": "split_size_mismatch",
    "message": "split layout requires all images to have the same dimensions"
  }
}
```

Branch on `error.code`, not `error.message`. Messages may include paths and operating-system details.

| Code | Meaning / recovery |
| --- | --- |
| `invalid_arguments` | Missing inputs, unknown flags, invalid enum/color/numeric values, or wrong argument arity. Inspect the schema and correct the arguments. |
| `input_error` | Input is missing, unreadable, corrupt, or unsupported. Check its path and PNG/JPEG encoding. |
| `unsupported_output_format` | Choose an output ending in `.png`, `.jpg`, or `.jpeg`. |
| `output_error` | Check the destination directory, permissions, available space, and whether the destination is a directory. |
| `freeform_position_count` | Supply exactly one `--position X Y` pair per input image. |
| `split_size_mismatch` | Use identically sized inputs, or choose a grid/stack layout. Stitchr does not resize inputs for split mode. |
| `canvas_too_large` | Reduce spacing, coordinates, or input dimensions/count. The renderer also uses this code when canvas memory cannot be reserved. |
| `no_images` | The composition engine received no images. Supply at least one input. |
| `invalid_image_dimensions` | An input has a zero dimension. Replace it with a valid image. |
| `invalid_columns` | The engine requires positive grid columns. CLI argument validation normally reports this as `invalid_arguments`. |
| `image_count_mismatch` | The render inputs do not match the planned composition. Replan; if persistent, report a bug. |
| `serialization_error` | The composition could not be serialized. Report a bug. |

Some engine-level codes are defensive and are not normally reachable through valid CLI arguments.

### Exit codes

- **0:** success, including help, version, schema, and spec inspection.
- **1:** input, composition, rendering, output-format, serialization, or file-write failure.
- **2:** argument parsing/validation failure.

## Composition rules

- Inputs are local PNG/JPEG paths, in paint order. Repeated paths are allowed. EXIF orientation is applied before planning.
- No automatic resizing or cropping occurs. Output dimensions are derived from inputs and placement.
- Grid/stack spacing sets both gaps and equal outer margins. Grid columns default to `2`; spacing defaults to `0`; alignment defaults to `start`.
- Alignment is within each grid cell or along a stack's cross-axis. Center alignment rounds offsets down to whole pixels.
- Freeform coordinates are nonnegative top-left pixel positions. Later inputs paint over earlier ones. Spacing/alignment do not affect freeform or split mode.
- Split inputs must have identical dimensions. The output shows parallel diagonal slices in input order. Divider width defaults to `0`.
- Background defaults to black. All outputs are opaque; input alpha blends onto the background. JPEG is lossy.
- Coordinates, spacing, divider, and columns are unsigned 32-bit integers; columns must be at least `1`. Large valid integers can still exceed composition limits.
- The RGBA canvas buffer is limited to 512 MiB. Input decoding and intermediate buffers use additional memory.
- Output defaults to `stitched.png`. The extension controls format, case-insensitively. Existing files are overwritten without a prompt; parent directories must already exist.

## Invoke without a shell

```python
import json
import subprocess

process = subprocess.run(
    [
        "stitchr", "--json", "first.png", "second.png",
        "--layout", "horizontal", "--background", "#e2e8f0",
        "--output", "combined.png",
    ],
    capture_output=True,
    text=True,
    check=False,
)
result = json.loads(process.stdout)
if result["schema_version"] != 1:
    print("Unsupported Stitchr contract version")
elif process.returncode != 0 or not result["ok"]:
    print(result["error"]["code"], result["error"]["message"])
else:
    print(result["output"], result["width"], result["height"])
```

Argument-array values need no shell quoting. For shell commands, quote hex/RGB colors and paths containing spaces. Use absolute paths when the agent's working directory is uncertain.

## Cookbook manifest

`examples/cases.json` is versioned separately with `schema_version: 1`. It specifies a repository-root working directory, input fixture paths, and a list of cases. Each case includes an `id`, task description, constraints, exact argument array (excluding the executable), and expected output metadata.

`expected.reference` points to the checked-in result image. `expected.pixels` contains representative pixel coordinates, RGBA values, and per-channel tolerances. `reference_tolerance` applies to decoded reference-image channels, not compressed file bytes. PNG comparisons are exact; JPEG comparisons allow small differences.

CI runs these argument arrays in temporary directories, checks spec inspection has no file side effect, and verifies output metadata, encoding, dimensions, opacity, representative pixels, and reference images:

```sh
cargo test -p stitchr-cli --test cookbook --locked
```
