use std::{
    cmp::Ordering,
    collections::HashMap,
    path::Path,
};
use num::Num;
use serde::{Deserialize, Serialize, ser::SerializeMap};
use evalexpr::HashMapContext;

/// An input parameter which can be varied by the user in the protocol section.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct ValueRange<T: Num + PartialOrd + Copy + Default + IsInteger> {
    pub default: T,
    pub min: Option<T>,
    pub max: Option<T>,
    #[serde(serialize_with="ValueRange::<T>::serialize_dec_places")]
    pub dec_places: Option<u16>,
    pub increment: Option<T>,
    pub display_units: Option<String>,
    pub display_name: Option<String>,
    #[serde(default="force_number_box_default")]
    pub force_number_box: bool,
}
fn force_number_box_default() -> bool { false }

/// Helper trait for serialization of ValueRange.
/// TODO: this could be a lot cleaner/more general once rust supports specialization
pub trait IsInteger {
    const VAL: bool;
}
impl IsInteger for i64 {
    const VAL: bool = true;
}
impl IsInteger for f64 {
    const VAL: bool = false;
}

impl<T: Num + PartialOrd + Copy + Default + IsInteger> ValueRange<T> {
    /// Serialize integers with 0 as dec_places, but floats can have null for no limit
    fn serialize_dec_places<S>(dec_places: &Option<u16>, serializer: S) -> std::prelude::v1::Result<S::Ok, S::Error>
        where
            S: serde::Serializer {
        if <T as IsInteger>::VAL {
            Some(0u16).serialize(serializer)
        } else {
            dec_places.serialize(serializer)
        }
    }
}

impl<T: Num + PartialOrd + Copy + Default + IsInteger> ValueRange<T> {
    /// Apply min/max boundings if set
    fn clamp(&self, mut val: T) -> T {
        if let Some(min) = self.min {
            match val.partial_cmp(&min) {
                Some(Ordering::Less) | None => { val = min; }
                _ => {},
            }
        }
        if let Some(max) = self.max {
            match val.partial_cmp(&max) {
                Some(Ordering::Greater) | None => { val = max; }
                _ => {},
            }
        }
        val
    }
}

impl ValueRange<i64> {
    /// Sanitize a json value based on specified bounds
    fn apply(&self, val: &serde_json::Value) -> serde_json::Value {
        use serde_json::Value;
        match val {
            Value::Number(v) if v.is_i64() => {
                Value::Number(self.clamp(v.as_i64().unwrap()).into())
            }
            other => {
                log::warn!("Invalid value {other}. Defaulting to {}", self.default);
                Value::Number(self.default.into())
            }
        }
    }
}

impl ValueRange<f64> {
    /// Sanitize a json value based on specified bounds
    fn apply(&self, val: &serde_json::Value) -> serde_json::Value {
        use serde_json::{Number, Value};
        match val {
            Value::Number(v) if v.as_f64().is_some() => {
                // v can be f64, and can't be infinite or NaN, so safe to unwrap
                Value::Number(Number::from_f64(self.clamp(v.as_f64().unwrap())).unwrap())
            }
            other => {
                log::warn!("Invalid value {other}. Defaulting to {}", self.default);
                if let Some(v) = Number::from_f64(self.default) {
                    Value::Number(v)
                } else {
                    log::error!("Couldn't convert default value {} to Number", self.default);
                    Value::Null
                }
            }
        }
    }
}


/// Settings to be configured by the user, calculated, or passed through
#[derive(Deserialize, Debug, Clone, PartialEq)]
#[serde(untagged)]
pub enum ConfigSettingsValue {
    /// A possibly bounded integer, input by user through frontend
    IntRange(ValueRange<i64>),
    /// A possibly bounded float, input by user thorugh frontend
    FloatRange(ValueRange<f64>),
    /// A field to be calculated based on user input and any literals.
    /// Cannot depend on other formula fields.
    Formula {
        #[serde(deserialize_with="precompile_formula")]
        formula: evalexpr::Node
    },
    /// A literal to be passed through. May be used by formulas.
    Literal(serde_json::Value),
}

/// Deserialize a string formula into a compiled operator tree ready for evaluation
fn precompile_formula<'de, D: serde::Deserializer<'de>>(d: D) -> Result<evalexpr::Node, D::Error> {
    use serde::de::Error;
    evalexpr::build_operator_tree(&String::deserialize(d)?).map_err(|e| {
        Error::custom(e)
    })
}

/// Convert `evalexpr::Value` to `serde_json::Value`
fn evalexpr_value_to_serde_json(v: &evalexpr::Value) -> serde_json::Value {
    match v {
        evalexpr::Value::Empty => serde_json::Value::Null,
        evalexpr::Value::String(s) => serde_json::Value::String(s.clone()),
        evalexpr::Value::Int(i) => serde_json::Value::Number((*i).into()),
        evalexpr::Value::Float(f) => {
            if let Some(f) = serde_json::Number::from_f64(*f) {
                serde_json::Value::Number(f)
            } else {
                serde_json::Value::Null
            }
        },
        evalexpr::Value::Boolean(b) => serde_json::Value::Bool(*b),
        evalexpr::Value::Tuple(t) => serde_json::Value::Array(
            t.iter().map(evalexpr_value_to_serde_json).collect()),
    }
}

