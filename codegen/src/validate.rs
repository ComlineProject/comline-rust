//! A value's `@validators`, lowered into Rust.
//!
//! [`comline_codegen::validation`] works out *what* runs on each field - the
//! alias chain's checks and the field's own, every property bound. This
//! module writes it down: an `impl comline_runtime::validation::Validate`
//! per struct, error and params struct that has anything to check, and,
//! when `validate_on_decode` asks for it, the shadow type that makes decoding
//! run those checks.
//!
//! Every check is total. Lengths and integers widen to `i128` (no `usize` or
//! `u64` value overflows it), floats to `f64`, and nothing indexes - so a
//! failing condition is a failed check, never a panic.

use comline_codegen::validation::{Check, Plan, ValidateOnDecode, Validation, Value};
use comline_core::schema::ir::frozen::types::{
    CompareOp, Comparison, Condition, LogicalOp, Operand, PrimitiveType, TypeRef,
};
use comline_core::schema::ir::frozen::unit::{FrozenArgument, FrozenUnit};
use eyre::{eyre, Result};

use crate::message;

const RT: &str = "comline_runtime::validation";

/// One member of a validated type and what to check on it.
pub struct Member<'a> {
    pub name: String,
    pub optional: bool,
    pub plan: Plan<'a>,
}

/// The plans for a struct or error's fields.
pub fn field_members<'a>(
    model: &Validation<'a>,
    namespace: &str,
    owner: &str,
    fields: &'a [FrozenUnit],
) -> Result<Vec<Member<'a>>> {
    fields
        .iter()
        .filter_map(|field| match field {
            FrozenUnit::Field { name, ty, optional, parameters, .. } => Some((name, ty, optional, parameters)),
            _ => None,
        })
        .map(|(name, ty, optional, parameters)| {
            let at = format!("schema '{namespace}', '{owner}.{name}'");
            Ok(Member {
                name: name.clone(),
                optional: *optional,
                plan: model.plan(namespace, ty, parameters, &at)?,
            })
        })
        .collect()
}

/// The plans for a function's arguments, as its params struct's fields.
pub fn argument_members<'a>(
    model: &Validation<'a>,
    namespace: &str,
    owner: &str,
    arguments: &[FrozenArgument],
) -> Result<Vec<Member<'a>>> {
    arguments
        .iter()
        .map(|arg| {
            let at = format!("schema '{namespace}', '{owner}' argument '{}'", arg.name);
            Ok(Member {
                name: arg.name.clone(),
                optional: false,
                plan: model.plan(namespace, &arg.ty, &[], &at)?,
            })
        })
        .collect()
}

/// Whether any member has something to check.
pub fn any_checks(members: &[Member]) -> bool {
    members.iter().any(|m| !m.plan.is_empty())
}

/// `impl Validate for {ty}`, or nothing when no member has a check.
/// `lifetime` is `"<'a>"` for a borrowing params struct, else empty.
pub fn validate_impl(ty: &str, lifetime: &str, members: &[Member]) -> Result<String> {
    if !any_checks(members) {
        return Ok(String::new());
    }
    let mut out = Lines::default();
    out.line(0, &format!("impl{lifetime} {RT}::Validate for {ty}{lifetime} {{"));
    // A check need not mention `value` (or a member only recurse), and with
    // nothing to check `out` and `first` go unused.
    out.line(1, "#[allow(unused_variables)]");
    out.line(
        1,
        &format!(
            "fn validate_into(&self, path: &str, out: &mut Vec<{RT}::ValidationError>, first: bool) {{"
        ),
    );
    for member in members.iter().filter(|m| !m.plan.is_empty()) {
        out.line(2, "{");
        out.line(3, &format!("let path = {RT}::field_path(path, {:?});", member.name));
        if member.optional {
            out.line(3, &format!("if let Some(value) = &self.{} {{", member.name));
            plan(&mut out, 4, &member.plan)?;
            out.line(3, "}");
        } else {
            out.line(3, &format!("let value = &self.{};", member.name));
            plan(&mut out, 3, &member.plan)?;
        }
        out.line(2, "}");
    }
    out.line(1, "}");
    out.line(0, "}");
    out.0.push('\n');
    Ok(out.0)
}

/// For `validate_on_decode`: the attribute that routes decoding of `ty`
/// through its unchecked shadow, or nothing when decoding stays as it is.
pub fn try_from_attribute(ty: &str, on_decode: ValidateOnDecode, members: &[Member]) -> String {
    if on_decode == ValidateOnDecode::Off || !any_checks(members) {
        return String::new();
    }
    format!("#[serde(try_from = \"{}\")]\n", shadow_name(ty))
}

