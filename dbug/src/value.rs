//! Arguments to edit: a form built from a D-Bus type, filled with the values of a
//! captured message or with blanks. The type is the whole schema, so nothing is kept
//! per interface: a message's own signature, or the one an introspected method gives,
//! says what the form holds.

use std::str::FromStr;

use zbus::zvariant::{Array, Dict, ObjectPath, Signature, StructureBuilder, Value};

/// One argument, being edited.
#[derive(Clone, Debug, PartialEq)]
pub enum Edit {
    Bool(bool),
    /// An integer or a double, kept as the text typed so half a number can be there.
    Num {
        kind: Signature,
        text: String,
    },
    /// A string, an object path or a signature.
    Text {
        kind: Signature,
        text: String,
    },
    /// A value that says its own type: the type, written as a signature, and the value.
    Variant {
        sig: String,
        inner: Box<Edit>,
    },
    Array {
        elem: Signature,
        items: Vec<Edit>,
    },
    Dict {
        key: Signature,
        value: Signature,
        items: Vec<(Edit, Edit)>,
    },
    Struct(Vec<Edit>),
}

impl Edit {
    /// A blank of this type: zero, empty, false. A type that can't be sent (a file
    /// descriptor) becomes an empty string, which sending then refuses by its type.
    pub fn blank(sig: &Signature) -> Edit {
        use Signature as S;
        match sig {
            S::Bool => Edit::Bool(false),
            S::U8 | S::I16 | S::U16 | S::I32 | S::U32 | S::I64 | S::U64 => Edit::Num {
                kind: sig.clone(),
                text: "0".into(),
            },
            S::F64 => Edit::Num {
                kind: sig.clone(),
                text: "0".into(),
            },
            S::Str => Edit::Text {
                kind: sig.clone(),
                text: String::new(),
            },
            S::ObjectPath => Edit::Text {
                kind: sig.clone(),
                text: "/".into(),
            },
            S::Signature => Edit::Text {
                kind: sig.clone(),
                text: String::new(),
            },
            S::Variant => Edit::Variant {
                sig: "s".into(),
                inner: Box::new(Edit::blank(&S::Str)),
            },
            S::Array(child) => Edit::Array {
                elem: child.signature().clone(),
                items: Vec::new(),
            },
            S::Dict { key, value } => Edit::Dict {
                key: key.signature().clone(),
                value: value.signature().clone(),
                items: Vec::new(),
            },
            S::Structure(fields) => Edit::Struct(fields.iter().map(Edit::blank).collect()),
            _ => Edit::Text {
                kind: S::Str,
                text: String::new(),
            },
        }
    }

    /// The form for a value read off the bus.
    pub fn from_value(value: &Value<'_>) -> Edit {
        use Signature as S;
        let num = |kind: S, text: String| Edit::Num { kind, text };
        let text = |kind: S, text: String| Edit::Text { kind, text };
        match value {
            Value::Bool(b) => Edit::Bool(*b),
            Value::U8(n) => num(S::U8, n.to_string()),
            Value::I16(n) => num(S::I16, n.to_string()),
            Value::U16(n) => num(S::U16, n.to_string()),
            Value::I32(n) => num(S::I32, n.to_string()),
            Value::U32(n) => num(S::U32, n.to_string()),
            Value::I64(n) => num(S::I64, n.to_string()),
            Value::U64(n) => num(S::U64, n.to_string()),
            Value::F64(n) => num(S::F64, n.to_string()),
            Value::Str(s) => text(S::Str, s.to_string()),
            Value::ObjectPath(p) => text(S::ObjectPath, p.to_string()),
            Value::Signature(s) => text(S::Signature, s.to_string()),
            Value::Value(inner) => Edit::Variant {
                sig: inner.value_signature().to_string(),
                inner: Box::new(Edit::from_value(inner)),
            },
            Value::Array(a) => Edit::Array {
                elem: a.element_signature().clone(),
                items: a.inner().iter().map(Edit::from_value).collect(),
            },
            Value::Dict(d) => {
                let (key, value) = match d.signature() {
                    S::Dict { key, value } => (key.signature().clone(), value.signature().clone()),
                    _ => (S::Str, S::Variant),
                };
                Edit::Dict {
                    key,
                    value,
                    items: d
                        .iter()
                        .map(|(k, v)| (Edit::from_value(k), Edit::from_value(v)))
                        .collect(),
                }
            }
            Value::Structure(s) => Edit::Struct(s.fields().iter().map(Edit::from_value).collect()),
            other => text(S::Str, format!("{other}")),
        }
    }

