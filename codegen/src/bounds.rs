//! Bounds on types (core#98), checked while decoding and encoding.
//!
//! A bound is part of the type, so it's checked always, by every decoder and
//! encoder: it isn't one of the optional validators `validate_on_decode`
//! turns on. Every bounded position (a struct or error field, a function's
//! argument or its return) decodes through `comline_runtime::bounds`, which
//! refuses an oversized length before allocating, and encodes after checking
//! the whole value, so nothing is written for a value over its bound.
//!
//! - **Inline bounds** (`char[max 64]`, `u32[max 10]`, `char[2]`) are the
//!   native type, checked: `String`, `Vec<T>` (level 1 of the design's
//!   generated-code levels).
//! - **Bounded aliases** are level 1 too: `type Name = char[max 32]` stays
//!   `pub type Name = String`, and each use site checks the alias's bound
//!   ([`Validation::expand_bounds`]). The checked newtype of level 2 is left
//!   for later; the design allows plain aliases while consumers migrate.
//! - **A single-level maximum** goes straight through the runtime's field
//!   adapters (`bounds::decode::string::<_, N>` and the rest). Anything
//!   else (nested bounds, an optional bounded value, an exact string length)
//!   gets a `with` module of its own in the file's `__comline_bounds`, built
//!   from the runtime's `CharsSeed` and `ItemsSeed`.
//! - **`@max_depth`** puts the struct's `Serialize` and `Deserialize`
//!   through a [`Depth`](comline_runtime::bounds::Depth) count per thread.

use std::collections::BTreeMap;

use comline_core::schema::ir::frozen::types::{PrimitiveType, TypeRef};
use eyre::{eyre, Result};

const RT: &str = "comline_runtime::bounds";

/// The file's helper module.
pub const MODULE: &str = "__comline_bounds";

/// What a position checks, from its type with bounded aliases expanded.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Shape {
    /// Nothing here: the value's own type checks whatever it holds.
    Plain,
    CharsMax(u64),
    CharsExact(u64),
    /// A dynamic array, with a maximum or (holding bounded items) without.
    Items {
        of: Box<Shape>,
        max: Option<u64>,
    },
    Option(Box<Shape>),
}

fn shape(ty: &TypeRef) -> Result<Shape> {
    Ok(match ty {
        TypeRef::Array {
            element,
            length,
            max,
        } if **element == TypeRef::Primitive(PrimitiveType::Char) => match (length, max) {
            (Some(n), _) => Shape::CharsExact(*n),
            (None, Some(n)) => Shape::CharsMax(*n),
            (None, None) => Shape::Plain,
        },
        TypeRef::Array {
            element,
            length: Some(_),
            ..
        } => {
            if shape(element)? != Shape::Plain {
                return Err(eyre!(
                    "a fixed-length array of bounded values isn't supported by the Rust generator yet \
                     (core#98): bound the array (`T[max N]`) instead"
                ));
            }
            Shape::Plain
        }
        TypeRef::Array {
            element,
            length: None,
            max,
        } => {
            let of = shape(element)?;
            if max.is_none() && of == Shape::Plain {
                Shape::Plain
            } else {
                Shape::Items {
                    of: Box::new(of),
                    max: *max,
                }
            }
        }
        _ => Shape::Plain,
    })
}

fn limit(n: u64) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

impl Shape {
    /// A name for its `with` module, unique to the shape.
    fn key(&self) -> String {
        match self {
            Shape::Plain => "value".into(),
            Shape::CharsMax(n) => format!("chars_max_{n}"),
            Shape::CharsExact(n) => format!("chars_{n}"),
            Shape::Items { of, max: Some(n) } => format!("items_max_{n}_of_{}", of.key()),
            Shape::Items { of, max: None } => format!("items_of_{}", of.key()),
            Shape::Option(of) => format!("option_of_{}", of.key()),
        }
    }

    /// The Rust type, `T` standing for the plain leaf.
    fn rust(&self) -> String {
        match self {
            Shape::Plain => "T".into(),
            Shape::CharsMax(_) | Shape::CharsExact(_) => "$String".into(),
            Shape::Items { of, .. } => format!("$Vec<{}>", of.rust()),
            Shape::Option(of) => format!("$Option<{}>", of.rust()),
        }
    }

    fn has_plain_leaf(&self) -> bool {
        match self {
            Shape::Plain => true,
            Shape::CharsMax(_) | Shape::CharsExact(_) => false,
            Shape::Items { of, .. } | Shape::Option(of) => of.has_plain_leaf(),
        }
    }

    fn uses_exact(&self) -> bool {
        match self {
            Shape::CharsExact(_) => true,
            Shape::Items { of, .. } | Shape::Option(of) => of.uses_exact(),
            _ => false,
        }
    }