/// The shadow type `try_from_attribute` names: the same fields, decoded
/// without checks, and a `TryFrom` that runs them.
pub fn decode_shadow(
    ty: &str,
    on_decode: ValidateOnDecode,
    members: &[Member],
    field_lines: &str,
) -> String {
    if on_decode == ValidateOnDecode::Off || !any_checks(members) {
        return String::new();
    }
    let shadow = shadow_name(ty);
    let (error, verb) = match on_decode {
        ValidateOnDecode::First => ("ValidationError", "validate_first"),
        _ => ("ValidationErrors", "validate"),
    };
    let moves: String = members
        .iter()
        .map(|m| format!("            {0}: raw.{0},\n", m.name))
        .collect();
    format!(
        "/// [`{ty}`] as decoded, before its checks run.\n\
         #[doc(hidden)]\n\
         #[allow(non_camel_case_types)]\n\
         #[derive(Deserialize)]\n\
         pub struct {shadow} {{\n{field_lines}}}\n\n\
         impl TryFrom<{shadow}> for {ty} {{\n    \
             type Error = {RT}::{error};\n\n    \
             fn try_from(raw: {shadow}) -> Result<Self, Self::Error> {{\n        \
                 let value = Self {{\n{moves}        }};\n        \
                 {RT}::Validate::{verb}(&value)?;\n        \
                 Ok(value)\n    \
             }}\n\
         }}\n\n"
    )
}

/// After a params struct `p` decodes: stop the call the way an undecodable
/// one stops when the checks fail.
pub fn params_check(stop: &[&str]) -> Vec<String> {
    let mut lines = vec![format!("                if {RT}::Validate::validate_first(&p).is_err() {{")];
    lines.extend(stop.iter().map(|l| format!("                    {l}")));
    lines.push("                }".to_string());
    lines
}

fn shadow_name(ty: &str) -> String {
    format!("__{ty}Unchecked")
}

#[derive(Default)]
struct Lines(String);

impl Lines {
    fn line(&mut self, indent: usize, text: &str) {
        for _ in 0..indent {
            self.0.push_str("    ");
        }
        self.0.push_str(text);
        self.0.push('\n');
    }
}

/// The checks on `value` (a reference) at `path` (a `String`), then on its
/// elements, then its own validation.
fn plan(out: &mut Lines, indent: usize, plan: &Plan) -> Result<()> {
    for check in &plan.checks {
        let args = check
            .params
            .iter()
            .map(|(name, value)| format!("{name} = {}", value_text(value)))
            .collect::<Vec<_>>()
            .join(", ");
        out.line(indent, &format!("// {}({args})", check.validator));
        for assert in check.asserts {
            let FrozenUnit::Assert { expr, message, .. } = assert else {
                continue;
            };
            out.line(indent, "if first && !out.is_empty() {");
            out.line(indent + 1, "return;");
            out.line(indent, "}");
            out.line(indent, &format!("if !({}) {{", condition(expr, check, &plan.base)?));
            out.line(indent + 1, &format!("out.push({RT}::ValidationError {{"));
            out.line(indent + 2, "field: path.clone(),");
            out.line(
                indent + 2,
                &format!("message: {},", message_expr(message, check, &plan.base)),
            );
            out.line(indent + 1, "});");
            out.line(indent, "}");
        }
    }
    if let Some(elements) = &plan.elements {
        out.line(indent, "for (i, value) in value.iter().enumerate() {");
        out.line(indent + 1, &format!("let path = {RT}::index_path(&path, i);"));
        self::plan(out, indent + 1, elements)?;
        out.line(indent, "}");
    }
    if plan.nested {
        out.line(indent, &format!("{RT}::Validate::validate_into(value, &path, out, first);"));
    }
    Ok(())
}

/// What an operand is, for deciding how two of them compare.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Integer,
    Float,
    Text,
    Bool,
}

fn condition(expr: &Condition, check: &Check, base: &TypeRef) -> Result<String> {
    // No precedence in the grammar: grouped left to right, so
    // `a and b or c` is `((a) && (b)) || (c)`.
    let mut text = comparison(&expr.first, check, base)?;
    for (op, next) in &expr.rest {
        let op = match op {
            LogicalOp::And => "&&",
            LogicalOp::Or => "||",
        };
        text = format!("({text}) {op} ({})", comparison(next, check, base)?);
    }
    Ok(text)
}

