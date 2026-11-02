//! Flag kinds and values, with parsing from TOML and from `--set id=value`.

use std::path::PathBuf;

use toml_edit::{Array, Value};

use crate::error::SettingsError;

/// The declared type of a flag (DEVELOPER-MODE.md §3: `bool`, `enum`, `u8/u16/u32`, `string`,
/// `keys`, `list<path>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlagKind {
    Bool,
    Int {
        min: u32,
        max: u32,
    },
    Enum(&'static [&'static str]),
    /// A keystroke in GPUI syntax (`ctrl-g`); empty = unbound.
    Keys,
    Text,
    Paths,
}

/// A flag value in kind-independent form.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FlagValue {
    Bool(bool),
    Int(u32),
    /// Enum variant, keystroke or free text.
    Text(String),
    Paths(Vec<PathBuf>),
}

impl FlagKind {
    /// Validates a TOML value for this kind.
    pub fn from_toml(self, id: &str, value: &Value) -> Result<FlagValue, SettingsError> {
        let invalid = || SettingsError::InvalidValue {
            id: id.to_string(),
            value: value.to_string().trim().to_string(),
            expected: self.describe(),
        };
        match self {
            Self::Bool => value.as_bool().map(FlagValue::Bool).ok_or_else(invalid),
            Self::Int { .. } => {
                let n = value.as_integer().ok_or_else(invalid)?;
                let n = u32::try_from(n).map_err(|_| invalid())?;
                self.check(id, FlagValue::Int(n))
            }
            Self::Enum(_) | Self::Keys | Self::Text => {
                let s = value.as_str().ok_or_else(invalid)?;
                self.check(id, FlagValue::Text(s.to_string()))
            }
            Self::Paths => {
                let array = value.as_array().ok_or_else(invalid)?;
                let paths = array
                    .iter()
                    .map(|v| v.as_str().map(PathBuf::from).ok_or_else(invalid))
                    .collect::<Result<_, _>>()?;
                Ok(FlagValue::Paths(paths))
            }
        }
    }

    /// Parses the right-hand side of `--set id=value`.
    pub fn parse_cli(self, id: &str, raw: &str) -> Result<FlagValue, SettingsError> {
        let invalid = || SettingsError::InvalidValue {
            id: id.to_string(),
            value: raw.to_string(),
            expected: self.describe(),
        };
        let value = match self {
            Self::Bool => match raw.to_ascii_lowercase().as_str() {
                "true" | "on" | "1" | "yes" => FlagValue::Bool(true),
                "false" | "off" | "0" | "no" => FlagValue::Bool(false),
                _ => return Err(invalid()),
            },
            Self::Int { .. } => FlagValue::Int(raw.parse().map_err(|_| invalid())?),
            Self::Enum(_) | Self::Keys | Self::Text => FlagValue::Text(raw.to_string()),
            Self::Paths => FlagValue::Paths(
                raw.split(';')
                    .filter(|p| !p.is_empty())
                    .map(PathBuf::from)
                    .collect(),
            ),
        };
        self.check(id, value)
    }

    /// Range and variant checks shared by every input path.
    pub fn check(self, id: &str, value: FlagValue) -> Result<FlagValue, SettingsError> {
        let ok = match (self, &value) {
            (Self::Int { min, max }, FlagValue::Int(n)) => (min..=max).contains(n),
            (Self::Enum(variants), FlagValue::Text(s)) => variants.contains(&s.as_str()),
            (Self::Bool, FlagValue::Bool(_))
            | (Self::Keys | Self::Text, FlagValue::Text(_))
            | (Self::Paths, FlagValue::Paths(_)) => true,
            _ => false,
        };
        if ok {
            Ok(value)
        } else {
            Err(SettingsError::InvalidValue {
                id: id.to_string(),
                value: value.to_string(),
                expected: self.describe(),
            })
        }
    }

    pub fn describe(self) -> String {
        match self {
            Self::Bool => "true or false".into(),
            Self::Int { min, max } => format!("an integer {min}..={max}"),
            Self::Enum(variants) => format!("one of {}", variants.join(", ")),
            Self::Keys => "a keystroke such as \"ctrl-g\"".into(),
            Self::Text => "a string".into(),
            Self::Paths => "a list of paths".into(),
        }
    }
}

impl FlagValue {
    pub fn to_toml(&self) -> Value {
        match self {
            Self::Bool(b) => Value::from(*b),
            Self::Int(n) => Value::from(i64::from(*n)),
            Self::Text(s) => Value::from(s.as_str()),
            Self::Paths(paths) => {
                let mut array = Array::new();
                for path in paths {
                    array.push(path.to_string_lossy().as_ref());
                }
                Value::Array(array)
            }
        }
    }
}

impl std::fmt::Display for FlagValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bool(b) => write!(f, "{b}"),
            Self::Int(n) => write!(f, "{n}"),
            Self::Text(s) => f.write_str(s),
            Self::Paths(paths) => {
                let joined: Vec<_> = paths.iter().map(|p| p.display().to_string()).collect();
                f.write_str(&joined.join(";"))
            }
        }
    }
}

/// Conversion between a typed `Features` field and [`FlagValue`].
pub trait FlagField: Sized {
    fn to_value(&self) -> FlagValue;
    fn from_value(value: &FlagValue, kind: FlagKind) -> Option<Self>;
}

impl FlagField for bool {
    fn to_value(&self) -> FlagValue {
        FlagValue::Bool(*self)
    }
    fn from_value(value: &FlagValue, _: FlagKind) -> Option<Self> {
        match value {
            FlagValue::Bool(b) => Some(*b),
            _ => None,
        }
    }
}

macro_rules! int_field {
    ($($ty:ty),*) => {$(
        impl FlagField for $ty {
            fn to_value(&self) -> FlagValue {
                FlagValue::Int(u32::from(*self))
            }
            fn from_value(value: &FlagValue, _: FlagKind) -> Option<Self> {
                match value {
                    FlagValue::Int(n) => <$ty>::try_from(*n).ok(),
                    _ => None,
                }
            }
        }
    )*};
}
int_field!(u8, u16, u32);

impl FlagField for String {
    fn to_value(&self) -> FlagValue {
        FlagValue::Text(self.clone())
    }
    fn from_value(value: &FlagValue, _: FlagKind) -> Option<Self> {
        match value {
            FlagValue::Text(s) => Some(s.clone()),
            _ => None,
        }
    }
}

/// Enum flags are stored as the matching `'static` variant from their declaration.
impl FlagField for &'static str {
    fn to_value(&self) -> FlagValue {
        FlagValue::Text((*self).to_string())
    }
    fn from_value(value: &FlagValue, kind: FlagKind) -> Option<Self> {
        match (value, kind) {
            (FlagValue::Text(s), FlagKind::Enum(variants)) => {
                variants.iter().copied().find(|v| *v == s)
            }
            _ => None,
        }
    }
}

impl FlagField for Vec<PathBuf> {
    fn to_value(&self) -> FlagValue {
        FlagValue::Paths(self.clone())
    }
    fn from_value(value: &FlagValue, _: FlagKind) -> Option<Self> {
        match value {
            FlagValue::Paths(paths) => Some(paths.clone()),
            _ => None,
        }
    }
}
