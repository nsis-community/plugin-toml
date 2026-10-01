//! Read and write TOML files from NSIS scripts.
//!
//! Documents live in the DLL under a name the script picks, so a file is parsed
//! once and queried many times. Comments, spacing and key order survive a
//! round trip through `Save`, because the document is a `toml_edit` one.
//!
//! ```nsis
//! toml::Load "cfg" "$INSTDIR\config.toml"
//! toml::Get "cfg" "server.port"
//! Pop $0
//! toml::SetInt "cfg" "server.port" 8080
//! toml::Save "cfg" "$INSTDIR\config.toml"
//! toml::Free "cfg"
//! ```
//!
//! Every failure sets the error flag and pushes nothing; `LastError` says what
//! went wrong. The one exception is a value too long for the installer's
//! `NSIS_MAX_STRLEN`: it is pushed truncated, and the flag is set as well.

#![warn(missing_docs)]
#![allow(
	non_snake_case,
	reason = "the crate name is the DLL name, and scripts call `TOML::Load`"
)]

mod doc;

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use nsis_plugin::{Nsis, Result, nsis_fn, nsis_plugin, nsis_unload};
use toml_edit::{Datetime, DocumentMut, Value};

use doc::{New, Res};

nsis_plugin!(std);

/// An open document.
struct Handle {
	doc: DocumentMut,
	/// Whether the file started with a byte-order mark, so `Save` can keep it.
	bom: bool,
	/// Whether the file used CRLF line endings. `toml_edit` writes LF only.
	crlf: bool,
}

/// Open documents by name.
///
/// A global behind a lo
/// ck rather than a thread-local: the exehead runs
/// sections on its own install thread (`install_thread` in
/// `Source/exehead/ui.c`) while `.onInit` and page callbacks run on the UI
/// thread, and one document may be used from both.
static DOCS: Mutex<BTreeMap<String, Handle>> = Mutex::new(BTreeMap::new());

/// What `LastError` reports.
static LAST_ERROR: Mutex<String> = Mutex::new(String::new());

/// The installer calls one export at a time, so the lock is never contended;
/// and with `panic = "abort"` nothing can poison it.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
	m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Runs an export body, recording a failure for `LastError`.
fn run(nsis: &mut Nsis, body: impl FnOnce(&mut Nsis) -> Res<()>) -> Result<()> {
	body(nsis).map_err(|message| {
		*lock(&LAST_ERROR) = message;
		nsis_plugin::Error::Failed
	})
}

fn pop(nsis: &mut Nsis) -> Res<String> {
	nsis.stack.pop().map_err(|e| e.to_string())
}

/// Pushes a value. Truncation still pushes the clipped value, as
/// `Stack::push` does, and is reported as a failure.
fn push(nsis: &mut Nsis, value: &str) -> Res<()> {
	nsis.stack.push(value).map_err(|e| match e {
		nsis_plugin::Error::Truncated => format!(
			"value of {} characters truncated to NSIS_MAX_STRLEN - 1 ({})",
			value.chars().count(),
			nsis.stack.max_len()
		),
		other => other.to_string(),
	})
}

fn with_doc<T>(name: &str, f: impl FnOnce(&mut Handle) -> Res<T>) -> Res<T> {
	let mut docs = lock(&DOCS);
	let handle = docs
		.get_mut(name)
		.ok_or_else(|| format!("no document named `{name}`; Load or New it first"))?;
	f(handle)
}

fn open(nsis: &mut Nsis, name: String, handle: Handle) {
	// Fails before NSIS 2.42 and under the test harness. Documents then live
	// until the process exits, which is all that unloading would achieve.
	let _ = nsis.register_callback(unload);
	lock(&DOCS).insert(name, handle);
}

/// Pops `name, path` and resolves the path, for the read-only exports.
fn read<T>(nsis: &mut Nsis, f: impl FnOnce(&toml_edit::Item, &[doc::Seg]) -> Res<T>) -> Res<T> {
	let name = pop(nsis)?;
	let path = pop(nsis)?;
	let segs = doc::parse_path(&path)?;
	with_doc(&name, |h| f(doc::lookup(&h.doc, &segs)?, &segs))
}

/// Pops `name, path, value` and writes what `make` builds from the value.
fn setter(nsis: &mut Nsis, make: impl FnOnce(String) -> Res<Value>) -> Result<()> {
	run(nsis, |nsis| {
		let name = pop(nsis)?;
		let path = pop(nsis)?;
		let value = pop(nsis)?;
		let segs = doc::parse_path(&path)?;
		let value = make(value)?;
		with_doc(&name, |h| doc::set(&mut h.doc, &segs, New::Value(value)))
	})
}