fn comparison(cmp: &Comparison, check: &Check, base: &TypeRef) -> Result<String> {
    let (mut left, left_kind) = operand(&cmp.left, check, base)?;
    let (mut right, right_kind) = operand(&cmp.right, check, base)?;
    match (left_kind, right_kind) {
        // Two bare literals: nothing casts either side, so name the width
        // rather than let `i32` overflow on a `u64::MAX` bound.
        (Kind::Integer, Kind::Integer) if !left.contains(" as ") && !right.contains(" as ") => {
            right = format!("{right}i128");
        }
        (a, b) if a == b => {}
        // Numbers compare by value: an integer meets a float as a float.
        (Kind::Integer, Kind::Float) => left = format!("({left} as f64)"),
        (Kind::Float, Kind::Integer) => right = format!("({right} as f64)"),
        (a, b) => {
            return Err(eyre!(
                "validator '{}': can't compare {a:?} with {b:?} in `{} {} {}`",
                check.validator,
                operand_text(&cmp.left),
                op_text(cmp.op),
                operand_text(&cmp.right)
            ))
        }
    }
    Ok(format!("{left} {} {right}", op_text(cmp.op)))
}

fn op_text(op: CompareOp) -> &'static str {
    match op {
        CompareOp::Eq => "==",
        CompareOp::Ne => "!=",
        CompareOp::Ge => ">=",
        CompareOp::Le => "<=",
        CompareOp::Gt => ">",
        CompareOp::Lt => "<",
    }
}

fn operand_text(operand: &Operand) -> String {
    match operand {
        Operand::Path(segments) => segments.join("."),
        Operand::Integer(i) => i.to_string(),
        Operand::String(s) => format!("{s:?}"),
    }
}

/// An operand as a Rust expression, and its kind.
fn operand(operand: &Operand, check: &Check, base: &TypeRef) -> Result<(String, Kind)> {
    match operand {
        Operand::Integer(i) => Ok((i.to_string(), Kind::Integer)),
        Operand::String(s) => Ok((format!("{s:?}"), Kind::Text)),
        Operand::Path(segments) => {
            let segments: Vec<&str> = segments.iter().map(String::as_str).collect();
            path_operand(&segments, check, base)
        }
    }
}

fn path_operand(segments: &[&str], check: &Check, base: &TypeRef) -> Result<(String, Kind)> {
    match segments {
        ["value"] => value_operand(base).ok_or_else(|| {
            eyre!("validator '{}': `value` of type `{base}` can't be compared", check.validator)
        }),
        ["value", "length"] => Ok((length_expr(base, check)?, Kind::Integer)),
        ["value", "name"] => Ok(("path.as_str()".to_string(), Kind::Text)),
        ["params", name] => match check.param(name) {
            Some(Value::Integer(i)) => Ok((i.to_string(), Kind::Integer)),
            Some(Value::Bool(b)) => Ok((b.to_string(), Kind::Bool)),
            Some(Value::String(s)) => Ok((format!("{s:?}"), Kind::Text)),
            None => Err(eyre!("validator '{}' has no property '{name}'", check.validator)),
        },
        _ => Err(eyre!(
            "validator '{}': `{}` isn't something a check can read",
            check.validator,
            segments.join(".")
        )),
    }
}

fn value_operand(base: &TypeRef) -> Option<(String, Kind)> {
    let p = base.as_primitive()?;
    Some(if p == PrimitiveType::String {
        ("AsRef::<str>::as_ref(value)".to_string(), Kind::Text)
    } else if p == PrimitiveType::Bool {
        ("*value".to_string(), Kind::Bool)
    } else if p.is_float() {
        ("(*value as f64)".to_string(), Kind::Float)
    } else {
        ("(*value as i128)".to_string(), Kind::Integer)
    })
}

/// `value.length`: Unicode scalar values for a string, elements for an
/// array (core fixes the meaning, so every target counts the same).
fn length_expr(base: &TypeRef, check: &Check) -> Result<String> {
    match base {
        TypeRef::Primitive(PrimitiveType::String) => Ok("(value.chars().count() as i128)".to_string()),
        TypeRef::Array { .. } => Ok("(value.len() as i128)".to_string()),
        _ => Err(eyre!("validator '{}': `value.length` isn't defined for `{base}`", check.validator)),
    }
}

/// The `format!` producing a failed check's message.
fn message_expr(template: &str, check: &Check, base: &TypeRef) -> String {
    let rendered = message::render_with(template, |placeholder| match placeholder {
        "value.name" => Some("path".to_string()),
        "value.length" => length_expr(base, check).ok(),
        "value" => match base.as_primitive() {
            Some(PrimitiveType::String) => Some("AsRef::<str>::as_ref(value)".to_string()),
            Some(_) => Some("value".to_string()),
            None => None,
        },
        _ => {
            let name = placeholder.strip_prefix("params.")?;
            check.param(name).map(value_text)
        }
    });
    if rendered.args.is_empty() {
        format!("{:?}.into()", rendered.format)
    } else {
        format!("format!({:?}, {})", rendered.format, rendered.args.join(", "))
    }
}

/// A bound property as a Rust literal: also how it prints in a message.
fn value_text(value: &Value) -> String {
    match value {
        Value::Integer(i) => i.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::String(s) => format!("{s:?}"),
    }
}
