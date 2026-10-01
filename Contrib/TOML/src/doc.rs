//! Everything that does not touch the installer: key paths, lookup, rendering
//! and the setters. Plain functions, so they are tested directly.

use toml_edit::{Array, ArrayOfTables, DocumentMut, InlineTable, Item, Key, Table, Value};

/// A failure, as the message `LastError` will report.
pub type Res<T> = Result<T, String>;

/// One step of a key path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Seg {
	/// A table key, already unquoted.
	Key(String),
	/// An array index.
	Index(usize),
}

/// What a setter writes at the end of its path.
pub enum New {
	/// A scalar.
	Value(Value),
	/// An empty table.
	Table,
	/// An empty array.
	Array,
}

/// Parses `servers[1].host`, `env."app.mode"` and the like. The empty path is
/// the document root.
///
/// Key runs go through `toml_edit`'s own dotted-key parser, so quoting and
/// escaping are exactly TOML's. Only the `[n]` suffixes are split off here, and
/// only outside quotes.
pub fn parse_path(path: &str) -> Res<Vec<Seg>> {
	let mut segs = Vec::new();
	if path.is_empty() {
		return Ok(segs);
	}

	let bytes = path.as_bytes();
	let mut run_start = 0;
	// A key is required at the start and after a `.`; after `]` it is optional.
	let mut key_needed = true;
	let mut quote = None;
	let mut i = 0;
	while i < bytes.len() {
		// Byte-wise is safe: every delimiter is ASCII, and UTF-8 continuation
		// bytes never are.
		match (quote, bytes[i]) {
			(Some(b'"'), b'\\') => i += 1,
			(Some(q), b) if b == q => quote = None,
			(Some(_), _) => {}
			(None, b @ (b'"' | b'\'')) => quote = Some(b),
			(None, b'[') => {
				let run = &path[run_start..i];
				if key_needed || !run.is_empty() {
					push_keys(&mut segs, run, path)?;
				}
				let close = path[i..]
					.find(']')
					.map(|c| i + c)
					.ok_or_else(|| format!("unclosed `[` in path `{path}`"))?;
				let digits = &path[i + 1..close];
				if digits.is_empty() || !digits.bytes().all(|d| d.is_ascii_digit()) {
					return Err(format!(
						"`[{digits}]` is not an array index in path `{path}`"
					));
				}
				let index = digits
					.parse()
					.map_err(|_| format!("index `{digits}` is too large in path `{path}`"))?;
				segs.push(Seg::Index(index));

				i = close + 1;
				key_needed = match bytes.get(i) {
					None | Some(b'[') => false,
					Some(b'.') => {
						i += 1;
						true
					}
					Some(_) => {
						return Err(format!("expected `.` or `[` after `]` in path `{path}`"));
					}
				};
				run_start = i;
				continue;
			}
			_ => {}
		}
		i += 1;
	}
	if quote.is_some() {
		return Err(format!("unterminated quote in path `{path}`"));
	}
	let run = &path[run_start..];
	if key_needed || !run.is_empty() {
		push_keys(&mut segs, run, path)?;
	}
	Ok(segs)
}

fn push_keys(segs: &mut Vec<Seg>, run: &str, path: &str) -> Res<()> {
	let keys = Key::parse(run).map_err(|e| format!("bad key `{run}` in path `{path}`: {e}"))?;
	segs.extend(keys.into_iter().map(|k| Seg::Key(k.get().to_owned())));
	Ok(())
}

/// A path as TOML would write it, for messages.
fn show(path: &[Seg]) -> String {
	let mut out = String::new();
	for seg in path {
		match seg {
			Seg::Key(k) => {
				if !out.is_empty() {
					out.push('.');
				}
				out.push_str(&Key::new(k.as_str()).display_repr());
			}
			Seg::Index(n) => out.push_str(&format!("[{n}]")),
		}
	}
	if out.is_empty() {
		"the root".into()
	} else {
		format!("`{out}`")
	}
}

fn not_found(path: &[Seg]) -> String {
	format!("{} not found", show(path))
}

/// Resolves a path without creating anything.
pub fn lookup<'a>(doc: &'a DocumentMut, path: &[Seg]) -> Res<&'a Item> {
	let mut item = doc.as_item();
	for (i, seg) in path.iter().enumerate() {
		let next = match seg {
			Seg::Key(k) => item.as_table_like().and_then(|t| t.get(k)),
			Seg::Index(n) => item.get(*n),
		};
		item = next
			.filter(|item| !item.is_none())
			.ok_or_else(|| not_found(&path[..=i]))?;
	}
	Ok(item)
}