/// Pops `name, path` and writes an empty container.
fn container(nsis: &mut Nsis, new: New) -> Result<()> {
	run(nsis, |nsis| {
		let name = pop(nsis)?;
		let path = pop(nsis)?;
		let segs = doc::parse_path(&path)?;
		with_doc(&name, |h| doc::set(&mut h.doc, &segs, new))
	})
}

nsis_unload! {
	/// Frees every open document when the installer unloads the plug-in.
	fn unload() {
		lock(&DOCS).clear();
	}
}

nsis_fn! {
	/// `name file` — parses a UTF-8 file. Replaces any document of that name.
	fn Load(nsis: &mut Nsis) -> Result<()> {
		run(nsis, |nsis| {
			let name = pop(nsis)?;
			let file = pop(nsis)?;
			let text = std::fs::read_to_string(&file)
				.map_err(|e| format!("could not read `{file}`: {e}"))?;
			let (text, bom) = match text.strip_prefix('\u{feff}') {
				Some(rest) => (rest, true),
				None => (text.as_str(), false),
			};
			let crlf = text.contains("\r\n");
			let doc = text
				.parse::<DocumentMut>()
				.map_err(|e| format!("could not parse `{file}`: {e}"))?;
			open(nsis, name, Handle { doc, bom, crlf });
			Ok(())
		})
	}

	/// `name` — an empty document, for creating a file.
	fn New(nsis: &mut Nsis) -> Result<()> {
		run(nsis, |nsis| {
			let name = pop(nsis)?;
			let handle = Handle {
				doc: DocumentMut::new(),
				bom: false,
				crlf: false,
			};
			open(nsis, name, handle);
			Ok(())
		})
	}

	/// `name file` — writes the document as UTF-8.
	fn Save(nsis: &mut Nsis) -> Result<()> {
		run(nsis, |nsis| {
			let name = pop(nsis)?;
			let file = pop(nsis)?;
			let text = with_doc(&name, |h| {
				let mut text = h.doc.to_string();
				if h.crlf {
					// Normalise first, so a CRLF kept inside a multiline string
					// does not become CR CR LF.
					text = text.replace("\r\n", "\n").replace('\n', "\r\n");
				}
				Ok(format!("{}{text}", if h.bom { "\u{feff}" } else { "" }))
			})?;
			std::fs::write(&file, text).map_err(|e| format!("could not write `{file}`: {e}"))
		})
	}

	/// `name` — forgets the document.
	fn Free(nsis: &mut Nsis) -> Result<()> {
		run(nsis, |nsis| {
			let name = pop(nsis)?;
			match lock(&DOCS).remove(&name) {
				Some(_) => Ok(()),
				None => Err(format!("no document named `{name}`")),
			}
		})
	}

	/// `name path` → the value: scalars plain, datetimes as RFC 3339, arrays
	/// and tables as inline TOML.
	fn Get(nsis: &mut Nsis) -> Result<()> {
		run(nsis, |nsis| {
			let value = read(nsis, |item, _| Ok(doc::render(item)))?;
			push(nsis, &value)
		})
	}

	/// `name path` → `string`, `integer`, `float`, `boolean`, `datetime`,
	/// `array` or `table`. A missing path is an error, so this doubles as "has".
	fn Type(nsis: &mut Nsis) -> Result<()> {
		run(nsis, |nsis| {
			let name = read(nsis, |item, _| Ok(doc::type_name(item)))?;
			push(nsis, name)
		})
	}

	/// `name path` → array length or number of table keys.
	fn Count(nsis: &mut Nsis) -> Result<()> {
		run(nsis, |nsis| {
			let n = read(nsis, doc::count)?;
			nsis.stack.push_int(n as isize).map_err(|e| e.to_string())
		})
	}

	/// `name path i` → the value, then the key on top. For arrays the key is
	/// the index.
	fn EntryAt(nsis: &mut Nsis) -> Result<()> {
		run(nsis, |nsis| {
			let name = pop(nsis)?;
			let path = pop(nsis)?;
			let i = nsis.stack.pop_int().map_err(|e| e.to_string())?;
			let i = usize::try_from(i).map_err(|_| format!("index {i} is negative"))?;
			let segs = doc::parse_path(&path)?;
			let (key, value) = with_doc(&name, |h| {
				let (key, item) = doc::entry(doc::lookup(&h.doc, &segs)?, &segs, i)?;
				Ok((key, doc::render(item)))
			})?;
			// Push both even if the value truncates, or the script's two Pops
			// would take someone else's entry.
			let value = push(nsis, &value);
			push(nsis, &key).and(value)
		})
	}

	/// `name path value` — a string. The plug-in does the quoting.
	fn SetString(nsis: &mut Nsis) -> Result<()> {
		setter(nsis, |s| Ok(s.into()))
	}

	/// `name path value` — a 64-bit integer. TOML syntax (`0o755`, `0b101`,
	/// `1_000`, `+5`) is written as given; anything else is read the NSIS way:
	/// `0x` hex and leading-zero octal work, and garbage is `0`.
	fn SetInt(nsis: &mut Nsis) -> Result<()> {
		setter(nsis, |s| match doc::literal(&s) {
			Some(v @ Value::Integer(_)) => Ok(v),
			_ => Ok(doc::str_to_i64(&s).into()),
		})
	}

	/// `name path value` — a float: `3.14`, `1e6`, `1_000.5`, `inf`, `nan`.
	fn SetFloat(nsis: &mut Nsis) -> Result<()> {
		setter(nsis, |s| match doc::literal(&s) {
			Some(v @ Value::Float(_)) => Ok(v),
			_ => s
				.parse::<f64>()
				.map(Value::from)
				.map_err(|_| format!("`{s}` is not a float")),
		})
	}

	/// `name path value` — `1`/`true` or `0`/`false`.
	fn SetBool(nsis: &mut Nsis) -> Result<()> {
		setter(nsis, |s| match s.as_str() {
			"1" | "true" => Ok(true.into()),
			"0" | "false" => Ok(false.into()),
			_ => Err(format!("`{s}` is not a boolean; use 1, 0, true or false")),
		})
	}

	/// `name path value` — an RFC 3339 date, time or date-time, in any of the
	/// four forms TOML has.
	fn SetDate(nsis: &mut Nsis) -> Result<()> {
		setter(nsis, |s| {
			s.parse::<Datetime>()
				.map(Value::from)
				.map_err(|e| format!("`{s}` is not a TOML date or time: {e}"))
		})
	}

	/// `name path value` — any value in TOML syntax, as `Get` returns arrays
	/// and tables: `[1, 2]`, `{ a = "x" }`, `'literal'`, `0o755`.
	fn SetRaw(nsis: &mut Nsis) -> Result<()> {
		setter(nsis, |s| {
			doc::literal(&s).ok_or_else(|| format!("`{s}` is not a TOML value"))
		})
	}

	/// `name path` — an empty array. Setting `path[0]` creates one anyway;
	/// this is for an array that should stay empty.
	fn SetArray(nsis: &mut Nsis) -> Result<()> {
		container(nsis, New::Array)
	}

	/// `name path` — an empty table. Setting `path.key` creates one anyway;
	/// this is for a table that should stay empty.
	fn SetTable(nsis: &mut Nsis) -> Result<()> {
		container(nsis, New::Table)
	}

	/// `name path` — removes a key or an array element.
	fn Remove(nsis: &mut Nsis) -> Result<()> {
		run(nsis, |nsis| {
			let name = pop(nsis)?;
			let path = pop(nsis)?;
			let segs = doc::parse_path(&path)?;
			with_doc(&name, |h| doc::remove(&mut h.doc, &segs))
		})
	}

	/// → the message from the last failure.
	fn LastError(nsis: &mut Nsis) -> Result<()> {
		let message = lock(&LAST_ERROR).clone();
		nsis.stack.push(&message)
	}
}

