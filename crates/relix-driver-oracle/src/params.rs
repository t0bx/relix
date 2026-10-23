/*
 * @author t0bx
 * @since 23.10.2026
 */

use oracle::sql_type::ToSql;
use relix_core::Value;

/// Statement parameters for the `:1, :2, …` placeholders. NULL is bound as a VARCHAR2
/// NULL and unsigned values beyond `i64` as text; Oracle converts both implicitly.
pub fn bind_values(values: &[Value]) -> Vec<Box<dyn ToSql>> {
    values
        .iter()
        .map(|value| -> Box<dyn ToSql> {
            match value {
                Value::Null => Box::new(None::<String>),
                Value::Bool(flag) => Box::new(*flag),
                Value::Int(number) => Box::new(*number),
                Value::UInt(number) => match i64::try_from(*number) {
                    Ok(number) => Box::new(number),
                    Err(_) => Box::new(number.to_string()),
                },
                Value::Float(number) => Box::new(*number),
                Value::Text(text) => Box::new(text.clone()),
                Value::Bytes(bytes) => Box::new(bytes.clone()),
            }
        })
        .collect()
}

pub fn as_params(values: &[Box<dyn ToSql>]) -> Vec<&dyn ToSql> {
    values.iter().map(|value| value.as_ref()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_value_becomes_one_parameter() {
        let values = [
            Value::Null,
            Value::Int(1),
            Value::UInt(u64::MAX),
            Value::Text("x".into()),
            Value::Bytes(vec![1]),
        ];
        let bound = bind_values(&values);
        assert_eq!(as_params(&bound).len(), values.len());
    }
}