/// As [`lookup`], mutably. `Item::get_mut` with a key would insert a
/// placeholder on a miss, so tables go through `TableLike` instead.
fn lookup_mut<'a>(doc: &'a mut DocumentMut, path: &[Seg]) -> Res<&'a mut Item> {
	let mut item = doc.as_item_mut();
	for (i, seg) in path.iter().enumerate() {
		let next = match seg {
			Seg::Key(k) => item.as_table_like_mut().and_then(|t| t.get_mut(k)),
			Seg::Index(n) => item.get_mut(*n),
		};
		item = next.ok_or_else(|| not_found(&path[..=i]))?;
	}
	Ok(item)
}

/// The TOML type name `Type` reports.
pub fn type_name(item: &Item) -> &'static str {
	match item {
		Item::Value(Value::String(_)) => "string",
		Item::Value(Value::Integer(_)) => "integer",
		Item::Value(Value::Float(_)) => "float",
		Item::Value(Value::Boolean(_)) => "boolean",
		Item::Value(Value::Datetime(_)) => "datetime",
		Item::Value(Value::Array(_)) | Item::ArrayOfTables(_) => "array",
		Item::Value(Value::InlineTable(_)) | Item::Table(_) => "table",
		Item::None => "none",
	}
}

fn array_len(item: &Item) -> Option<usize> {
	match item {
		Item::Value(Value::Array(a)) => Some(a.len()),
		Item::ArrayOfTables(a) => Some(a.len()),
		_ => None,
	}
}

fn not_a(item: &Item, path: &[Seg], wanted: &str) -> String {
	format!(
		"{} is {}, not {wanted}",
		show(path),
		article(type_name(item))
	)
}

fn article(name: &str) -> String {
	match name {
		"array" | "integer" => format!("an {name}"),
		_ => format!("a {name}"),
	}
}

/// Array length or number of table keys.
pub fn count(item: &Item, path: &[Seg]) -> Res<usize> {
	array_len(item)
		.or_else(|| item.as_table_like().map(|t| t.len()))
		.ok_or_else(|| not_a(item, path, "an array or a table"))
}

/// The `i`th entry: its key (the index, for arrays) and its value.
pub fn entry<'a>(item: &'a Item, path: &[Seg], i: usize) -> Res<(String, &'a Item)> {
	let found = if array_len(item).is_some() {
		item.get(i).map(|v| (i.to_string(), v))
	} else if let Some(table) = item.as_table_like() {
		table.iter().nth(i).map(|(k, v)| (k.to_owned(), v))
	} else {
		return Err(not_a(item, path, "an array or a table"));
	};
	found.ok_or_else(|| format!("index {i} is out of range for {}", show(path)))
}

/// A value as a script sees it: scalars plain, containers as inline TOML.
pub fn render(item: &Item) -> String {
	match item {
		Item::Value(Value::String(s)) => s.value().clone(),
		Item::Value(Value::Integer(n)) => n.value().to_string(),
		Item::Value(Value::Float(f)) => render_float(*f.value()),
		Item::Value(Value::Boolean(b)) => b.value().to_string(),
		Item::Value(Value::Datetime(d)) => d.value().to_string(),
		other => match other.clone().into_value() {
			Ok(mut value) => {
				inline(&mut value);
				value.to_string()
			}
			Err(_) => String::new(),
		},
	}
}

/// `Debug` rather than `Display`, which spells `1e300` out in full. TOML
/// writes NaN as `nan`.
fn render_float(f: f64) -> String {
	if f.is_nan() {
		"nan".into()
	} else {
		format!("{f:?}")
	}
}

/// Strips comments and line breaks so a container fits on one line.
fn inline(value: &mut Value) {
	match value {
		Value::Array(a) => {
			a.iter_mut().for_each(inline);
			a.fmt();
		}
		Value::InlineTable(t) => {
			t.iter_mut().for_each(|(_, v)| inline(v));
			t.fmt();
		}
		_ => {}
	}
	value.decor_mut().clear();
}

/// Parses a value written in TOML syntax, such as `0o755`, `1_000.5` or
/// `[1, { a = 2 }]`, dropping the whitespace around it.
pub fn literal(s: &str) -> Option<Value> {
	let mut value = s.trim().parse::<Value>().ok()?;
	value.decor_mut().clear();
	Some(value)
}