/// Build an `evalexpr::HashMapContext` from a `HashMap` of named `serde_json::Value`s
fn build_evalexpr_ctx_from_settings(settings: &HashMap<String, serde_json::Value>) -> HashMapContext {
    let mut ctx = HashMapContext::new();
    for (k, v) in settings {
        if let Err(e) = add_setting_to_ctx(&k, &v, &mut ctx) {
            log::error!("Error adding setting {k} to context for use as variable in formulas: {e}. Skipping.");
        }
    }
    ctx
}

/// Add a named value to an `evalexpr::HashMapContext`
fn add_setting_to_ctx(k: &str, v: &serde_json::Value, ctx: &mut HashMapContext) -> evalexpr::EvalexprResult<()> {
    use serde_json::Value;
        match v {
        Value::Number(n) => {
            if let Some(n) = n.as_i64() {
                _ = evalexpr::eval_with_context_mut(&format!("{k}={n};"), ctx)?;
            } else if let Some(n) = n.as_u64() {
                _ = evalexpr::eval_with_context_mut(&format!("{k}={n};"), ctx)?;
            } else if let Some(n) = n.as_f64() {
                _ = evalexpr::eval_with_context_mut(&format!("{k}={n};"), ctx)?;
            }
        },
        Value::String(s) => _ = evalexpr::eval_with_context_mut(&format!("{k}=\"{s}\";"), ctx)?,
        Value::Bool(b) => _ = evalexpr::eval_with_context_mut(&format!("{k}={b};"), ctx)?,
        Value::Object(_) | Value::Array(_) => return Err(evalexpr::EvalexprError::CustomMessage("Eval of objects and arrays not supported!".into())),
        Value::Null => _ = evalexpr::eval_with_context_mut(&format!("{k}=();"), ctx)?,
    }
    Ok(())
}

impl ConfigSettingsValue {
    /// Apply sanitization to `val`, or compute it if this is a formula.
    /// `IntRange` and `FloatRange` apply sanitization, `Literal` overwrites `val` with the
    /// stored literal value, `Formula` overwrites `val` with the result of the formula.
    /// `ctx` only required if evaluating `Formula` variants which depend on other values.
    fn apply(&self, val: &mut serde_json::Value, ctx: Option<&evalexpr::HashMapContext>) -> anyhow::Result<()> {
        match self {
            Self::IntRange(range) => *val = range.apply(val),
            Self::FloatRange(range) => *val = range.apply(val),
            Self::Literal(v) => {
                log::warn!("Overwriting value literal in configuration: {val} -> {v}");
                *val = v.clone();
            },
            Self::Formula { formula: f } => {
                if let Some(ctx) = ctx {
                    *val = evalexpr_value_to_serde_json(&f.eval_with_context(ctx)?);
                } else {
                    *val = evalexpr_value_to_serde_json(&f.eval()?);
                }
            },
        }
        Ok(())
    }

    /// Get the default value if `self` is an `IntRange` or `FloatRange`, otherwise
    /// returns the stored literal value if `Literal`, or calculated result if `Formula`.
    /// `ctx` only required if evaluating `Formula` variants which depend on other values.
    fn get(&self, ctx: Option<&evalexpr::HashMapContext>) -> anyhow::Result<serde_json::Value> {
        use serde_json::{Value, Number};
        match self {
            Self::IntRange(range) => Ok(Value::Number(range.default.into())),
            Self::FloatRange(range) => Ok(
                if range.default.is_finite() {
                    Value::Number(Number::from_f64(range.default).unwrap())
                } else { Value::Null }
            ),
            Self::Literal(v) => Ok(v.clone()),
            Self::Formula { formula: f } => {
                if let Some(ctx) = ctx {
                    Ok(evalexpr_value_to_serde_json(&f.eval_with_context(ctx)?))
                } else {
                    Ok(evalexpr_value_to_serde_json(&f.eval()?))
                }
            }
        }
    }
}

impl Serialize for ConfigSettingsValue {
    fn serialize<S>(&self, serializer: S) -> std::prelude::v1::Result<S::Ok, S::Error>
        where
            S: serde::Serializer {
        match self {
            Self::Literal(v) => v.serialize(serializer),
            Self::IntRange(range) => range.serialize(serializer),
            Self::FloatRange(range) => range.serialize(serializer),
            // Formula should have been computed before calling serialize,
            // but don't want to panic here just in case.
            Self::Formula { formula } => serializer.serialize_str(&formula.to_string()),
        }
    }
}

/// Set of named `ConfigSettingsValue` fields, read from (e.g.) `resources/input_config.yml`.
#[derive(Deserialize, Debug, Clone, Default)]
pub struct ConfigSettings {
    #[serde(flatten)]
    pub settings: HashMap<String, ConfigSettingsValue>,
}