    /// The `DeserializeSeed` that decodes it.
    fn seed(&self) -> String {
        match self {
            Shape::Plain => "::core::marker::PhantomData::<T>".into(),
            Shape::CharsMax(n) => format!("{RT}::CharsSeed::<$String>::new({})", limit(*n)),
            Shape::CharsExact(n) => format!("Exact({})", limit(*n)),
            Shape::Items { of, max } => format!(
                "{RT}::ItemsSeed::<_, {}>::new({}, {})",
                self.rust(),
                of.seed(),
                max.map_or(u32::MAX, limit)
            ),
            Shape::Option(of) => format!("Opt({})", of.seed()),
        }
    }

    /// The statements that check `value` (a reference) before encoding.
    fn checks(&self, value: &str, indent: usize, out: &mut String) {
        let pad = " ".repeat(indent);
        match self {
            Shape::Plain => {}
            Shape::CharsMax(n) => {
                out.push_str(&format!(
                    "{pad}{RT}::check_chars({value}, {}).map_err({RT}::ser_error)?;\n",
                    limit(*n)
                ));
            }
            Shape::CharsExact(n) => {
                out.push_str(&format!(
                    "{pad}if {value}.chars().count() != {n} {{\n{pad}    return $Err(serde::ser::Error::custom(\"a string of exactly {n} characters\"));\n{pad}}}\n"
                ));
            }
            Shape::Items { of, max } => {
                if let Some(n) = max {
                    out.push_str(&format!(
                        "{pad}{RT}::check_items({value}.len(), {}).map_err({RT}::ser_error)?;\n",
                        limit(*n)
                    ));
                }
                if **of != Shape::Plain {
                    let item = format!("item{indent}");
                    out.push_str(&format!("{pad}for {item} in {value}.iter() {{\n"));
                    of.checks(&item, indent + 4, out);
                    out.push_str(&format!("{pad}}}\n"));
                }
            }
            Shape::Option(of) => {
                let inner = format!("inner{indent}");
                out.push_str(&format!("{pad}if let $Some({inner}) = {value} {{\n"));
                of.checks(&inner, indent + 4, out);
                out.push_str(&format!("{pad}}}\n"));
            }
        }
    }

    /// The `with` module for it.
    fn module(&self) -> String {
        let (generics_de, generics_ser) = if self.has_plain_leaf() {
            (", T: serde::Deserialize<'de>", ", T: serde::Serialize")
        } else {
            ("", "")
        };
        let rust = self.rust();
        let mut checks = String::new();
        self.checks("value", 12, &mut checks);
        format!(
            "    pub mod {key} {{\n        \
                 use super::*;\n\n        \
                 pub fn deserialize<'de, D: serde::Deserializer<'de>{generics_de}>(d: D) -> $Result<{rust}, D::Error> {{\n            \
                     serde::de::DeserializeSeed::deserialize({seed}, d)\n        \
                 }}\n\n        \
                 pub fn serialize<S: serde::Serializer{generics_ser}>(value: &{rust}, s: S) -> $Result<S::Ok, S::Error> {{\n\
                 {checks}            \
                     serde::Serialize::serialize(value, s)\n        \
                 }}\n    \
             }}\n",
            key = self.key(),
            seed = self.seed(),
        )
    }
}

/// The `with` modules a file needs, collected while its positions render.
#[derive(Default)]
pub struct Helpers {
    modules: BTreeMap<String, String>,
    exact: bool,
    opt: bool,
}

impl Helpers {
    /// The serde attribute for a position of type `expanded` (bounded
    /// aliases already expanded), `optional` for an optional field; empty
    /// when it has no bound to check.
    pub fn attribute(
        &mut self,
        expanded: &TypeRef,
        optional: bool,
        indent: &str,
    ) -> Result<String> {
        let inner = shape(expanded)?;
        if inner == Shape::Plain {
            return Ok(String::new());
        }
        // A maximum one level deep goes straight through the runtime's own
        // field adapters.
        if !optional {
            match &inner {
                Shape::CharsMax(n) => {
                    let n = limit(*n);
                    return Ok(format!(
                        "{indent}#[serde(\n{indent}    deserialize_with = \"{RT}::decode::string::<_, {n}>\",\n{indent}    serialize_with = \"{RT}::encode::string::<_, {n}>\"\n{indent})]\n"
                    ));
                }
                Shape::Items { of, max: Some(n) } if **of == Shape::Plain => {
                    let n = limit(*n);
                    return Ok(format!(
                        "{indent}#[serde(\n{indent}    deserialize_with = \"{RT}::decode::vec::<_, _, {n}>\",\n{indent}    serialize_with = \"{RT}::encode::vec::<_, _, {n}>\"\n{indent})]\n"
                    ));
                }
                _ => {}
            }
        }
        let shape = if optional {
            Shape::Option(Box::new(inner))
        } else {
            inner
        };
        self.exact |= shape.uses_exact();
        self.opt |= optional;
        let key = shape.key();
        self.modules
            .entry(key.clone())
            .or_insert_with(|| shape.module());
        // An optional field may be left out of a JSON object, as one
        // without `with` may.
        let default = if optional {
            format!("{indent}#[serde(default)]\n")
        } else {
            String::new()
        };
        Ok(format!(
            "{default}{indent}#[serde(with = \"{MODULE}::{key}\")]\n"
        ))
    }