/// `nsishelper_str_to_ptr` from `Contrib/ExDLL/pluginapi.c`, widened to 64
/// bits.
///
/// `nsis_plugin::int::str_to_ptr` returns `isize`, which is 32 bits in an x86
/// installer and would silently wrap TOML's 64-bit integers. Everything else is
/// the same: `0x` hex, leading-zero octal, a sign only on decimal, stop at the
/// first unrecognised character, wrap on overflow, garbage is `0`. The window
/// is `popintptr`'s `TCHAR buf[128]`.
pub fn str_to_i64(s: &str) -> i64 {
	let s: Vec<u32> = s.chars().take(127).map(u32::from).collect();
	let at = |i: usize| s.get(i).copied().unwrap_or(0);
	let digit = |c: u32| char::from_u32(c).and_then(|c| c.to_digit(16));
	let zero = u32::from(b'0');
	let mut v: i64 = 0;

	if at(0) == zero && (at(1) == u32::from(b'x') || at(1) == u32::from(b'X')) {
		let mut i = 2;
		while let Some(d) = digit(at(i)) {
			v = (v << 4).wrapping_add(i64::from(d));
			i += 1;
		}
		return v;
	}

	if at(0) == zero && (zero..=u32::from(b'7')).contains(&at(1)) {
		let mut i = 1;
		while (zero..=u32::from(b'7')).contains(&at(i)) {
			v = (v << 3).wrapping_add(i64::from(at(i) - zero));
			i += 1;
		}
		return v;
	}

	let negative = at(0) == u32::from(b'-');
	let mut i = usize::from(negative);
	while (zero..=u32::from(b'9')).contains(&at(i)) {
		v = v.wrapping_mul(10).wrapping_add(i64::from(at(i) - zero));
		i += 1;
	}
	if negative { v.wrapping_neg() } else { v }
}

/// Writes `new` at `path`, creating missing containers on the way.
///
/// On failure the document is untouched.
pub fn set(doc: &mut DocumentMut, path: &[Seg], new: New) -> Res<()> {
	if path.is_empty() {
		return Err("the document root cannot be replaced".into());
	}
	// ponytail: clone so a path that fails halfway leaves nothing behind. Fine
	// for config-sized documents; an undo log if they ever get large.
	let mut work = doc.clone();
	let parent = walk(work.as_item_mut(), path, &new)?;
	put(parent, path, new)?;
	*doc = work;
	Ok(())
}

/// Walks to the parent of the last segment, creating what is missing.
fn walk<'a>(mut item: &'a mut Item, path: &[Seg], new: &New) -> Res<&'a mut Item> {
	for i in 0..path.len() - 1 {
		let here = &path[..=i];
		let next = &path[i + 1];
		// Whether an array created for `next` holds tables.
		let of_tables = match path.get(i + 2) {
			Some(Seg::Key(_)) => true,
			Some(Seg::Index(_)) => false,
			None => matches!(new, New::Table),
		};

		item = match &path[i] {
			Seg::Key(k) => {
				let inline = item.is_inline_table();
				let kind = not_a(item, here, "a table");
				let Some(table) = item.as_table_like_mut() else {
					return Err(kind);
				};
				if !table.contains_key(k) {
					table.insert(k, container(next, inline, of_tables, &path[..=i + 1])?);
				}
				table
					.get_mut(k)
					.ok_or_else(|| format!("{} could not be created", show(here)))?
			}
			Seg::Index(n) => {
				let n = *n;
				let Some(len) = array_len(item) else {
					return Err(not_a(item, &path[..i], "an array"));
				};
				if n > len {
					return Err(out_of_range(n, len, &path[..i]));
				}
				if n == len {
					match item {
						Item::ArrayOfTables(aot) if matches!(next, Seg::Key(_)) => {
							aot.push(Table::new());
						}
						Item::ArrayOfTables(_) => {
							return Err(format!("{} holds tables", show(&path[..i])));
						}
						Item::Value(Value::Array(a)) => {
							if let Item::Value(v) =
								container(next, true, of_tables, &path[..=i + 1])?
							{
								a.push(v);
							}
						}
						_ => {}
					}
				}
				item.get_mut(n)
					.ok_or_else(|| format!("{} could not be created", show(here)))?
			}
		};
	}
	Ok(item)
}

