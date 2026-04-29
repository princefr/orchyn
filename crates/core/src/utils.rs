use serde_json::Value;

pub fn extract_json(response: &str) -> std::result::Result<Value, String> {
    let start = response
        .find('{')
        .ok_or_else(|| "no '{' found".to_string())?;
    let bytes = response.as_bytes();
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut escape = false;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if in_string {
            if escape {
                escape = false;
            } else if b == b'\\' {
                escape = true;
            } else if b == b'"' {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    let slice = &response[start..=i];
                    return serde_json::from_str(slice).map_err(|e| format!("invalid JSON: {}", e));
                }
            }
            _ => {}
        }
    }
    Err("unbalanced braces".to_string())
}