    /// The value to send, or what's wrong with the form.
    pub fn to_value(&self) -> Result<Value<'static>, String> {
        use Signature as S;
        fn n<T: FromStr>(text: &str, what: &str) -> Result<T, String> {
            text.trim()
                .parse()
                .map_err(|_| format!("“{text}” isn't a {what}"))
        }
        Ok(match self {
            Edit::Bool(b) => Value::Bool(*b),
            Edit::Num { kind, text } => match kind {
                S::U8 => Value::U8(n(text, "byte (0 to 255)")?),
                S::I16 => Value::I16(n(text, "16-bit integer")?),
                S::U16 => Value::U16(n(text, "unsigned 16-bit integer")?),
                S::I32 => Value::I32(n(text, "32-bit integer")?),
                S::U32 => Value::U32(n(text, "unsigned 32-bit integer")?),
                S::I64 => Value::I64(n(text, "64-bit integer")?),
                S::U64 => Value::U64(n(text, "unsigned 64-bit integer")?),
                _ => Value::F64(n(text, "number")?),
            },
            Edit::Text { kind, text } => match kind {
                S::ObjectPath => Value::ObjectPath(
                    ObjectPath::try_from(text.clone())
                        .map_err(|e| format!("“{text}” isn't an object path: {e}"))?,
                ),
                S::Signature => Value::Signature(
                    Signature::from_str(text)
                        .map_err(|e| format!("“{text}” isn't a signature: {e}"))?,
                ),
                _ => Value::from(text.clone()),
            },
            Edit::Variant { sig, inner } => {
                Signature::from_str(sig).map_err(|e| format!("“{sig}” isn't a type: {e}"))?;
                Value::Value(Box::new(inner.to_value()?))
            }
            Edit::Array { elem, items } => {
                let mut array = Array::new(elem);
                for item in items {
                    array.append(item.to_value()?).map_err(|e| e.to_string())?;
                }
                Value::Array(array)
            }
            Edit::Dict { key, value, items } => {
                let mut dict = Dict::new(key, value);
                for (k, v) in items {
                    dict.append(k.to_value()?, v.to_value()?)
                        .map_err(|e| e.to_string())?;
                }
                Value::Dict(dict)
            }
            Edit::Struct(fields) => {
                let mut b = StructureBuilder::new();
                for f in fields {
                    b = b.append_field(f.to_value()?);
                }
                Value::Structure(b.build().map_err(|e| e.to_string())?)
            }
        })
    }

    /// The type, as a signature.
    pub fn signature(&self) -> String {
        match self.to_value() {
            Ok(v) => v.value_signature().to_string(),
            Err(_) => "?".into(),
        }
    }

    /// Draws the form; `true` if anything changed.
    pub fn show(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        match self {
            Edit::Bool(b) => changed |= ui.checkbox(b, if *b { "true" } else { "false" }).changed(),
            Edit::Num { kind, text } => {
                let r = ui.add(
                    egui::TextEdit::singleline(text)
                        .desired_width(140.0)
                        .hint_text(type_name(kind)),
                );
                changed |= r.changed();
                r.on_hover_text(type_name(kind));
            }
            Edit::Text { kind, text } => {
                let r = ui.add(
                    egui::TextEdit::singleline(text)
                        .desired_width(320.0)
                        .hint_text(type_name(kind)),
                );
                changed |= r.changed();
                r.on_hover_text(type_name(kind));
            }
            Edit::Variant { sig, inner } => {
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.weak("holds a");
                        let r = ui.add(egui::TextEdit::singleline(sig).desired_width(70.0)).on_hover_text(
                            "The type of what this holds, as a D-Bus signature: s string, u unsigned \
                             integer, i integer, b boolean, d number, o object path, as list of strings, \
                             a{sv} dictionary. Changing it starts a blank of that type.",
                        );
                        if r.changed()
                            && let Ok(parsed) = Signature::from_str(sig)
                        {
                            **inner = Edit::blank(&parsed);
                            changed = true;
                        }
                    });
                    changed |= inner.show(ui);
                });
            }
            Edit::Array { elem, items } => {
                ui.vertical(|ui| {
                    let mut gone = None;
                    for (i, item) in items.iter_mut().enumerate() {
                        ui.push_id(i, |ui| {
                            ui.horizontal(|ui| {
                                ui.weak(format!("{i}"));
                                changed |= item.show(ui);
                                if ui
                                    .small_button("✕")
                                    .on_hover_text("Remove this one")
                                    .clicked()
                                {
                                    gone = Some(i);
                                }
                            });
                        });
                    }
                    if let Some(i) = gone {
                        items.remove(i);
                        changed = true;
                    }
                    if ui
                        .small_button(format!("+ Add {}", type_name(elem)))
                        .clicked()
                    {
                        items.push(Edit::blank(elem));
                        changed = true;
                    }
                });
            }
            Edit::Dict { key, value, items } => {
                ui.vertical(|ui| {
                    let mut gone = None;
                    for (i, (k, v)) in items.iter_mut().enumerate() {
                        ui.push_id(i, |ui| {
                            ui.horizontal(|ui| {
                                changed |= k.show(ui);
                                ui.weak("→");
                                changed |= v.show(ui);
                                if ui
                                    .small_button("✕")
                                    .on_hover_text("Remove this entry")
                                    .clicked()
                                {
                                    gone = Some(i);
                                }
                            });
                        });
                    }
                    if let Some(i) = gone {
                        items.remove(i);
                        changed = true;
                    }
                    if ui
                        .small_button("+ Add entry")
                        .on_hover_text(format!("{} → {}", type_name(key), type_name(value)))
                        .clicked()
                    {
                        items.push((Edit::blank(key), Edit::blank(value)));
                        changed = true;
                    }
                });
            }
            Edit::Struct(fields) => {
                ui.vertical(|ui| {
                    for (i, f) in fields.iter_mut().enumerate() {
                        ui.push_id(i, |ui| changed |= f.show(ui));
                    }
                });
            }
        }
        changed
    }
}