/// An empty container of the kind `next` needs.
///
/// Inside an inline table or an array, containers must be inline too. Elsewhere
/// a table becomes an implicit `[a.b]` header, which renders only once it holds
/// a value, and an array of tables becomes `[[a]]`.
fn container(next: &Seg, inline: bool, of_tables: bool, path: &[Seg]) -> Res<Item> {
	Ok(match next {
		Seg::Key(_) if inline => Item::Value(InlineTable::new().into()),
		Seg::Key(_) => {
			let mut table = Table::new();
			table.set_implicit(true);
			Item::Table(table)
		}
		Seg::Index(0) if of_tables && !inline => Item::ArrayOfTables(ArrayOfTables::new()),
		Seg::Index(0) => Item::Value(Array::new().into()),
		Seg::Index(n) => return Err(out_of_range(*n, 0, &path[..path.len() - 1])),
	})
}

fn out_of_range(n: usize, len: usize, path: &[Seg]) -> String {
	format!(
		"index {n} is out of range for {}, which has {len} element(s); use {len} to append",
		show(path)
	)
}

/// Writes `new` into `parent` at the last segment of `path`.
fn put(parent: &mut Item, path: &[Seg], new: New) -> Res<()> {
	let (last, parent_path) = path.split_last().expect("set rejects the empty path");
	match last {
		Seg::Key(k) => {
			let inline = parent.is_inline_table();
			let kind = not_a(parent, parent_path, "a table");
			let Some(table) = parent.as_table_like_mut() else {
				return Err(kind);
			};
			let mut item = match new {
				New::Value(v) => Item::Value(v),
				New::Table if inline => Item::Value(InlineTable::new().into()),
				New::Table => Item::Table(Table::new()),
				New::Array => Item::Value(Array::new().into()),
			};
			// Keep the old value's spacing and trailing comment, so a replaced
			// line differs only in its value.
			if let (Some(Item::Value(old)), Item::Value(v)) = (table.get(k), &mut item) {
				*v.decor_mut() = old.decor().clone();
			}
			table.insert(k, item);
		}
		Seg::Index(n) => {
			let n = *n;
			let Some(len) = array_len(parent) else {
				return Err(not_a(parent, parent_path, "an array"));
			};
			if n > len {
				return Err(out_of_range(n, len, parent_path));
			}
			// A non-table in an array of tables: the array has to become
			// inline, since `[[a]]` can only hold tables.
			if let Item::ArrayOfTables(aot) = parent {
				if matches!(new, New::Table) {
					if n == len {
						aot.push(Table::new());
					} else {
						aot.replace(n, Table::new());
					}
					return Ok(());
				}
				*parent = Item::Value(std::mem::take(aot).into_array().into());
			}
			let Item::Value(Value::Array(a)) = parent else {
				return Err(not_a(parent, parent_path, "an array"));
			};
			let v = match new {
				New::Value(v) => v,
				New::Table => InlineTable::new().into(),
				New::Array => Array::new().into(),
			};
			if n == len {
				a.push(v)
			} else {
				a.replace(n, v);
			}
		}
	}
	Ok(())
}