#[cfg(test)]
mod tests {
	use std::path::PathBuf;

	use nsis_plugin::raw::Export;
	use nsis_plugin::testing::TestInstaller;

	use super::*;

	/// The documents and `LastError` are process-wide, and tests run in
	/// parallel.
	static SERIAL: Mutex<()> = Mutex::new(());

	/// Calls an export with script-order arguments; true if it failed.
	fn call(inst: &mut TestInstaller, export: Export, args: &[&str]) -> bool {
		for arg in args.iter().rev() {
			inst.push(arg);
		}
		inst.clear_error();
		inst.call(export);
		inst.error()
	}

	fn get(inst: &mut TestInstaller, name: &str, path: &str) -> Option<String> {
		if call(inst, Get, &[name, path]) {
			None
		} else {
			inst.pop()
		}
	}

	fn last_error(inst: &mut TestInstaller) -> String {
		call(inst, LastError, &[]);
		inst.pop().unwrap()
	}

	fn temp(name: &str) -> PathBuf {
		std::env::temp_dir().join(format!("nsis-toml-{}-{name}", std::process::id()))
	}

	fn load(inst: &mut TestInstaller, name: &str, text: &str) {
		let file = temp(name);
		std::fs::write(&file, text).unwrap();
		assert!(!call(inst, Load, &[name, file.to_str().unwrap()]));
	}