impl Serialize for ConfigSettings {
    fn serialize<S>(&self, serializer: S) -> std::prelude::v1::Result<S::Ok, S::Error>
        where
            S: serde::Serializer {
        // Serialize called when sending to frontend, so only
        // send settings to be configured by the frontend user.
        let mut map = serializer.serialize_map(None)?;
        for (k, v) in &self.settings {
            match v {
                ConfigSettingsValue::FloatRange(_)
                    | ConfigSettingsValue::IntRange(_)
                    => map.serialize_entry(k, v)?,
                _ => ()
            }
        }
        map.end()
    }
}

impl ConfigSettings {
    /// Load input configuration from a yaml file.
    pub fn open(yml_file: impl AsRef<Path>) -> anyhow::Result<Self> {
        let config: ConfigSettings = serde_yaml::from_reader(
            std::fs::OpenOptions::new().read(true).open(yml_file)?)?;
        Ok(config)
    }

    /// Apply sanitization to json values with names corresponding
    /// to keys in `self.settings`. Any missing fields are inserted,
    /// and formulas are calculated.
    pub fn apply(&self, input_config: &mut HashMap<String, serde_json::Value>) {
        let mut formulas = false;
        for (k, v) in &self.settings {
            if let ConfigSettingsValue::Formula{..} = v {
                formulas = true;
                continue
            }
            if let Some(val) = input_config.get_mut(k) {
                v.apply(val, None).expect("Applying setting should never fail for literals or ranges");
            } else {
                input_config.insert(k.clone(), v.get(None).unwrap());
            }
        }
        if formulas {
            let ctx = build_evalexpr_ctx_from_settings(&input_config);
            for (k, v) in &self.settings {
                if let ConfigSettingsValue::Formula{..} = v {
                    if let Ok(val) = v.get(Some(&ctx)) {
                        _ = input_config.insert(k.clone(), val);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn test_config_settings_deserialize() {
        use serde_json::Value;
        let yml = concat!(
            "my_str: Hello, world!\n",
            "my_int: 12\n",
            "my_float: 3.14\n",
            "flt_range:\n",
            "  default: 1.0\n",
            "  dec_places: 2\n",
            "int_range:\n",
            "  default: 10\n",
            "  min: 1\n",
            "  max: 20\n",
            "fn:\n",
            "  formula: my_int + my_float\n",
            "fn2:\n",
            "  formula: flt_range * int_range + my_float",
        );
        let settings: ConfigSettings = serde_yaml::from_str(yml).unwrap();
        assert_eq!(settings.settings["my_str"], ConfigSettingsValue::Literal(Value::from("Hello, world!")));
        assert_eq!(settings.settings["my_int"], ConfigSettingsValue::Literal(Value::from(12)));
        assert_eq!(settings.settings["my_float"], ConfigSettingsValue::Literal(Value::from(3.14)));
        assert_eq!(settings.settings["flt_range"], ConfigSettingsValue::FloatRange(
            ValueRange {
                default: 1.0,
                dec_places: Some(2),
                ..Default::default()
            }
        ));
        assert_eq!(settings.settings["int_range"], ConfigSettingsValue::IntRange(
            ValueRange {
                default: 10,
                min: Some(1),
                max: Some(20),
                ..Default::default()
            }
        ));
        let input = concat!(
            "flt_range: 2\n",
            "int_range: 23\n",
            "my_int: 4\n",
        );
        let mut config: HashMap<String, Value> = serde_yaml::from_str(input).unwrap();
        settings.apply(&mut config);
        assert_eq!(config["flt_range"], Value::from(2.0));
        assert_eq!(config["int_range"], Value::from(20));
        assert_eq!(config["fn"], Value::from(12. + 3.14));
        assert_eq!(config["fn2"], Value::from(2.0 * 20.0 + 3.14));
    }

    #[test]
    fn test_config_settings_serialize() {
        use serde_json::Value;
        let json = serde_json::to_string(&ConfigSettingsValue::Literal(Value::from(12))).unwrap();
        assert_eq!(json, "12");

        let json = serde_json::to_string(&ConfigSettingsValue::IntRange( ValueRange {
                default: 10,
                min: Some(1),
                max: Some(20),
                ..Default::default()
            }
        )).unwrap();
        assert_eq!(json, "{\"default\":10,\"min\":1,\"max\":20,\"dec_places\":0,\"increment\":null,\"display_units\":null,\"display_name\":null,\"force_number_box\":false}");

        let json = serde_json::to_string(&ConfigSettingsValue::FloatRange( ValueRange {
                default: 10.,
                dec_places: Some(2),
                ..Default::default()
            }
        )).unwrap();
        assert_eq!(json, "{\"default\":10.0,\"min\":null,\"max\":null,\"dec_places\":2,\"increment\":null,\"display_units\":null,\"display_name\":null,\"force_number_box\":false}");
    }

    #[test]
    fn test_dec_places() {
        let json = serde_json::to_string(&ValueRange::<i64>::default()).unwrap();
        let vr: ValueRange<i64> = serde_json::from_str(&json).unwrap();
        assert_eq!(vr.dec_places, Some(0));
    }
}