/// Removes the entry at `path`.
pub fn remove(doc: &mut DocumentMut, path: &[Seg]) -> Res<()> {
	let Some((last, parent_path)) = path.split_last() else {
		return Err("the document root cannot be removed".into());
	};
	let parent = lookup_mut(doc, parent_path)?;
	let removed = match (last, parent) {
		(Seg::Key(k), parent) => parent
			.as_table_like_mut()
			.and_then(|t| t.remove(k))
			.is_some(),
		(Seg::Index(n), Item::ArrayOfTables(a)) if *n < a.len() => {
			a.remove(*n);
			true
		}
		(Seg::Index(n), Item::Value(Value::Array(a))) if *n < a.len() => {
			a.remove(*n);
			true
		}
		_ => false,
	};
	if removed {
		Ok(())
	} else {
		Err(not_found(path))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn key(k: &str) -> Seg {
		Seg::Key(k.into())
	}

	fn path(p: &str) -> Vec<Seg> {
		parse_path(p).unwrap()
	}

	fn doc(text: &str) -> DocumentMut {
		text.parse().unwrap()
	}

	fn set_str(d: &mut DocumentMut, p: &str, v: &str) -> Res<()> {
		set(d, &path(p), New::Value(v.into()))
	}

	#[test]
	fn parses_paths() {
		assert_eq!(path(""), []);
		assert_eq!(path("a.b"), [key("a"), key("b")]);
		assert_eq!(
			path("servers[1].host"),
			[key("servers"), Seg::Index(1), key("host")]
		);
		assert_eq!(path("a[0][2]"), [key("a"), Seg::Index(0), Seg::Index(2)]);
		assert_eq!(path(r#"env."app.mode""#), [key("env"), key("app.mode")]);
		assert_eq!(path("'a[0]'.b"), [key("a[0]"), key("b")]);
		assert_eq!(path(r#""q\"[".x"#), [key("q\"["), key("x")]);
		assert_eq!(path(r#""grüße".x"#), [key("grüße"), key("x")]);
	}

	#[test]
	fn rejects_bad_paths() {
		for bad in [
			"a.", ".a", "[0]", "a[", "a[]", "a[x]", "a[-1]", "a[0]b", "a[0].", "a..b", "\"open",
			"a b",
		] {
			assert!(parse_path(bad).is_err(), "`{bad}` should not parse");
		}
	}

	#[test]
	fn looks_up_and_renders() {
		let d = doc(r#"
title = "x" # comment
n = 0x10
f = 1e300
nan = nan
yes = true
when = 1979-05-27T07:32:00Z
list = [
	1, # one
	2,
]
[t]
a = 1
[[servers]]
host = "a"
[[servers]]
host = "b"
"#);
		let get = |p: &str| render(lookup(&d, &path(p)).unwrap());
		assert_eq!(get("title"), "x");
		assert_eq!(get("n"), "16");
		assert_eq!(get("f"), "1e300");
		assert_eq!(get("nan"), "nan");
		assert_eq!(get("yes"), "true");
		assert_eq!(get("when"), "1979-05-27T07:32:00Z");
		assert_eq!(get("list"), "[1, 2]");
		assert_eq!(get("t"), "{ a = 1 }");
		assert_eq!(get("servers"), r#"[{ host = "a" }, { host = "b" }]"#);
		assert_eq!(get("servers[1].host"), "b");
		assert_eq!(get("list[0]"), "1");

		assert_eq!(type_name(lookup(&d, &path("servers")).unwrap()), "array");
		assert_eq!(type_name(lookup(&d, &path("t")).unwrap()), "table");
		assert_eq!(count(d.as_item(), &[]).unwrap(), 9);
		assert!(lookup(&d, &path("missing")).is_err());
		assert!(lookup(&d, &path("list[2]")).is_err());
		assert!(lookup(&d, &path("title.x")).is_err());
	}

	#[test]
	fn entries_are_key_and_value() {
		let d = doc("a = 1\nb = [\"x\", \"y\"]\n");
		let (k, v) = entry(d.as_item(), &[], 1).unwrap();
		assert_eq!((k.as_str(), render(v).as_str()), ("b", r#"["x", "y"]"#));
		let b = lookup(&d, &path("b")).unwrap();
		let (k, v) = entry(b, &path("b"), 1).unwrap();
		assert_eq!((k.as_str(), render(v).as_str()), ("1", "y"));
		assert!(entry(b, &path("b"), 2).is_err());
	}

	#[test]
	fn replacing_a_value_changes_only_that_value() {
		let text = "# header\n[server]\nhost = \"old\"   # keep me\nport = 80\n";
		let mut d = doc(text);
		set_str(&mut d, "server.host", "new").unwrap();
		assert_eq!(
			d.to_string(),
			"# header\n[server]\nhost = \"new\"   # keep me\nport = 80\n"
		);
	}

	#[test]
	fn creates_containers_on_demand() {
		let mut d = DocumentMut::new();
		set_str(&mut d, "a.b.c", "x").unwrap();
		set(&mut d, &path("ports[0]"), New::Value(80.into())).unwrap();
		set(&mut d, &path("ports[1]"), New::Value(443.into())).unwrap();
		set_str(&mut d, "servers[0].host", "a").unwrap();
		set_str(&mut d, "servers[1].host", "b").unwrap();
		set_str(&mut d, "inline[0].k", "v").unwrap();
		set_str(&mut d, "inline[0].k", "w").unwrap();
		let text = d.to_string();
		assert!(text.contains("ports = [80, 443]"), "{text}");
		assert!(text.contains("[a.b]\nc = \"x\""), "{text}");
		assert!(text.contains("[[servers]]\nhost = \"a\""), "{text}");
		// Re-parses to the same structure.
		let again = doc(&text);
		assert_eq!(
			render(lookup(&again, &path("servers[1].host")).unwrap()),
			"b"
		);
		assert_eq!(render(lookup(&again, &path("inline[0].k")).unwrap()), "w");
	}

	#[test]
	fn a_table_in_an_inline_array_stays_inline() {
		let mut d = doc("pts = [{ x = 1 }]\n");
		set(&mut d, &path("pts[1].x"), New::Value(2.into())).unwrap();
		assert_eq!(d.to_string(), "pts = [{ x = 1 }, { x = 2 }]\n");
	}

	#[test]
	fn out_of_range_writes_nothing() {
		let mut d = doc("list = [1]\n");
		assert!(set_str(&mut d, "list[2]", "x").is_err());
		assert!(set_str(&mut d, "new[3]", "x").is_err());
		assert!(set_str(&mut d, "a.b[1].c", "x").is_err());
		assert_eq!(d.to_string(), "list = [1]\n");
	}

	#[test]
	fn a_scalar_is_not_a_container() {
		let mut d = doc("a = 1\n");
		assert!(set_str(&mut d, "a.b", "x").is_err());
		assert!(set_str(&mut d, "a[0]", "x").is_err());
		assert!(set_str(&mut d, "", "x").is_err());
	}

	#[test]
	fn replacing_changes_the_type() {
		let mut d = doc("a = \"1\"\n[[t]]\nx = 1\n");
		set(&mut d, &path("a"), New::Value(1.into())).unwrap();
		set(&mut d, &path("t[0]"), New::Value(5.into())).unwrap();
		let again = doc(&d.to_string());
		assert_eq!(type_name(lookup(&again, &path("a")).unwrap()), "integer");
		assert_eq!(render(lookup(&again, &path("t")).unwrap()), "[5]");
	}

	#[test]
	fn empty_containers() {
		let mut d = DocumentMut::new();
		set(&mut d, &path("t"), New::Table).unwrap();
		set(&mut d, &path("a"), New::Array).unwrap();
		set(&mut d, &path("aot[0]"), New::Table).unwrap();
		let again = doc(&d.to_string());
		assert_eq!(render(lookup(&again, &path("a")).unwrap()), "[]");
		assert_eq!(type_name(lookup(&again, &path("t")).unwrap()), "table");
		assert_eq!(
			count(lookup(&again, &path("aot")).unwrap(), &[]).unwrap(),
			1
		);
	}

	#[test]
	fn removes() {
		let mut d = doc("a = 1\nb = [1, 2]\n[[t]]\n[[t]]\n");
		remove(&mut d, &path("a")).unwrap();
		remove(&mut d, &path("b[0]")).unwrap();
		remove(&mut d, &path("t[1]")).unwrap();
		assert!(remove(&mut d, &path("a")).is_err());
		assert!(remove(&mut d, &path("b[1]")).is_err());
		assert!(remove(&mut d, &path("zz.y")).is_err());
		assert_eq!(render(lookup(&d, &path("b")).unwrap()), "[2]");
		assert_eq!(count(lookup(&d, &path("t")).unwrap(), &[]).unwrap(), 1);
	}

	#[test]
	fn literals_are_toml() {
		let lit = |s: &str| literal(s).map(|v| v.to_string());
		assert_eq!(lit(" 0o755 ").as_deref(), Some("0o755"));
		assert_eq!(lit("[1, { a = 2 }]").as_deref(), Some("[1, { a = 2 }]"));
		assert!(literal("0755").is_none(), "leading zeros are not TOML");
		assert!(literal("bare words").is_none());
	}

	#[test]
	fn integers_follow_nsis() {
		assert_eq!(str_to_i64("42"), 42);
		assert_eq!(str_to_i64("-42"), -42);
		assert_eq!(str_to_i64("0x7fffffffff"), 0x7f_ffff_ffff);
		assert_eq!(str_to_i64("0X1F"), 31);
		assert_eq!(str_to_i64("0755"), 0o755);
		assert_eq!(str_to_i64("08"), 8);
		assert_eq!(str_to_i64("9abc"), 9);
		assert_eq!(str_to_i64("abc"), 0);
		assert_eq!(str_to_i64(""), 0);
		assert_eq!(str_to_i64("-"), 0);
		assert_eq!(str_to_i64("9223372036854775807"), i64::MAX);
		assert_eq!(str_to_i64("9223372036854775808"), i64::MIN);
	}
}