	#[test]
	fn load_get_save_round_trips() {
		let _serial = lock(&SERIAL);
		let mut inst = TestInstaller::stock();
		let text = "\u{feff}# settings\n[server]\nhost = \"old\" # keep\nport = 80\n";
		load(&mut inst, "rt", text);
		assert_eq!(get(&mut inst, "rt", "server.port").as_deref(), Some("80"));

		assert!(!call(&mut inst, SetString, &["rt", "server.host", "new"]));
		let out = temp("rt-out");
		assert!(!call(&mut inst, Save, &["rt", out.to_str().unwrap()]));
		assert_eq!(
			std::fs::read_to_string(&out).unwrap(),
			"\u{feff}# settings\n[server]\nhost = \"new\" # keep\nport = 80\n"
		);
		assert!(!call(&mut inst, Free, &["rt"]));
		assert!(call(&mut inst, Free, &["rt"]));
		assert!(inst.is_empty());
	}

	#[test]
	fn crlf_survives_save() {
		let _serial = lock(&SERIAL);
		let mut inst = TestInstaller::stock();
		load(&mut inst, "crlf", "a = 1\r\ns = '''\r\nx\r\n'''\r\n");
		assert!(!call(&mut inst, SetInt, &["crlf", "b", "2"]));
		let out = temp("crlf-out");
		assert!(!call(&mut inst, Save, &["crlf", out.to_str().unwrap()]));
		assert_eq!(
			std::fs::read_to_string(&out).unwrap(),
			"a = 1\r\ns = '''\r\nx\r\n'''\r\nb = 2\r\n"
		);
		call(&mut inst, Free, &["crlf"]);
	}

	#[test]
	fn load_failures_are_reported() {
		let _serial = lock(&SERIAL);
		let mut inst = TestInstaller::stock();
		let missing = temp("does-not-exist");
		assert!(call(&mut inst, Load, &["x", missing.to_str().unwrap()]));
		assert!(last_error(&mut inst).contains("could not read"));

		let bad = temp("bad");
		std::fs::write(&bad, "a = \n").unwrap();
		assert!(call(&mut inst, Load, &["x", bad.to_str().unwrap()]));
		assert!(last_error(&mut inst).contains("could not parse"));

		// Failures push nothing.
		assert!(inst.is_empty());
		assert!(call(&mut inst, Get, &["never-loaded", "a"]));
		assert!(last_error(&mut inst).contains("never-loaded"));
		assert!(call(&mut inst, Load, &[]));
		assert!(inst.is_empty());
	}

	#[test]
	fn new_then_set_every_type() {
		let _serial = lock(&SERIAL);
		let mut inst = TestInstaller::stock();
		assert!(!call(&mut inst, New, &["types"]));
		for (export, value, path, want_type, want) in [
			(
				SetString as Export,
				"hi \"there\"",
				"s",
				"string",
				"hi \"there\"",
			),
			(SetString, "grüße 🦀", "u", "string", "grüße 🦀"),
			(SetInt, "0x7fffffffff", "i", "integer", "549755813887"),
			(SetInt, "abc", "garbage", "integer", "0"),
			(SetInt, "0o755", "oct", "integer", "493"),
			(SetInt, "0755", "nsis_oct", "integer", "493"),
			(SetInt, "0b101", "bin", "integer", "5"),
			(SetInt, "1_000", "under", "integer", "1000"),
			(SetInt, "+5", "plus", "integer", "5"),
			(SetFloat, "1_000.5", "f2", "float", "1000.5"),
			(SetRaw, "[1, 'a']", "raw", "array", "[1, 'a']"),
			(
				SetRaw,
				"{ x = { y = 1 } }",
				"rawt",
				"table",
				"{ x = { y = 1 } }",
			),
			(SetFloat, "1e6", "f", "float", "1000000.0"),
			(SetBool, "1", "b", "boolean", "true"),
			(SetBool, "false", "b2", "boolean", "false"),
			(SetDate, "1979-05-27", "d", "datetime", "1979-05-27"),
			(SetDate, "07:32:00", "t", "datetime", "07:32:00"),
		] {
			assert!(!call(&mut inst, export, &["types", path, value]), "{path}");
			assert!(!call(&mut inst, Type, &["types", path]));
			assert_eq!(inst.pop().as_deref(), Some(want_type), "{path}");
			assert_eq!(
				get(&mut inst, "types", path).as_deref(),
				Some(want),
				"{path}"
			);
		}
		assert!(!call(&mut inst, SetArray, &["types", "arr"]));
		assert!(!call(&mut inst, SetTable, &["types", "tbl"]));
		assert_eq!(get(&mut inst, "types", "arr").as_deref(), Some("[]"));
		assert_eq!(get(&mut inst, "types", "tbl").as_deref(), Some("{}"));
		call(&mut inst, Free, &["types"]);
	}