    /// The file's `__comline_bounds` module; empty when nothing used it.
    pub fn render(&self) -> String {
        if self.modules.is_empty() {
            return String::new();
        }
        let mut s = format!(
            "/// How this file's bounded values decode and encode (core#98): each\n\
             /// module is a `#[serde(with)]` for one shape of bounds.\n\
             #[doc(hidden)]\n\
             #[allow(dead_code, unused_imports)]\n\
             mod {MODULE} {{\n"
        );
        if self.exact {
            s.push_str(&format!(
                "    /// A string of exactly `.0` characters: more is refused before\n    \
                 /// it's allocated, as a maximum is.\n    \
                 #[derive(Clone, Copy)]\n    \
                 pub struct Exact(pub u32);\n\n    \
                 impl<'de> serde::de::DeserializeSeed<'de> for Exact {{\n        \
                     type Value = $String;\n\n        \
                     fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> $Result<$String, D::Error> {{\n            \
                         let s: $String = serde::de::DeserializeSeed::deserialize({RT}::CharsSeed::<$String>::new(self.0), d)?;\n            \
                         if s.chars().count() != self.0 as usize {{\n                \
                             return $Err(serde::de::Error::custom(::core::format_args!(\"a string of exactly {{}} characters\", self.0)));\n            \
                         }}\n            \
                         $Ok(s)\n        \
                     }}\n    \
                 }}\n\n"
            ));
        }
        if self.opt {
            s.push_str(
                "    /// An optional value, decoded through the seed `.0` when present.\n    \
                 #[derive(Clone, Copy)]\n    \
                 pub struct Opt<S>(pub S);\n\n    \
                 impl<'de, S: serde::de::DeserializeSeed<'de>> serde::de::DeserializeSeed<'de> for Opt<S> {\n        \
                     type Value = $Option<S::Value>;\n\n        \
                     fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> $Result<Self::Value, D::Error> {\n            \
                         d.deserialize_option(self)\n        \
                     }\n    \
                 }\n\n    \
                 impl<'de, S: serde::de::DeserializeSeed<'de>> serde::de::Visitor<'de> for Opt<S> {\n        \
                     type Value = $Option<S::Value>;\n\n        \
                     fn expecting(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {\n            \
                         f.write_str(\"an optional value\")\n        \
                     }\n\n        \
                     fn visit_none<E: serde::de::Error>(self) -> $Result<Self::Value, E> {\n            \
                         $Ok($None)\n        \
                     }\n\n        \
                     fn visit_unit<E: serde::de::Error>(self) -> $Result<Self::Value, E> {\n            \
                         $Ok($None)\n        \
                     }\n\n        \
                     fn visit_some<D: serde::Deserializer<'de>>(self, d: D) -> $Result<Self::Value, D::Error> {\n            \
                         self.0.deserialize(d).map($Some)\n        \
                     }\n    \
                 }\n\n",
            );
        }
        let modules: Vec<&str> = self.modules.values().map(String::as_str).collect();
        s.push_str(&modules.join("\n"));
        s.push_str("}\n\n");
        s
    }
}

