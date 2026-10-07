# TOML plug-in

Read and write TOML 1.0 files from an NSIS script. Comments, spacing and key order survive a round trip, so a script can edit a config file without reformatting it.

## Quick start

```nsis
!include "TOML.nsh"                      ; only needed for ${TomlForEach}

TOML::Load "cfg" "$INSTDIR\config.toml"  ; parse once, under that name
TOML::Get "cfg" "server.port"            ; arguments are name, path
Pop $0                                   ; $0 = "80"
TOML::SetInt "cfg" "server.port" 8080
TOML::Save "cfg" "$INSTDIR\config.toml"
TOML::Free "cfg"
```

The name is any string you like, so a `!define` keeps it in one place. A `Var` works too (`StrCpy $Doc "cfg"`, then `$Doc` in each call) if the name is only known at run time. The examples below write `"cfg"` out for brevity.

To create a file from scratch, use `TOML::New "cfg"` instead of `Load`.

## Paths

A path addresses a value inside the document:

| Path              | Means                                 |
| ----------------- | ------------------------------------- |
| `title`           | top-level key                         |
| `server.port`     | key in a table                        |
| `plugins[1].name` | key in the second element of an array |
| `env."app.mode"`  | quoted key containing a dot           |
| `""`              | the document root                     |

Setting a path that doesn't exist creates it, including parent tables. `plugins[2]` appends if the array has two elements.

## Commands

| Command                | Arguments         | Result                                                                                                                                    |
| ---------------------- | ----------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| `Load`                 | `name file`       | Parses a UTF-8 file. Replaces any document of that name.                                                                                  |
| `New`                  | `name`            | An empty document.                                                                                                                        |
| `Save`                 | `name file`       | Writes the document as UTF-8. Keeps the BOM and CRLF line endings if the file had them.                                                   |
| `Free`                 | `name`            | Forgets the document. Open documents are also freed when the installer exits.                                                             |
| `Get`                  | `name path`       | Pushes the value.                                                                                                                         |
| `Type`                 | `name path`       | Pushes `string`, `integer`, `float`, `boolean`, `datetime`, `array` or `table`. A missing path is an error, so this doubles as "has key". |
| `Count`                | `name path`       | Pushes the array length or the number of table keys.                                                                                      |
| `EntryAt`              | `name path index` | Pushes the value, then the key (for an array, the index). `Pop` the key first.                                                            |
| `SetString`            | `name path value` | A string. The plug-in does the quoting.                                                                                                   |
| `SetInt`               | `name path value` | A 64-bit integer.                                                                                                                         |
| `SetFloat`             | `name path value` | A float: `3.14`, `1e6`, `1_000.5`, `inf`, `nan`.                                                                                          |
| `SetBool`              | `name path value` | `1`/`true` or `0`/`false`.                                                                                                                |
| `SetDate`              | `name path value` | An RFC 3339 date, time or date-time.                                                                                                      |
| `SetRaw`               | `name path value` | Any value in TOML syntax: `[1, 2]`, `{ a = "x" }`, `'literal'`.                                                                           |
| `SetArray`, `SetTable` | `name path`       | An empty array or table.                                                                                                                  |
| `Remove`               | `name path`       | Removes a key or array element.                                                                                                           |
| `LastError`            |                   | Pushes the message from the last failure.                                                                                                 |

## Error handling

Every failure sets the NSIS error flag and pushes nothing. Clear the flag first, then check it:

```nsis
ClearErrors

TOML::Load "cfg" "$INSTDIR\config.toml"

${If} ${Errors}
	TOML::LastError
	Pop $0
	MessageBox MB_OK "Could not read config: $0"
	Abort
${EndIf}
```

The one exception is a value longer than the installer's `NSIS_MAX_STRLEN`: it is pushed truncated and the flag is set as well.

## Recipes

**Check whether a key exists**

```nsis
ClearErrors

TOML::Type "cfg" "server.tls"

${IfNot} ${Errors}
	Pop $0   ; the type name
${EndIf}
```

**Loop over a table or array** (`TOML.nsh`)

```nsis
${TomlForEach} "cfg" "server" $0 $1
	DetailPrint "$0 = $1"
${TomlNext}
```

Each pass sets the key and the value; for an array the key is the index. `${TomlBreak}` leaves the loop and `${Continue}` skips to the next entry. A path that doesn't exist, or isn't an array or table, runs zero passes. Loops nest.

**Write a container in one call**

```nsis
TOML::SetRaw "cfg" "server.ports" "[80, 443]"
TOML::SetRaw "cfg" "server.limits" "{ cpu = 2, mem = '1G' }"
```

**Write a string that needs a particular style**

```nsis
TOML::SetRaw "cfg" "paths.root" "'C:\Program Files\App'"   ; literal string, no escaping
```

## Reading: what `Get` returns

| TOML type                                                 | `Type` reports | `Get` returns         |
| --------------------------------------------------------- | -------------- | --------------------- |
| string (basic, literal, multiline)                        | `string`       | the raw value         |
| integer (decimal, `0x`, `0o`, `0b`, `1_000`)              | `integer`      | decimal i64           |
| float (`inf`, `nan`, exponents)                           | `float`        | `1e300`, `inf`, `nan` |
| boolean                                                   | `boolean`      | `true` / `false`      |
| offset date-time, local date-time, local date, local time | `datetime`     | RFC 3339              |
| array, array of tables                                    | `array`        | inline TOML           |
| table, inline table                                       | `table`        | inline TOML           |

See `render` and `type_name` in [doc.rs](../../Contrib/TOML/src/doc.rs).

## Caveats

- **The four date/time kinds share one type name.** All of them report `datetime`. To tell them apart, a script has to inspect the returned string.
- **A string containing `\u0000` gets cut off at the NUL.** NSIS strings are NUL-terminated, so the script sees only the part before the NUL. No error is reported.
- **`SetInt` accepts TOML syntax and NSIS syntax.** `0o755`, `0b101`, `1_000` and `+5` are written as given. Anything else falls back to the NSIS rules: `0755` is octal, `0x1F` is hex, and garbage becomes `0`.
- **`Get` on a container returns inline TOML, and a scalar string comes back unquoted.** Passing an array or table result to `SetRaw` works. Passing a plain string result fails, because it isn't valid TOML. Use `SetString` for strings.
- **Some formatting can't be chosen.**
  - `SetString` always writes a basic `"..."` string. Use `SetRaw` for a literal (`'...'`) or multiline string.
  - New tables become `[header]` tables, except inside inline containers, where they become `{ }`. `SetRaw "{ ... }"` writes an inline table instead. There is no way to force a `[header]` table inside an inline container, and TOML doesn't allow one there anyway.
- **NSIS integer math is 32-bit.** `Get` returns 64-bit integers exactly as strings, but `IntOp` and `IntCmp` wrap values above 2³¹−1.
- **Value length is capped by `NSIS_MAX_STRLEN`**, 1024 in a stock makensis build.
