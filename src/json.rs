use std::io::{self, Write};

pub(crate) enum Value<'a> {
    String(&'a str),
    Number(u64),
    Null,
}

pub(crate) fn emit(event: &str, fields: &[(&str, Value<'_>)]) {
    let output = line(event, fields);
    let mut stdout = io::stdout().lock();
    let _ = writeln!(stdout, "{output}");
}

pub(crate) fn line(event: &str, fields: &[(&str, Value<'_>)]) -> String {
    let mut output = String::from("{\"schema\":\"ii.event/v1\",\"event\":");
    push_string(&mut output, event);
    for (name, value) in fields {
        output.push(',');
        push_string(&mut output, name);
        output.push(':');
        match value {
            Value::String(value) => push_string(&mut output, value),
            Value::Number(value) => output.push_str(&value.to_string()),
            Value::Null => output.push_str("null"),
        }
    }
    output.push('}');
    output
}

pub(crate) fn started(operation: &str) {
    emit("started", &[("operation", Value::String(operation))]);
}

pub(crate) fn completed(operation: &str) {
    emit("completed", &[("operation", Value::String(operation))]);
}

pub(crate) fn error(operation: &str, message: &str) {
    emit(
        "error",
        &[
            ("operation", Value::String(operation)),
            ("message", Value::String(message)),
        ],
    );
}

pub(crate) fn progress(operation: &str, bytes: u64) {
    emit(
        "progress",
        &[
            ("operation", Value::String(operation)),
            ("bytes", Value::Number(bytes)),
        ],
    );
}

fn push_string(output: &mut String, value: &str) {
    output.push('"');
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            character if character.is_control() => {
                output.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => output.push(character),
        }
    }
    output.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_escape_json_controls() {
        let mut output = String::new();
        push_string(&mut output, "quote\" slash\\ newline\n\u{0001}");
        assert_eq!(output, "\"quote\\\" slash\\\\ newline\\n\\u0001\"");
    }

    #[test]
    fn lines_have_the_fixed_schema() {
        let value: serde_json::Value =
            serde_json::from_str(&line("ticket", &[("ticket", Value::String("ii1example"))]))
                .unwrap();
        assert_eq!(value["schema"], "ii.event/v1");
        assert_eq!(value["event"], "ticket");
        assert_eq!(value["ticket"], "ii1example");
    }
}