/// For a struct with `@max_depth = D`: the public struct is declared without
/// serde's derive, which goes on `__<Name>Depth` as a `remote` definition
/// instead, and the struct's own `Serialize` and `Deserialize` count the
/// nesting around it. `validate` is the check a decoded value runs when
/// `validate_on_decode` is on (`Validate::validate(&value)` or the like).
pub fn depth_impls(name: &str, max: u64, lines: &str, validate: Option<&str>) -> String {
    let def = format!("__{name}Depth");
    let max = limit(max);
    let validated = match validate {
        Some(call) => format!(
            "        let value = r?;\n        \
                 {call}.map_err(serde::de::Error::custom)?;\n        \
                 $Ok(value)\n"
        ),
        None => "        r\n".to_string(),
    };
    format!(
        "/// [`{name}`]'s fields as serde reads and writes them; its own impls\n\
         /// count how deep it nests, at most {max} levels (`@max_depth`).\n\
         #[doc(hidden)]\n\
         #[allow(dead_code, non_camel_case_types)]\n\
         #[derive($Serialize, $Deserialize)]\n\
         #[serde(remote = \"{name}\")]\n\
         struct {def} {{\n{lines}}}\n\n\
         impl {def} {{\n    \
             fn depth<R>(f: impl FnOnce(&mut {RT}::Depth) -> R) -> R {{\n        \
                 ::std::thread_local! {{\n            \
                     static DEPTH: ::core::cell::Cell<{RT}::Depth> =\n                \
                         const {{ ::core::cell::Cell::new({RT}::Depth::new({max})) }};\n        \
                 }}\n        \
                 DEPTH.with(|cell| {{\n            \
                     let mut depth = cell.get();\n            \
                     let r = f(&mut depth);\n            \
                     cell.set(depth);\n            \
                     r\n        \
                 }})\n    \
             }}\n\
         }}\n\n\
         impl $Serialize for {name} {{\n    \
             fn serialize<S: serde::Serializer>(&self, s: S) -> $Result<S::Ok, S::Error> {{\n        \
                 {def}::depth(|d| d.enter()).map_err({RT}::ser_error)?;\n        \
                 let r = {def}::serialize(self, s);\n        \
                 {def}::depth(|d| d.leave());\n        \
                 r\n    \
             }}\n\
         }}\n\n\
         impl<'de> $Deserialize<'de> for {name} {{\n    \
             fn deserialize<D: serde::Deserializer<'de>>(d: D) -> $Result<Self, D::Error> {{\n        \
                 {def}::depth(|d| d.enter()).map_err({RT}::de_error)?;\n        \
                 let r = {def}::deserialize(d);\n        \
                 {def}::depth(|d| d.leave());\n\
             {validated}    \
             }}\n\
         }}\n\n"
    )
}

/// `lines` without its serde attributes, for a struct serde doesn't derive.
pub fn without_serde_attributes(lines: &str) -> String {
    let mut out = String::new();
    let mut in_attr = false;
    for line in lines.lines() {
        let trimmed = line.trim_start();
        if in_attr {
            if trimmed == ")]" {
                in_attr = false;
            }
            continue;
        }
        if trimmed.starts_with("#[serde(") {
            in_attr = !trimmed.ends_with(")]");
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn array(element: TypeRef, length: Option<u64>, max: Option<u64>) -> TypeRef {
        TypeRef::Array {
            element: Box::new(element),
            length,
            max,
        }
    }

    const CHAR: TypeRef = TypeRef::Primitive(PrimitiveType::Char);

    #[test]
    fn single_level_maximums_use_the_runtime_adapters() {
        let mut h = Helpers::default();
        let a = h
            .attribute(&array(CHAR, None, Some(32)), false, "    ")
            .unwrap();
        assert!(a.contains("bounds::decode::string::<_, 32>"), "{a}");
        let a = h
            .attribute(
                &array(TypeRef::Primitive(PrimitiveType::U32), None, Some(10)),
                false,
                "    ",
            )
            .unwrap();
        assert!(a.contains("bounds::encode::vec::<_, _, 10>"), "{a}");
        assert_eq!(h.render(), "");
    }

    #[test]
    fn nested_optional_and_exact_bounds_get_a_module() {
        let mut h = Helpers::default();
        let nested = array(array(CHAR, None, Some(16)), None, Some(8));
        assert!(h
            .attribute(&nested, false, "")
            .unwrap()
            .contains("__comline_bounds::items_max_8_of_chars_max_16"));
        assert!(h
            .attribute(&array(CHAR, None, Some(254)), true, "")
            .unwrap()
            .contains("option_of_chars_max_254"));
        assert!(h
            .attribute(&array(CHAR, Some(2), None), false, "")
            .unwrap()
            .contains("chars_2"));
        let out = h.render();
        assert!(
            out.contains("pub struct Exact") && out.contains("pub struct Opt<S>"),
            "{out}"
        );
        assert!(out.contains("ItemsSeed::<_, $Vec<$String>>::new(comline_runtime::bounds::CharsSeed::<$String>::new(16), 8)"), "{out}");
    }

    #[test]
    fn plain_types_have_no_attribute() {
        let mut h = Helpers::default();
        assert_eq!(
            h.attribute(&TypeRef::Primitive(PrimitiveType::String), true, "")
                .unwrap(),
            ""
        );
        assert_eq!(
            h.attribute(
                &array(TypeRef::Primitive(PrimitiveType::U8), Some(4), None),
                false,
                ""
            )
            .unwrap(),
            ""
        );
        assert!(h
            .attribute(&array(array(CHAR, Some(2), None), Some(3), None), false, "")
            .is_err());
    }

    #[test]
    fn serde_attributes_are_stripped_whole() {
        let lines = "    #[serde(\n        deserialize_with = \"x\",\n    )]\n    pub a: String,\n    #[serde(rename = \"b\")]\n    pub b: u8,\n";
        assert_eq!(
            without_serde_attributes(lines),
            "    pub a: String,\n    pub b: u8,\n"
        );
    }
}