/// A type in words, for hints.
pub fn type_name(sig: &Signature) -> String {
    use Signature as S;
    match sig {
        S::Bool => "boolean".into(),
        S::U8 => "byte".into(),
        S::I16 => "16-bit integer".into(),
        S::U16 => "unsigned 16-bit integer".into(),
        S::I32 => "32-bit integer".into(),
        S::U32 => "unsigned 32-bit integer".into(),
        S::I64 => "64-bit integer".into(),
        S::U64 => "unsigned 64-bit integer".into(),
        S::F64 => "number".into(),
        S::Str => "string".into(),
        S::ObjectPath => "object path".into(),
        S::Signature => "signature".into(),
        S::Variant => "variant".into(),
        S::Array(c) => format!("list of {}", type_name(c.signature())),
        S::Dict { key, value } => format!(
            "dictionary of {} to {}",
            type_name(key.signature()),
            type_name(value.signature())
        ),
        S::Structure(_) => "struct".into(),
        other => other.to_string(),
    }
}

/// What the arguments' signature is, in the way a message carries it.
pub fn signature_of(args: &[Edit]) -> String {
    args.iter().map(Edit::signature).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sig(s: &str) -> Signature {
        Signature::from_str(s).unwrap()
    }

    #[test]
    fn a_blank_of_each_type_makes_a_value_of_that_type() {
        for s in [
            "b",
            "y",
            "n",
            "q",
            "i",
            "u",
            "x",
            "t",
            "d",
            "s",
            "o",
            "g",
            "v",
            "as",
            "a{sv}",
            "(su)",
            "a(ss)",
            "a{sa{sv}}",
        ] {
            let value = Edit::blank(&sig(s)).to_value().unwrap();
            assert_eq!(
                value.value_signature().to_string(),
                sig(s).to_string(),
                "for {s}"
            );
        }
    }

    #[test]
    fn a_value_from_the_bus_goes_through_the_form_and_back() {
        let mut dict = Dict::new(&sig("s"), &sig("v"));
        dict.append(Value::from("volume"), Value::Value(Box::new(Value::U32(7))))
            .unwrap();
        let original = Value::Dict(dict);
        let edit = Edit::from_value(&original);
        assert_eq!(edit.to_value().unwrap(), original);
    }

    #[test]
    fn a_number_that_isnt_one_says_so() {
        let bad = Edit::Num {
            kind: sig("u"),
            text: "-3".into(),
        };
        let err = bad.to_value().unwrap_err();
        assert!(err.contains("unsigned 32-bit integer"), "{err}");
        let ok = Edit::Num {
            kind: sig("u"),
            text: " 12 ".into(),
        };
        assert_eq!(ok.to_value().unwrap(), Value::U32(12));
    }

    #[test]
    fn a_bad_object_path_is_refused() {
        let bad = Edit::Text {
            kind: sig("o"),
            text: "not/a/path".into(),
        };
        assert!(bad.to_value().is_err());
    }

    #[test]
    fn an_array_takes_items_of_its_type() {
        let mut edit = Edit::blank(&sig("au"));
        if let Edit::Array { elem, items } = &mut edit {
            items.push(Edit::blank(elem));
            items.push(Edit::Num {
                kind: sig("u"),
                text: "5".into(),
            });
        }
        assert_eq!(edit.signature(), "au");
        let Value::Array(a) = edit.to_value().unwrap() else {
            panic!()
        };
        assert_eq!(a.inner(), &[Value::U32(0), Value::U32(5)]);
    }
}
