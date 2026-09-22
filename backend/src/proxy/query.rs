use serde_json::{Map, Value};

pub fn encode(query: &Map<String, Value>) -> Result<String, String> {
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());

    for (key, value) in query {
        let values = match value {
            Value::Array(items) => items.iter().collect(),
            value => vec![value],
        };

        for value in values {
            match value {
                Value::Null => {}
                Value::String(text) => {
                    serializer.append_pair(key, text);
                }
                Value::Number(number) => {
                    serializer.append_pair(key, &number.to_string());
                }
                Value::Bool(flag) => {
                    serializer.append_pair(key, if *flag { "true" } else { "false" });
                }
                Value::Array(_) | Value::Object(_) => {
                    return Err(format!(
                        "Query parameter \"{key}\" must be a string, number, boolean or a list of those"
                    ));
                }
            }
        }
    }

    Ok(serializer.finish())
}