	#[test]
	fn setters_reject_bad_input() {
		let _serial = lock(&SERIAL);
		let mut inst = TestInstaller::stock();
		call(&mut inst, New, &["bad"]);
		for (export, value) in [
			(SetFloat as Export, "1.2.3"),
			(SetBool, "yes"),
			(SetBool, "TRUE"),
			(SetDate, "yesterday"),
			(SetRaw, "bare words"),
		] {
			assert!(call(&mut inst, export, &["bad", "k", value]), "{value}");
		}
		assert!(call(&mut inst, SetString, &["bad", "a[", "x"]));
		assert!(last_error(&mut inst).contains("unclosed"));
		assert!(call(&mut inst, Type, &["bad", "k"]), "nothing was written");
		assert!(inst.is_empty());
		call(&mut inst, Free, &["bad"]);
	}

	#[test]
	fn count_and_entry_at_iterate() {
		let _serial = lock(&SERIAL);
		let mut inst = TestInstaller::stock();
		load(&mut inst, "it", "a = 1\nb = \"two\"\nlist = [10, 20]\n");

		assert!(!call(&mut inst, Count, &["it", ""]));
		assert_eq!(inst.pop().as_deref(), Some("3"));
		assert!(!call(&mut inst, EntryAt, &["it", "", "1"]));
		assert_eq!(inst.pop().as_deref(), Some("b"), "key on top");
		assert_eq!(inst.pop().as_deref(), Some("two"));

		assert!(!call(&mut inst, EntryAt, &["it", "list", "1"]));
		assert_eq!(inst.stack(), ["1", "20"]);
		inst.pop();
		inst.pop();

		assert!(call(&mut inst, EntryAt, &["it", "list", "2"]));
		assert!(call(&mut inst, EntryAt, &["it", "list", "-1"]));
		assert!(call(&mut inst, Count, &["it", "a"]));
		assert!(inst.is_empty());
		call(&mut inst, Free, &["it"]);
	}

	#[test]
	fn remove_deletes() {
		let _serial = lock(&SERIAL);
		let mut inst = TestInstaller::stock();
		load(&mut inst, "rm", "a = 1\nb = 2\n");
		assert!(!call(&mut inst, Remove, &["rm", "a"]));
		assert!(call(&mut inst, Type, &["rm", "a"]));
		assert!(call(&mut inst, Remove, &["rm", "a"]));
		call(&mut inst, Free, &["rm"]);
	}

	/// `string_size - 1` characters fit; `string_size` are pushed truncated
	/// with the flag set.
	#[test]
	fn values_are_bounded_by_string_size() {
		let _serial = lock(&SERIAL);
		for size in [64, 1024, 8192] {
			let fits = "x".repeat(size - 1);
			let too_long = "y".repeat(size);
			let text = format!("fits = \"{fits}\"\ntoo_long = \"{too_long}\"\n");
			// Loaded through a stock installer, because the temp path alone
			// does not fit in 64 characters. The document is not bound by
			// either size.
			load(&mut TestInstaller::stock(), "big", &text);
			let mut inst = TestInstaller::new(size);

			assert_eq!(get(&mut inst, "big", "fits"), Some(fits));

			assert!(call(&mut inst, Get, &["big", "too_long"]));
			assert_eq!(inst.pop().unwrap().len(), size - 1);
			assert!(last_error(&mut inst).contains("truncated"));

			// EntryAt still pushes both halves.
			assert!(call(&mut inst, EntryAt, &["big", "", "1"]));
			assert_eq!(inst.pop().as_deref(), Some("too_long"));
			assert_eq!(inst.pop().unwrap().len(), size - 1);
			assert!(inst.is_empty());
			call(&mut inst, Free, &["big"]);
		}
	}
}
