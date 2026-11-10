# Using Stitchr from an agent

Stitchr composes local PNG, JPEG, WebP, and GIF files into a PNG, JPEG, or WebP image without network access or an interactive session. Pass arguments as an array and use `--json` for a machine-readable result.

- [Runnable cookbook](examples/README.md)
- [Machine-readable cookbook](examples/cases.json)
- [Installation](../README.md#install-the-cli)

## Recommended workflow

### 1. Discover the installed interface

```sh
stitchr --json --help-json
```

The response contains `schema`, including the installed program version, arguments, types, defaults, enums, arity, and examples. `--schema` is an alias for `--help-json`.

Each argument also lists `minimum`/`maximum` for integers, `conflicts_with`, and when it has an effect: `applies_to_layouts` (`null` means every layout), `requires` (for example `["--label"]`), `ignored_with` (`--print-spec`, `-o -`), and where relevant `applies_to_formats` or `applies_to_gradient_kinds`. The schema also lists `error_codes` (with each code's `exit_code`), `warning_codes`, and `exit_codes`.

For a standard [JSON Schema](https://json-schema.org/draft/2020-12/schema) of a request, with one property per argument, pass `--schema=jsonschema` (or `--help-json=jsonschema`). It can be used directly as a tool input schema; each property's `x-stitchr-flag` names the CLI flag it maps to. The `=` is required, so `--schema image.png` still treats `image.png` as an input.

### 2. Choose inputs and layout

Use the cookbook to select a layout. Resolve inputs to local files and put them in the intended order. When passing an argument array, expand globs yourself; Stitchr does not expand `*.png`.

| Goal | Layout | Important options |
| --- | --- | --- |
| Contact sheet | `grid` | `--columns`, `--spacing`, `--alignment` |
| Side-by-side comparison | `horizontal` | `--spacing`, `--alignment` |
| Long vertical image | `vertical` | `--spacing`, `--alignment` |
| Explicit placement or overlap | `freeform` | One `--position X Y` per input |
| Diagonal comparison | `split` | Same-size inputs (or `--fit cover\|contain`), optional `--divider` |

### 3. Inspect the composition

```sh
stitchr --json docs/examples/light.png docs/examples/dark.png \
  --layout split --divider 4 --background white --print-spec
```

Read `response.spec.width`, `height`, `layout`, `background_color`, and `images`. Each image has a `name`, the `width` and `height` it is drawn at, and a top-left `position`. Split images all have position `(0, 0)`; the renderer selects their diagonal regions.

`--print-spec` loads and decodes the inputs, but writes no image. It does not validate the export destination or allocate the canvas. Rendering can still fail because of memory limits or output permissions. Each image in the printed spec also has a `path`: the input made absolute (or `"-"` for stdin).

### 3b. Optionally, patch the spec and render it

`--spec FILE` (or `--spec -` for stdin) renders a spec printed by `--print-spec` instead of composing positional inputs. This gives shell-free, patchable requests: edit `position`, `background_color`, `background_gradient`, `frame`, `width`, or `height`, then render. The spec is authoritative: images are drawn at their `position` on a `width` x `height` canvas, and layout options such as `--spacing` are ignored (each one passed explicitly produces an `option_ignored` warning). Output options (`-o`, `--format`, `--jpeg-quality`, `--force`, `--no-clobber`, `--mkdir`) and `--print-spec` work as usual.

```sh
stitchr --json a.png b.png --layout horizontal --print-spec > spec.json
# edit spec.json ...
stitchr --json --spec spec.json -o out.png
```

Rules:

- Inputs are loaded from each image's `path`; relative paths are relative to the process working directory. `name` is informational.
- Each input must still have the `width` and `height` recorded in the spec, otherwise the request fails with `image_size_mismatch`.
- The file can be the bare spec printed by `--print-spec` or the whole `--json --print-spec` response; the `spec` object is taken from the response. An error response is rejected with `invalid_spec`.
- `schema_version`, when present, must be `1`. Unknown fields are ignored.
- `--spec` cannot be combined with positional inputs. With `--spec -`, no image may also use `-`.
- Malformed or inconsistent specs fail with `invalid_spec` (exit code 2), or with the engine's error code, such as `label_count_mismatch` or `split_size_mismatch`.

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
  "output_abs": "/home/me/project/split.png",
  "width": 640,
  "height": 400,
  "format": "png",
  "scale": 1.0,
  "warnings": []
}
```

Check the process exit code and `ok`, then inspect the saved image when visual verification matters. `output` is the destination as supplied; `output_abs` is the same path made absolute against the process working directory (symlinks are not resolved). With `-o -`, `output_abs` is `null`.

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
| Render | `output` (string), `output_abs` (string or `null`), `width` and `height` (integers), `format` (`"png"`, `"jpeg"`, or `"webp"`), `scale` (number, below 1 when `--max-width`/`--max-height` shrank the images), `warnings` (array) |
| `--print-spec` | `spec` (object), `warnings` (array) |
| `--help-json` / `--schema` | `schema` (object) |
| `--help` / `-h` | `help` (string) |
| `--version` / `-V` | `version` (program version string) |

`spec` contains `schema_version`, `images`, `layout`, `width`, `height`, and `background_color`. Coordinates and dimensions are integers in pixels; the RGB background has integer `red`, `green`, and `blue` channels in `0..255`.

### Warnings

A request can succeed but probably not do what was intended. Renders and `--print-spec` then list `warnings`, which is an empty array when there is nothing to report. Each warning has a stable `code` and a human-readable `reason`:

| Code | Extra fields | Meaning |
| --- | --- | --- |
| `option_ignored` | `option` | An explicitly passed option has no effect, for example `--spacing` with `--layout split`, label styling without `--label`, `--gradient-angle` with a radial gradient, `--jpeg-quality` for PNG output, or output options with `--print-spec`. |
| `label_overflow` | `index`, `label`, `tab_width`, `available_width` | A label tab is wider than its image (or split band) and extends past it. |
| `missing_glyphs` | `index`, `label`, `characters` | The label font cannot draw these characters; they render as gaps. Pass `--font` with a font that covers them. |
| `format_mismatch` | `option`, `format`, `path`, `extension_format` | `--format` writes a different format than the output extension suggests, for example JPEG data in `out.png`. |
| `tiny_images` | `index`, `width`, `height`, `source_width`, `source_height`, `scale`, `min_side` | `--max-width`/`--max-height` shrinks an image below 16 px on a side. Raise the limit, or reduce the number of inputs or the spacing. |
| `fit_not_applied` | `option`, `index`, `size`, `target` | An image is already smaller than the `--fit-height`/`--fit-width` target, so it is left unchanged. |

```json
{"code": "option_ignored", "option": "--spacing", "reason": "--spacing is not used by the split layout"}
```

Without `--json`, each warning is printed to stderr as `stitchr: warning: <reason>`. With `--json`, warnings appear only in the JSON object and stderr stays empty.

`--strict` turns any warning into an `invalid_arguments` error (exit code 2) before anything is written. It also applies to `--print-spec`, so a request can be checked without rendering.

Built-in help/version flags are handled during argument parsing. Otherwise, `--help-json` takes precedence over `--print-spec`, which takes precedence over rendering. Neither discovery nor spec inspection writes an output image.

Without `--json`, `--help-json` and `--print-spec` still print their original top-level objects, now with `schema_version: 1`. Other human-readable output is unchanged: successful renders print the output path (nothing extra with `-o -`), and failures print a diagnostic to stderr.

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

Many errors also carry an `error.details` object with machine-readable context. It is omitted when there is nothing to add:

| Code | `details` fields |
| --- | --- |
| `invalid_arguments` | `kind` (for example `value_validation`, `invalid_value`, `unknown_argument`, `argument_conflict`, `missing_required_argument`, `strict`), and when known `option`, `value`, `reason`, `allowed` (valid enum values), `suggestion` (closest flag or value). With `--strict`, `warnings` lists every warning. |
| `input_error` | `index` (input position) and `path` (`"-"` for stdin) |
| `split_size_mismatch` | `index`, `path`, `width`, `height` of the first differing input, and `expected` (`width`, `height`) |
| `canvas_too_large` | `width`, `height`, `bytes`, `limit_bytes` when the canvas buffer exceeds the memory limit, or `max_dimension` when a side would not fit in 32 bits |
| `freeform_position_count`, `label_count_mismatch`, `image_count_mismatch` | `expected`, `actual` |
| `unsupported_output_format` | `path`, and `supported_extensions` or `supported_formats` |
| `output_error` | `path` |

```json
{"code": "split_size_mismatch", "message": "...", "details": {"index": 1, "path": "shot3.png", "width": 1024, "height": 768, "expected": {"width": 1280, "height": 800}}}
```

| Code | Meaning / recovery |
| --- | --- |
| `invalid_arguments` | Missing inputs, unknown flags, invalid enum/color/numeric values, wrong argument arity, or a repeated single-use option such as `--background-gradient`. Also: `-` used as an input more than once, `-o -` with `--json`, `-o -` without `--format`, or any warning under `--strict`. Inspect the schema and correct the arguments. |
| `input_error` | Input is missing, unreadable, corrupt, or unsupported. Check its path (or stdin bytes for `-`) and that it is a PNG, JPEG, WebP, or GIF image. |
| `unsupported_output_format` | Choose an output ending in `.png`, `.jpg`, `.jpeg`, or `.webp`, or pass `--format png\|jpeg\|webp`. |
| `output_error` | Check the destination directory, permissions, available space, and whether the destination is a directory. Pass `--mkdir` to create missing parent directories. |
| `output_conflicts_with_input` | The output is the same file as an input (including through symlinks or hard links). Choose another output, or pass `--force` to overwrite the input. |
| `output_exists` | `--no-clobber` was given and the output already exists. Choose another output or drop `--no-clobber`. |
| `freeform_position_count` | Supply exactly one `--position X Y` pair per input image. |
| `split_size_mismatch` | Use identically sized inputs, pass `--fit cover` or `--fit contain`, or choose a grid/stack layout. |
| `label_count_mismatch` | Supply exactly one `--label` per input image, or none. Use `--label ""` for a border without a label. |
| `canvas_too_large` | Reduce spacing, coordinates, or input dimensions/count. The renderer also uses this code when canvas memory cannot be reserved. |
| `canvas_cannot_fit` | `--max-width`/`--max-height` is smaller than the space taken by spacing, borders, and labels, even with images shrunk to one pixel. Raise the limit or reduce spacing. |
| `no_images` | The composition engine received no images. Supply at least one input. |
| `invalid_image_dimensions` | An input has a zero dimension. Replace it with a valid image. |
| `invalid_columns` | The engine requires positive grid columns. CLI argument validation normally reports this as `invalid_arguments`. |
| `image_size_mismatch` | An input's size differs from the `width`/`height` its placement in the spec expects. Re-run `--print-spec` against the current inputs. |
| `image_count_mismatch` | The render inputs do not match the planned composition. Replan; if persistent, report a bug. |
| `invalid_spec` | The `--spec` input is not valid JSON, is not a composition spec, has an unsupported `schema_version`, has an image without a `path`, or reads stdin twice. Re-create it with `--print-spec`. |
| `serialization_error` | The composition could not be serialized. Report a bug. |

Some engine-level codes are defensive and are not normally reachable through valid CLI arguments.

### Exit codes

- **0:** success, including help, version, schema, and spec inspection.
- **1:** input, composition, rendering, output-format, serialization, or file-write failure.
- **2:** argument parsing/validation failure.

## Composition rules

- Inputs are local PNG, JPEG, WebP, or GIF paths, in paint order. The format is detected from file content, so the extension does not matter. Animated GIFs and WebPs use their first frame. Repeated paths are allowed. EXIF orientation is applied before planning.
- `-` as an input reads one image from stdin (format detected from content); it may appear only once and is named `"-"` in the spec. Use `./-` for a file named `-`.
- Images are never enlarged, and are only resized or cropped when asked. Output dimensions are derived from inputs and placement.
- `--label TEXT` (once per input) adds a border and a label tab. The tab covers the image's top-left corner by default; `--label-position outside` puts it in a header strip above the image instead, so no content is hidden. `--label-color` overrides the automatic black/white text color, and `--font FILE` uses a TrueType/OpenType font instead of the bundled Inter SemiBold (for example for CJK labels). The spec records `label_position` and `label_color` when they are not the defaults, and `font` as an absolute path. An unreadable or invalid font is an `input_error` with `details.kind` `"font"`.
- `--fit-height PIXELS|min` (or `--fit-width`) shrinks taller (wider) images to the target, keeping their aspect ratio; `min` matches the shortest (narrowest) input, which gives a stack equal heights (widths). Images already smaller stay as they are and produce a `fit_not_applied` warning. Not used by split.
- `--fit cover|contain` lets split take inputs of different sizes: they are fitted into the smallest input width and height. `cover` scales and crops the overflow equally from both sides; `contain` scales to fit and pads with the background. The spec records it as each image's `fit`.
- `--max-width`/`--max-height` shrink the images just enough that the whole output fits. Spacing, borders, and labels keep their pixel size, freeform positions scale with the images, and the spec reports the factor as `scale` when it is below 1. If spacing, borders, and labels alone exceed the limit, the request fails with `canvas_cannot_fit`. All resizing combines into one resample per image.
- Grid/stack spacing sets both gaps and equal outer margins. Grid columns default to `2`; spacing defaults to `0`; alignment defaults to `start`.
- Alignment is within each grid cell or along a stack's cross-axis. Center alignment rounds offsets down to whole pixels.
- Freeform coordinates are nonnegative top-left pixel positions. Later inputs paint over earlier ones. Spacing/alignment do not affect freeform or split mode.
- Split inputs must have identical dimensions. The output shows parallel diagonal slices in input order. Divider width defaults to `0`.
- Background defaults to black. All outputs are opaque; input alpha blends onto the background. JPEG is lossy.
- Coordinates, spacing, divider, and columns are unsigned 32-bit integers; columns must be at least `1`. Large valid integers can still exceed composition limits.
- The RGBA canvas buffer is limited to 512 MiB. Input decoding and intermediate buffers use additional memory.
- Output defaults to `stitched.png`. The extension controls format, case-insensitively, unless `--format png|jpeg|webp` (alias `jpg`) overrides it. WebP output is lossless. Existing files are overwritten without a prompt, except that an output naming one of the inputs is refused unless `--force` is given. `--no-clobber` refuses any existing output file. Parent directories must already exist unless `--mkdir` is given. Spec inspection does not check the destination.
- `-o -` writes only the encoded image bytes to stdout and requires `--format`. It cannot be combined with `--json`, since stdout would no longer be a single JSON object; errors still go to stderr.
- `--jpeg-quality` is an integer from `1` to `100`, default `75`. It applies only to JPEG output and is ignored for PNG and WebP. Values outside the range are `invalid_arguments` errors.

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
