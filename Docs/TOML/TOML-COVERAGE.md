# TOML type coverage

How much of TOML 1.0 the plug-in can read and write, and where it falls short.

## Summary

The plug-in can read every TOML 1.0 type, because parsing is done by `toml_edit`. Writing gaps 1–4 below are fixed; only the choice of formatting style (#5) remains, and `SetRaw` covers most of it.

## Reading

| TOML type | `Type` reports | `Get` returns |
|---|---|---|
| string (basic, literal, multiline) | `string` | the raw value |
| integer (decimal, `0x`, `0o`, `0b`, `1_000`) | `integer` | decimal i64 |
| float (`inf`, `nan`, exponents) | `float` | `1e300`, `inf`, `nan` |
| boolean | `boolean` | `true` / `false` |
| offset date-time, local date-time, local date, local time | `datetime` | RFC 3339 |
| array, array of tables | `array` | inline TOML |
| table, inline table | `table` | inline TOML |

See `render` and `type_name` in [src/doc.rs](src/doc.rs).

### Limits

- **The four date/time kinds share one type name.** All of them report `datetime`. To tell them apart, a script has to inspect the returned string.
- **A string containing `\u0000` gets cut off at the NUL.** NSIS strings are NUL-terminated, so the script sees only the part before the NUL. No error is reported.

## Writing

### 1. ~~Saving turns CRLF line endings into LF~~ — fixed

`toml_edit` writes `\n` everywhere. `Load` now records whether the file used CRLF, as it does for the BOM, and `Save` converts the line endings back. Line endings are normalised to LF before that conversion, so a CRLF already kept inside a multiline string doesn't come out as CR CR LF.

### 2. ~~`SetInt` gets TOML integer syntax silently wrong~~ — fixed

`SetInt` now tries TOML syntax first and keeps it as written (`0o755` stays `0o755` in the file). Anything that isn't a TOML integer falls back to the NSIS rules, so `0755` (octal), `0x1F` and garbage (`0`) behave as before.

| Input | Before | Now |
|---|---|---|
| `0o755` | `0` | `493` |
| `0b101` | `0` | `5` |
| `1_000` | `1` | `1000` |
| `+5` | `0` | `5` |

### 3. ~~`SetFloat` rejects underscores~~ — fixed

Fixed the same way: TOML syntax first (`1_000.5`), then Rust's `f64` parser.

### 4. ~~Containers can't be written back~~ — fixed

`SetRaw "name" "path" "<toml value>"` writes any value in TOML syntax, including the inline form that `Get` returns for arrays and tables. That makes reading and writing symmetric, and it lets a script write heterogeneous arrays and nested inline tables in one call. Note that `Get` returns a scalar string unquoted, so passing a plain `Get` result for a string back to `SetRaw` fails. Use `SetString` for strings.

### 5. Some formatting can't be chosen

- `SetString` always writes a basic `"..."` string. Use `SetRaw` for a literal (`'...'`) or multiline string.
- New tables become `[header]` tables, except inside inline containers, where they become `{ }`. `SetRaw "{ ... }"` writes an inline table instead; there is no way to force a `[header]` table inside an inline container, and TOML doesn't allow one there anyway.

## Outside the plug-in

`Get` returns 64-bit integers exactly as strings. `IntOp` and `IntCmp` in NSIS work on 32 bits, so a script doing arithmetic on values above 2³¹−1 will wrap them. This is worth stating in the user docs.
